#!/usr/bin/env pwsh
# Demo: bidirectional causality (`re impact`, `re lens`, revert preview).
$ErrorActionPreference = "Stop"
$RE = "$PSScriptRoot\..\target\debug\re.exe"

Remove-Item -Recurse -Force demo -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force demo | Out-Null
Set-Location demo

& $RE init | Out-Null

# E1: write the spec.
'spec: sum should add two numbers' | Out-File -Encoding utf8 spec.md
'{"agent":"user","tool":"write_file","message":"initial spec"}' | & $RE record --stdin | Out-Null

# E2: write calc.js based on spec (reads spec.md).
"export function sum(a, b) { return a + b }" | Out-File -Encoding utf8 calc.js
'{"agent":"claude-3.5","tool":"write_file","message":"implement sum","prompt":"implement per spec","reads":["spec.md"]}' | & $RE record --stdin | Out-Null

# E3 (THE SOURCE EVENT): modify spec.md saying sum should subtract.
'spec: sum should SUBTRACT two numbers (renamed)' | Out-File -Encoding utf8 spec.md
'{"agent":"gpt-4o","tool":"edit_file","message":"redefine sum in spec","prompt":"PM says sum should mean signed-difference"}' | & $RE record --stdin | Out-Null

$E3 = (& $RE log --oneline | Select-Object -First 1).Split()[0]

# E4: align calc.js with new spec (reads spec.md = E3 output).
"export function sum(a, b) { return a - b }" | Out-File -Encoding utf8 calc.js
'{"agent":"gpt-4o","tool":"edit_file","message":"align calc with new spec","prompt":"update calc per spec","reads":["spec.md","calc.js"]}' | & $RE record --stdin | Out-Null

# E5: write tests using calc.js (reads calc.js = E4 output).
"import {sum} from './calc.js'; console.assert(sum(5,3) === 2)" | Out-File -Encoding utf8 test.js
'{"agent":"claude-3.5","tool":"write_file","message":"add tests","prompt":"add a test for sum","reads":["calc.js"]}' | & $RE record --stdin | Out-Null

# E6: write README mentioning the API (reads spec.md).
"# Project`nSee spec.md for the sum API." | Out-File -Encoding utf8 README.md
'{"agent":"claude-3.5","tool":"write_file","message":"add README","prompt":"write README","reads":["spec.md"]}' | & $RE record --stdin | Out-Null

Write-Host "`n=== log ===" -ForegroundColor Cyan
& $RE log --oneline

Write-Host "`n=== re impact ${E3} (downstream cone) ===" -ForegroundColor Magenta
& $RE impact $E3

Write-Host "`n=== re lens calc.js (per-line provenance) ===" -ForegroundColor Cyan
& $RE lens calc.js

Write-Host "`n=== re revert ${E3} (causality-aware preview) ===" -ForegroundColor Yellow
"n" | & $RE revert $E3  # answer 'n' to confirmation, just to see the preview

Set-Location ..
Write-Host "`nDemo complete." -ForegroundColor Green
