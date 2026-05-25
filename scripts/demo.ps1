#!/usr/bin/env pwsh
# Full end-to-end demo for Causari.
# Run from the repo root with: pwsh scripts/demo.ps1

$ErrorActionPreference = "Stop"
$RE = "$PSScriptRoot\..\target\debug\re.exe"

# Reset
Remove-Item -Recurse -Force demo -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force demo | Out-Null
Set-Location demo

Write-Host "`n=== 1. init ===" -ForegroundColor Cyan
& $RE init

Write-Host "`n=== 2. agent writes initial file ===" -ForegroundColor Cyan
@'
export function sum(a, b) {
  return a + b;
}
'@ | Out-File -Encoding utf8 calc.js
@'
import { sum } from "./calc.js";
if (sum(2, 3) !== 5) { console.error("FAIL"); process.exit(1); }
console.log("OK");
'@ | Out-File -Encoding utf8 test.js

# Record this as an agent event with rich metadata via stdin.
'{"agent":"claude-3.5","model":"anthropic/claude-3-5-sonnet","tool":"write_file","message":"create calc.js and test.js","prompt":"write a sum function with a test"}' | & $RE record --stdin

Write-Host "`n=== 3. agent introduces a subtle bug ===" -ForegroundColor Cyan
@'
export function sum(a, b) {
  return a - b;
}
'@ | Out-File -Encoding utf8 calc.js
'{"agent":"gpt-4o","model":"openai/gpt-4o","tool":"edit_file","message":"refactor sum","prompt":"refactor sum to be more elegant","reasoning":"I noticed that subtraction is symmetric in a sense and decided to simplify."}' | & $RE record --stdin

Write-Host "`n=== 4. agent makes an unrelated edit ===" -ForegroundColor Cyan
@'
export function sum(a, b) {
  return a - b;
}
export const VERSION = "1.0.0";
'@ | Out-File -Encoding utf8 calc.js
'{"agent":"claude-3.5","tool":"edit_file","message":"add VERSION constant","prompt":"add a version constant"}' | & $RE record --stdin

Write-Host "`n=== 5. log ===" -ForegroundColor Cyan
& $RE log --oneline

Write-Host "`n=== 6. re why calc.js:2  -- who wrote the buggy line? ===" -ForegroundColor Cyan
& $RE why "calc.js:2"

Write-Host "`n=== 7. re bisect to find the bad event automatically ===" -ForegroundColor Cyan
$ids = (& $RE log --oneline) | ForEach-Object { ($_ -split '\s+')[0] }
$bad  = $ids[0]   # most recent
$good = $ids[-1]  # oldest
& $RE bisect --good $good --bad $bad --test "node test.js"

Write-Host "`n=== 8. cleanup ===" -ForegroundColor Cyan
Set-Location ..
Write-Host "Demo complete." -ForegroundColor Green
