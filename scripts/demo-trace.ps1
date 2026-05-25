#!/usr/bin/env pwsh
# Demo for `re trace` — building a real causal chain across 4 events.
$ErrorActionPreference = "Stop"
$RE = "$PSScriptRoot\..\target\debug\re.exe"

Remove-Item -Recurse -Force demo -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force demo | Out-Null
Set-Location demo

& $RE init | Out-Null

# Event 1: user (or an agent) creates a spec file.
@'
# API: sum function should subtract for some reason
spec_version = 1
'@ | Out-File -Encoding utf8 spec.md
'{"agent":"user","tool":"write_file","message":"create initial spec","prompt":"write the spec"}' | & $RE record --stdin | Out-Null

# Event 2: an agent reads the spec and writes calc.js based on it.
@'
// Implementation guided by spec.md
export function sum(a, b) { return a + b; }
'@ | Out-File -Encoding utf8 calc.js
'{"agent":"claude-3.5","model":"claude-3-5-sonnet","tool":"write_file","message":"implement calc per spec","prompt":"implement sum() following the spec in spec.md","reads":["spec.md"]}' | & $RE record --stdin | Out-Null

# Event 3: another agent edits the spec to say "subtract".
@'
# API: sum function should subtract for some reason
# CHANGED: rebranded "sum" to mean signed-difference
spec_version = 2
'@ | Out-File -Encoding utf8 spec.md
'{"agent":"gpt-4o","model":"openai/gpt-4o","tool":"edit_file","message":"update spec to redefine sum","prompt":"the team decided sum should compute a-b, update the spec","reasoning":"PM wants sum to mean signed difference now."}' | & $RE record --stdin | Out-Null

# Event 4: an agent re-reads the updated spec and "fixes" calc.js — INTRODUCING THE BUG.
@'
// Implementation guided by spec.md
export function sum(a, b) { return a - b; }
'@ | Out-File -Encoding utf8 calc.js
'{"agent":"gpt-4o","tool":"edit_file","message":"align calc.js with updated spec","prompt":"the spec was updated, make calc.js match","reads":["spec.md","calc.js"]}' | & $RE record --stdin | Out-Null

Write-Host "`n=== log ===" -ForegroundColor Cyan
& $RE log --oneline

Write-Host "`n=== re why calc.js:2 ===" -ForegroundColor Cyan
& $RE why "calc.js:2"

Write-Host "`n=== re trace calc.js:2  (causal cone) ===" -ForegroundColor Magenta
& $RE trace "calc.js:2"

Write-Host "`n=== re find 'spec' ===" -ForegroundColor Cyan
& $RE find "spec"

Set-Location ..
Write-Host "`nDemo complete." -ForegroundColor Green
