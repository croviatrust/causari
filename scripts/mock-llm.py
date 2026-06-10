"""Tiny mock LLM upstream used by the e2e capture demo/test.

Answers any POST with a fixed OpenAI-style chat completion that contains a
recognizable code block, so `re proxy` + `re watch` can demonstrate the
content-based causal join without real API keys.
"""

import json
from http.server import BaseHTTPRequestHandler, HTTPServer

COMPLETION = (
    "Here is the fix:\n"
    "```python\n"
    "def refresh_token(user):\n"
    "    token = issue_token(user, scope=\"session\")\n"
    "    return rotate_every(token, hours=24)\n"
    "```\n"
)


class Handler(BaseHTTPRequestHandler):
    def do_POST(self):
        length = int(self.headers.get("content-length", 0))
        self.rfile.read(length)
        body = json.dumps(
            {
                "id": "chatcmpl-mock",
                "object": "chat.completion",
                "model": "gpt-4o-mock",
                "choices": [
                    {
                        "index": 0,
                        "message": {"role": "assistant", "content": COMPLETION},
                        "finish_reason": "stop",
                    }
                ],
                "usage": {"prompt_tokens": 42, "completion_tokens": 18},
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
    print("mock LLM upstream on http://127.0.0.1:4399")
    HTTPServer(("127.0.0.1", 4399), Handler).serve_forever()
