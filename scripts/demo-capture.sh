#!/usr/bin/env bash
# Causari capture engine demo (Linux / macOS)
#
# Shows the full zero-cooperation provenance loop:
#   mock LLM upstream -> re proxy -> agent request -> file change ->
#   re watch content-based causal join -> re why with the real prompt.
#
# Requires: re (cargo build --release), python3, curl.

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
export CARGO_TARGET_DIR="$ROOT/target"
RE="$ROOT/target/release/re"
[ -x "$RE" ] || RE="$ROOT/target/debug/re"
if [ ! -x "$RE" ]; then
  echo "building re (release)..."
  (cd "$ROOT" && cargo build --release)
  RE="$ROOT/target/release/re"
fi
[ -x "$RE" ] || { echo "could not find re after build"; exit 1; }

DEMO="$(mktemp -d -t causari-capture-demo.XXXXXX)"
cd "$DEMO"

echo
echo "=== 1. init repository ==="
"$RE" init

echo
echo "=== 2. start mock LLM upstream + capture proxy + watcher ==="
python3 "$ROOT/scripts/mock-llm.py" >/dev/null 2>&1 &
MOCK=$!
"$RE" proxy --openai-upstream http://127.0.0.1:4399 >/dev/null 2>&1 &
PROXY=$!
"$RE" watch --agent demo-agent >/dev/null 2>&1 &
WATCH=$!
trap 'kill $MOCK $PROXY $WATCH 2>/dev/null || true' EXIT
sleep 3

echo
echo "=== 3. agent asks the model through the proxy ==="
curl -s http://127.0.0.1:4242/openai/v1/chat/completions \
  -H 'content-type: application/json' \
  -A 'demo-agent/1.0' \
  -d '{"model":"gpt-4o-mock","messages":[{"role":"system","content":"You are a coding agent."},{"role":"user","content":"Add JWT refresh logic that rotates every 24h"}]}' \
  >/dev/null
echo "prompt sent: 'Add JWT refresh logic that rotates every 24h'"

echo
echo "=== 4. agent writes the code from the completion to disk ==="
sleep 1
cat > auth.py <<'EOF'
def refresh_token(user):
    token = issue_token(user, scope="session")
    return rotate_every(token, hours=24)
EOF
sleep 5

echo
echo "=== 5. the causal join: re why knows the real prompt ==="
"$RE" why auth.py:2

echo
echo "done. demo repo: $DEMO"
