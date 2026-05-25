#!/usr/bin/env bash
# Full Causari demo for Linux / macOS. Mirrors scripts/demo.ps1.
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
RE="$HERE/../target/debug/re"
if [[ ! -x "$RE" ]]; then
  RE="$HERE/../target/release/re"
fi

cd "$HERE/.."
rm -rf demo
mkdir demo
cd demo

echo
echo "=== 1. init ==="
"$RE" init

echo
echo "=== 2. agent writes initial file ==="
cat > calc.js <<'EOF'
export function sum(a, b) { return a + b; }
EOF
cat > test.js <<'EOF'
import {sum} from './calc.js';
if (sum(2, 3) !== 5) { process.exit(1); }
EOF
printf '%s\n' \
  '{"agent":"gpt-4o","model":"openai/gpt-4o","tool":"write_file","message":"create calc.js and test.js","prompt":"write a sum() function and a basic test for it"}' \
  | "$RE" record --stdin

echo
echo "=== 3. agent introduces a subtle bug ==="
cat > calc.js <<'EOF'
export function sum(a, b) { return a - b; }
EOF
printf '%s\n' \
  '{"agent":"gpt-4o","model":"openai/gpt-4o","tool":"edit_file","message":"refactor sum","prompt":"refactor sum to be more elegant","reasoning":"I noticed that subtraction is symmetric in a sense and decided to simplify."}' \
  | "$RE" record --stdin

echo
echo "=== 4. agent makes an unrelated edit ==="
cat > VERSION <<'EOF'
0.1.0
EOF
printf '%s\n' \
  '{"agent":"gpt-4o","tool":"edit_file","message":"add VERSION constant","prompt":"add a VERSION file"}' \
  | "$RE" record --stdin

echo
echo "=== 5. log ==="
"$RE" log --oneline

echo
echo "=== 6. re why calc.js:1  -- who wrote the buggy line? ==="
"$RE" why calc.js:1

echo
echo "=== 7. re bisect to find the bad event automatically ==="
GOOD=$("$RE" log --oneline | awk 'NR==3{print $1}')
BAD=$("$RE"  log --oneline | awk 'NR==1{print $1}')
"$RE" bisect --good "$GOOD" --bad "$BAD" --test "node test.js" || true

cd ..
rm -rf demo
echo
echo "Demo complete."
