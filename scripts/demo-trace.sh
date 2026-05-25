#!/usr/bin/env bash
# Causal-cone demo (`re trace`) for Linux / macOS.
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
RE="$HERE/../target/debug/re"
if [[ ! -x "$RE" ]]; then RE="$HERE/../target/release/re"; fi

cd "$HERE/.."
rm -rf demo
mkdir demo
cd demo

"$RE" init >/dev/null

echo 'spec_version = 1'                                              > spec.md
printf '%s\n' '{"agent":"user","tool":"write_file","message":"create initial spec"}' \
  | "$RE" record --stdin >/dev/null

echo '// Implementation guided by spec.md'                           > calc.js
echo 'export function sum(a, b) { return a + b; }'                  >> calc.js
printf '%s\n' '{"agent":"claude-3.5","model":"claude-3-5-sonnet","tool":"write_file","message":"implement calc per spec","prompt":"implement sum() following the spec in spec.md","reads":["spec.md"]}' \
  | "$RE" record --stdin >/dev/null

cat > spec.md <<'EOF'
# API: sum function should subtract for some reason
# CHANGED: rebranded "sum" to mean signed-difference
spec_version = 2
EOF
printf '%s\n' '{"agent":"gpt-4o","model":"openai/gpt-4o","tool":"edit_file","message":"update spec to redefine sum","prompt":"the team decided sum should compute a-b, update the spec","reasoning":"PM wants sum to mean signed difference now."}' \
  | "$RE" record --stdin >/dev/null

cat > calc.js <<'EOF'
// Implementation guided by spec.md
export function sum(a, b) { return a - b; }
EOF
printf '%s\n' '{"agent":"gpt-4o","tool":"edit_file","message":"align calc.js with updated spec","prompt":"the spec was updated, make calc.js match","reads":["spec.md","calc.js"]}' \
  | "$RE" record --stdin >/dev/null

echo; echo "=== log ==="
"$RE" log --oneline

echo; echo "=== re why calc.js:2 ==="
"$RE" why calc.js:2

echo; echo "=== re trace calc.js:2  (causal cone) ==="
"$RE" trace calc.js:2

echo; echo "=== re find 'spec' ==="
"$RE" find spec

cd ..
rm -rf demo
echo; echo "Demo complete."
