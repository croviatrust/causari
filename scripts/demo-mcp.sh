#!/usr/bin/env bash
# MCP server demo for Linux / macOS. Pipes 5 JSON-RPC requests at the server.
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
RE="$HERE/../target/debug/re"
if [[ ! -x "$RE" ]]; then RE="$HERE/../target/release/re"; fi

cd "$HERE/.."
rm -rf demo
mkdir demo
cd demo

"$RE" init >/dev/null
echo 'hello' > a.txt

REQS=$(cat <<'EOF'
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"demo-agent","version":"0.0.1"}}}
{"jsonrpc":"2.0","id":2,"method":"tools/list"}
{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"causari_record","arguments":{"agent":"claude-3.5","model":"claude-3-5-sonnet","tool":"write_file","message":"create a.txt","prompt":"create a hello file","writes":["a.txt"]}}}
{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"causari_recall","arguments":{"query":"hello file","limit":3}}}
{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"causari_why","arguments":{"file":"a.txt","line":1}}}
EOF
)

echo "=== sending 5 JSON-RPC requests ==="
echo "$REQS" | "$RE" mcp 2>/dev/null

cd ..
rm -rf demo
echo; echo "Demo MCP complete."
