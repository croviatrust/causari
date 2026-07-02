# Causari capture engine demo (Windows PowerShell)
#
# Shows the full zero-cooperation provenance loop:
#   mock LLM upstream -> re proxy -> agent request -> file change ->
#   re watch content-based causal join -> re why with the real prompt.
#
# Requires: re (cargo build --release), python 3.

$ErrorActionPreference = "Stop"

$root = Split-Path -Parent $PSScriptRoot
$env:CARGO_TARGET_DIR = Join-Path $root "target"
$re = Join-Path $root "target\release\re.exe"
if (-not (Test-Path $re)) { $re = Join-Path $root "target\debug\re.exe" }
if (-not (Test-Path $re)) {
    Write-Host "building re (release)..." -ForegroundColor Yellow
    Push-Location $root
    cargo build --release
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    Pop-Location
    $re = Join-Path $root "target\release\re.exe"
}
if (-not (Test-Path $re)) { Write-Error "could not find re.exe after build"; exit 1 }

$demo = Join-Path $env:TEMP "causari-capture-demo"
if (Test-Path $demo) { Remove-Item -Recurse -Force $demo }
New-Item -ItemType Directory $demo | Out-Null
Push-Location $demo

Write-Host "`n=== 1. init repository ===" -ForegroundColor Cyan
& $re init

Write-Host "`n=== 2. start mock LLM upstream + capture proxy + watcher ===" -ForegroundColor Cyan
$mock = Start-Process python -ArgumentList (Join-Path $root "scripts\mock-llm.py") -PassThru -WindowStyle Hidden
$proxy = Start-Process $re -ArgumentList "proxy", "--openai-upstream", "http://127.0.0.1:4399" -PassThru -WindowStyle Hidden -WorkingDirectory $demo
$watch = Start-Process $re -ArgumentList "watch", "--agent", "demo-agent" -PassThru -WindowStyle Hidden -WorkingDirectory $demo
Start-Sleep 3

Write-Host "`n=== 3. agent asks the model through the proxy ===" -ForegroundColor Cyan
$body = @{
    model    = "gpt-4o-mock"
    messages = @(
        @{ role = "system"; content = "You are a coding agent." },
        @{ role = "user"; content = "Add JWT refresh logic that rotates every 24h" }
    )
} | ConvertTo-Json -Depth 5
Invoke-RestMethod -Uri "http://127.0.0.1:4242/openai/v1/chat/completions" `
    -Method Post -Body $body -ContentType "application/json" `
    -UserAgent "demo-agent/1.0" | Out-Null
Write-Host "prompt sent: 'Add JWT refresh logic that rotates every 24h'"

Write-Host "`n=== 4. agent writes the code from the completion to disk ===" -ForegroundColor Cyan
Start-Sleep 1
Set-Content -Path (Join-Path $demo "auth.py") -Value @"
def refresh_token(user):
    token = issue_token(user, scope="session")
    return rotate_every(token, hours=24)
"@
Start-Sleep 5

Write-Host "`n=== 5. the causal join: re why knows the real prompt ===" -ForegroundColor Cyan
& $re why auth.py:2

Write-Host "`n=== cleanup ===" -ForegroundColor Cyan
Stop-Process -Id $mock.Id, $proxy.Id, $watch.Id -Force -ErrorAction SilentlyContinue
Pop-Location
Write-Host "done. demo repo: $demo"
