# Causari real-session test harness (Windows PowerShell)
#
# Measures the causal-join confidence on REAL, adversarial cases -- not the
# trivial happy path where file == completion. Two capture paths:
#   A) re proxy + re watch  -- heuristic content join (produces a confidence %)
#   B) re hook claude-code   -- agent-declared prompt+tool (exact, no heuristic)
#
# Dirty cases (the numbers that matter in production):
#   S1 clean baseline               -- agent output written verbatim
#   S2 human manual edit            -- human edits lines the agent just wrote
#   S3 formatter between model+disk -- output reflowed before saving
#   S4 near-simultaneous prompts    -- two completions competing for one file
#
# LLM transport is a local mock upstream; the CONTENT and the file/completion
# divergence are the variables under test. Payloads are string arrays joined
# with newlines to avoid PowerShell here-string parsing pitfalls.

$ErrorActionPreference = "Stop"
$here = $PSScriptRoot
$root = Split-Path -Parent (Split-Path -Parent $here)
$re = Join-Path $root "target\release\re.exe"
if (-not (Test-Path $re)) { Write-Error "build re first: cargo build --release"; exit 1 }

function Section($t) { Write-Host ("`n===== " + $t + " =====") -ForegroundColor Cyan }
function Doc($lines) { ($lines -join "`n") }

# ---------------------------------------------------------------------------
# PART A -- proxy + watch (heuristic join)
# ---------------------------------------------------------------------------
$demo = Join-Path $env:TEMP "causari-realsession-proxy"
if (Test-Path $demo) { Remove-Item -Recurse -Force $demo }
New-Item -ItemType Directory $demo | Out-Null
# Keep these OUTSIDE the watched tree, or the watcher records them as changes
# and their content coalesces into the join window, polluting the numbers.
$comp = Join-Path $env:TEMP "causari-rs-completion.txt"
$watchLog = Join-Path $env:TEMP "causari-rs-watch.log"

Push-Location $demo
& $re init | Out-Null

$env:COMPLETION_FILE = $comp
$env:MOCK_PORT = "4399"
Set-Content -Path $comp -Value "(warmup)" -NoNewline
$mock  = Start-Process python -ArgumentList (Join-Path $here "mock_upstream.py") -PassThru -WindowStyle Hidden
$proxy = Start-Process $re -ArgumentList "proxy","--openai-upstream","http://127.0.0.1:4399" -PassThru -WindowStyle Hidden -WorkingDirectory $demo
$watch = Start-Process $re -ArgumentList "watch","--agent","proxy-watch","--debounce","500" -PassThru -WindowStyle Hidden -WorkingDirectory $demo -RedirectStandardOutput $watchLog
Start-Sleep 3

# Send the request THROUGH the proxy (port 4242) so the exchange is captured
# into .causari/capture/ and becomes available to the watch causal join.
function AskViaProxy($prompt, $completion) {
    Set-Content -Path $comp -Value $completion -NoNewline
    $body = @{ model = "gpt-4o-realsession"; messages = @(@{ role = "user"; content = $prompt }) } | ConvertTo-Json -Depth 5
    Invoke-RestMethod -Uri "http://127.0.0.1:4242/openai/v1/chat/completions" -Method Post -Body $body -ContentType "application/json" -UserAgent "realsession/1.0" | Out-Null
    Start-Sleep 1
}

# --- S1: clean baseline ---------------------------------------------------
Section "S1 clean: agent writes the completion verbatim"
$auth = Doc @(
    'def refresh_token(user):',
    '    session = load_session(user)',
    '    rotated = rotate_every(session.token, hours=24)',
    '    audit_log.record("token.rotate", user=user.id)',
    '    return rotated'
)
AskViaProxy "Add JWT refresh logic that rotates every 24h with an audit log" $auth
Set-Content -Path (Join-Path $demo "auth.py") -Value $auth
Start-Sleep 3

# --- S2: human manual edit of agent code ----------------------------------
Section "S2 dirty: human hand-edits a line the agent just wrote"
$authEdited = Doc @(
    'def refresh_token(user):',
    '    session = load_session(user)',
    '    rotated = rotate_every(session.token, hours=48)  # hand-tuned by a human, not the model',
    '    audit_log.record("token.rotate", user=user.id)',
    '    return rotated'
)
Set-Content -Path (Join-Path $demo "auth.py") -Value $authEdited
Start-Sleep 3

# --- S3: formatter reflows the model output before it hits disk -----------
Section "S3 dirty: a formatter reflows the model output before saving"
$utilModel = Doc @(
    'def slugify(text):',
    '    return text.strip().lower().replace(" ", "-").replace("_", "-")'
)
$utilOnDisk = Doc @(
    'def slugify(text):',
    '    return (',
    '        text.strip()',
    '            .lower()',
    "            .replace(' ', '-')",
    "            .replace('_', '-')",
    '    )'
)
AskViaProxy "Add a slugify helper that lowercases and dashes separators" $utilModel
Set-Content -Path (Join-Path $demo "util.py") -Value $utilOnDisk
Start-Sleep 3

# --- S4: two near-simultaneous completions competing for one file ---------
Section "S4 dirty: two prompts in the window, one file mixes lines from both"
$compA = Doc @(
    'def connect_db(url):',
    '    pool = create_pool(url, max_connections=20)',
    '    return pool'
)
$compB = Doc @(
    'def cache_get(key):',
    '    value = redis_client.get(namespaced(key))',
    '    return decode(value)'
)
AskViaProxy "Write a connect_db helper with a connection pool of 20" $compA
AskViaProxy "Write a cache_get helper backed by redis" $compB
$mixed = Doc @(
    'def connect_db(url):',
    '    pool = create_pool(url, max_connections=20)',
    '    value = redis_client.get(namespaced(key))',
    '    return pool'
)
Set-Content -Path (Join-Path $demo "infra.py") -Value $mixed
Start-Sleep 3

Start-Sleep 2
Stop-Process -Id $watch.Id -Force -ErrorAction SilentlyContinue
Stop-Process -Id $proxy.Id -Force -ErrorAction SilentlyContinue
Stop-Process -Id $mock.Id  -Force -ErrorAction SilentlyContinue
Start-Sleep 1

Section "WATCH LOG (live confidence at record time)"
Get-Content $watchLog

Section "re why -- S1/S2 auth.py:3 (clean line became human-edited)"
& $re why "auth.py:3"
Section "re why -- S3 formatter-reflowed line util.py:4"
& $re why "util.py:4"
Section "re why -- S4 mixed file infra.py:3 (the redis line)"
& $re why "infra.py:3"

Pop-Location
Write-Host ("`nPROXY+WATCH repo: " + $demo) -ForegroundColor DarkGray

# ---------------------------------------------------------------------------
# PART B -- hook path (agent-declared, deterministic, no heuristic)
# ---------------------------------------------------------------------------
$hookRepo = Join-Path $env:TEMP "causari-realsession-hook"
if (Test-Path $hookRepo) { Remove-Item -Recurse -Force $hookRepo }
New-Item -ItemType Directory $hookRepo | Out-Null
Push-Location $hookRepo
& $re init | Out-Null

Section "HOOK H1: agent declares prompt (UserPromptSubmit) then edit (PostToolUse)"
$promptPayload = @{ session_id = "sess-1"; prompt = "Add a health-check endpoint returning build sha and uptime" } | ConvertTo-Json -Compress
$promptPayload | & $re hook-event user-prompt
$svc = Doc @(
    'def health_check():',
    '    return {"sha": BUILD_SHA, "uptime": uptime_seconds()}'
)
Set-Content -Path (Join-Path $hookRepo "service.py") -Value $svc
$toolPayload = @{ session_id = "sess-1"; tool_name = "Write"; tool_input = @{ file_path = (Join-Path $hookRepo "service.py") } } | ConvertTo-Json -Compress
$toolPayload | & $re hook-event post-tool

Section "re why -- hook path, agent line service.py:2 (expect exact prompt, no heuristic)"
& $re why "service.py:2"

Section "HOOK H2: a human edits a line directly -- NO hook fires"
$svcEdited = Doc @(
    'def health_check():',
    '    return {"sha": BUILD_SHA, "uptime": uptime_seconds(), "region": REGION}'
)
Set-Content -Path (Join-Path $hookRepo "service.py") -Value $svcEdited
Section "re why -- human-edited line service.py:2 (expect NO false attribution)"
& $re why "service.py:2"

Pop-Location
Write-Host ("`nHOOK repo: " + $hookRepo) -ForegroundColor DarkGray
