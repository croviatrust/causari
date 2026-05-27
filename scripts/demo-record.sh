#!/usr/bin/env bash
# Records a Causari CLI demo as a GIF using vhs.
# Run inside WSL/Linux/macOS:
#   ./scripts/demo-record.sh
#
# Outputs:
#   site/assets/demo.gif    (final GIF, embedded in the landing page)
#   demo-recording/         (scratch dir, gitignored via /demo)

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

say()  { printf '\033[1;36m==>\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33m==>\033[0m %s\n' "$*" >&2; }

# -------- 1. ensure tools --------
need() {
  command -v "$1" >/dev/null 2>&1 || { warn "missing: $1"; return 1; }
}
missing=0
for t in vhs ttyd ffmpeg curl tar; do
  need "$t" || missing=1
done
if [ "$missing" = 1 ]; then
  cat <<EOF >&2

Install hints (Debian/Ubuntu):

  sudo apt-get update
  sudo apt-get install -y ttyd ffmpeg curl tar
  # vhs (latest binary):
  curl -L "https://github.com/charmbracelet/vhs/releases/latest/download/vhs_Linux_x86_64.tar.gz" \\
    | sudo tar -xz -C /usr/local/bin vhs

EOF
  exit 1
fi

# -------- 2. ensure 're' binary is on PATH --------
if ! command -v re >/dev/null 2>&1; then
  if [ -x "target/release/re" ]; then
    export PATH="$ROOT/target/release:$PATH"
    say "using local target/release/re"
  else
    say "building re --release (cold build ~45s)"
    cargo build --release
    export PATH="$ROOT/target/release:$PATH"
  fi
fi
say "re version: $(re --version 2>&1 | head -n1)"

# -------- 3. prep clean scratch dir --------
SCRATCH="$ROOT/demo-recording"
rm -rf "$SCRATCH"
mkdir -p "$SCRATCH/sample-project"
cd "$SCRATCH/sample-project"

cat > package.json <<'EOF'
{"name":"sample","version":"0.1.0"}
EOF

mkdir -p src spec
cat > spec/auth.md <<'EOF'
# Auth spec

The user-facing API requires JSON Web Tokens.
Refresh tokens MUST rotate every 24 hours to reduce
replay-attack windows.
EOF

cat > src/auth.ts <<'EOF'
// Placeholder. Will be filled by the agent.
export function authenticate(token: string) {
  return { ok: !!token };
}
EOF

# -------- 4. write the .tape script --------
cd "$SCRATCH"
cat > demo.tape <<'TAPE'
Output ../site/assets/demo.gif

Set Shell "bash"
Set FontSize 16
Set Width 1100
Set Height 640
Set Theme "Dracula"
Set TypingSpeed 35ms
Set PlaybackSpeed 1.0

# pretty prompt
Hide
Type@1ms `export PS1='\[\033[1;36m\]$\[\033[0m\] '`
Enter
Type@1ms "clear"
Enter
Show

Sleep 600ms
Type "cd sample-project"
Enter
Sleep 400ms

Type "re init"
Enter
Sleep 1500ms

Type "cat spec/auth.md"
Enter
Sleep 2000ms

Type "# the agent now writes auth.ts based on the spec"
Enter
Sleep 800ms

Type "re record -m 'Add JWT refresh logic that rotates every 24h' --agent claude-3.5-sonnet --reads spec/auth.md,package.json --writes src/auth.ts"
Enter
Sleep 2200ms

Type "re log --limit 3"
Enter
Sleep 2800ms

Type "re why src/auth.ts:1"
Enter
Sleep 3000ms

Type "re trace src/auth.ts"
Enter
Sleep 3500ms

Sleep 1500ms
TAPE

# -------- 5. record --------
say "recording (this takes ~30s)..."
mkdir -p "$ROOT/site/assets"
vhs demo.tape

GIF="$ROOT/site/assets/demo.gif"
if [ -f "$GIF" ]; then
  say "produced: $GIF ($(du -h "$GIF" | cut -f1))"
else
  warn "vhs did not produce demo.gif — check the .tape script"
  exit 1
fi
