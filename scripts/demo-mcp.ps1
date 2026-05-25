#!/usr/bin/env pwsh
# Demo: MCP server end-to-end via JSON-RPC pipes.
$ErrorActionPreference = "Stop"
$RE = "$PSScriptRoot\..\target\debug\re.exe"

Remove-Item -Recurse -Force demo -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force demo | Out-Null
Set-Location demo

& $RE init | Out-Null
"hello" | Out-File -Encoding utf8 a.txt

# Build a batch of JSON-RPC messages an agent would send.
$messages = @(
  '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"demo-agent","version":"0.0.1"}}}',
  '{"jsonrpc":"2.0","id":2,"method":"tools/list"}',
  '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"causari_record","arguments":{"agent":"claude-3.5","model":"claude-3-5-sonnet","tool":"write_file","message":"create a.txt","prompt":"create a hello file","writes":["a.txt"]}}}',
  '{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"causari_recall","arguments":{"query":"hello file","limit":3}}}',
  '{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"causari_why","arguments":{"file":"a.txt","line":1}}}'
)

Write-Host "=== sending 5 JSON-RPC requests ===" -ForegroundColor Cyan
$messages -join "`n" | & $RE mcp 2>$null

Set-Location ..
Remove-Item -Recurse -Force demo
Write-Host "`nDemo MCP complete." -ForegroundColor Green
