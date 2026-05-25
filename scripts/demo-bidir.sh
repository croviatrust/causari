#!/usr/bin/env bash
# Bidirectional causality demo (`re impact`, `re lens`, causal revert) for Linux / macOS.
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
RE="$HERE/../target/debug/re"
if [[ ! -x "$RE" ]]; then RE="$HERE/../target/release/re"; fi

cd "$HERE/.."
rm -rf demo
mkdir demo
cd demo

"$RE" init >/dev/null

echo 'spec: sum should add two numbers' > spec.md
printf '%s\n' '{"agent":"user","tool":"write_file","message":"initial spec"}' | "$RE" record --stdin >/dev/null

echo 'export function sum(a, b) { return a + b }' > calc.js
printf '%s\n' '{"agent":"claude-3.5","tool":"write_file","message":"implement sum","prompt":"implement per spec","reads":["spec.md"]}' | "$RE" record --stdin >/dev/null

echo 'spec: sum should SUBTRACT two numbers (renamed)' > spec.md
printf '%s\n' '{"agent":"gpt-4o","tool":"edit_file","message":"redefine sum in spec","prompt":"PM says sum should mean signed-difference"}' | "$RE" record --stdin >/dev/null

E3="$("$RE" log --oneline | awk 'NR==1{print $1}')"

echo 'export function sum(a, b) { return a - b }' > calc.js
printf '%s\n' '{"agent":"gpt-4o","tool":"edit_file","message":"align calc with new spec","prompt":"update calc per spec","reads":["spec.md","calc.js"]}' | "$RE" record --stdin >/dev/null

echo "import {sum} from './calc.js'; console.assert(sum(5,3) === 2)" > test.js
printf '%s\n' '{"agent":"claude-3.5","tool":"write_file","message":"add tests","prompt":"add a test for sum","reads":["calc.js"]}' | "$RE" record --stdin >/dev/null

printf '%s\n%s\n' '# Project' 'See spec.md for the sum API.' > README.md
printf '%s\n' '{"agent":"claude-3.5","tool":"write_file","message":"add README","prompt":"write README","reads":["spec.md"]}' | "$RE" record --stdin >/dev/null

echo; echo "=== log ==="
"$RE" log --oneline

echo; echo "=== re impact ${E3} (downstream cone) ==="
"$RE" impact "$E3"

echo; echo "=== re lens calc.js (per-line provenance) ==="
"$RE" lens calc.js

echo; echo "=== re revert ${E3} (causality-aware preview) ==="
printf 'n\n' | "$RE" revert "$E3" || true

cd ..
rm -rf demo
echo; echo "Demo complete."
