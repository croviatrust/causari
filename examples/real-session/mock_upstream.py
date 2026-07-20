"""Configurable OpenAI-compatible upstream for the Causari real-session test.

Unlike scripts/mock-llm.py (which returns a single fixed body), this upstream
replies with whatever completion text is currently in COMPLETION_FILE. The
test harness overwrites that file before each proxy call, so every scenario
gets a *different, real* model output — and we control exactly how the code the
"agent" writes to disk diverges from what the "model" returned. That divergence
is the whole point: it exercises the causal join on dirty cases, not the trivial
happy path where file == completion.

The transport is local (no API key); the CONTENT and the file/completion
correspondence are the variables under test.
"""

import json
import os
from http.server import BaseHTTPRequestHandler, HTTPServer

COMPLETION_FILE = os.environ.get("COMPLETION_FILE", "_next_completion.txt")
PORT = int(os.environ.get("MOCK_PORT", "4399"))


class Handler(BaseHTTPRequestHandler):
    def do_POST(self):
        length = int(self.headers.get("content-length", 0))
        self.rfile.read(length)

        try:
            with open(COMPLETION_FILE, "r", encoding="utf-8") as fh:
                completion = fh.read()
        except FileNotFoundError:
            completion = "(no completion configured)"

        body = json.dumps(
            {
                "id": "chatcmpl-realsession",
                "object": "chat.completion",
                "model": "gpt-4o-realsession",
                "choices": [
                    {
                        "index": 0,
                        "message": {"role": "assistant", "content": completion},
                        "finish_reason": "stop",
                    }
                ],
                "usage": {
                    "prompt_tokens": 64,
                    "completion_tokens": max(1, len(completion) // 4),
                },
            }
        ).encode()
        self.send_response(200)
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *args):
        pass


if __name__ == "__main__":
    print(f"mock upstream on http://127.0.0.1:{PORT} (completion <- {COMPLETION_FILE})")
    HTTPServer(("127.0.0.1", PORT), Handler).serve_forever()
