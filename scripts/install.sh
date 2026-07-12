#!/usr/bin/env sh
# =============================================================
# Causari one-line installer (Linux & macOS)
#
# Usage:
#   curl -sSf https://causari.dev/install.sh | sh
#
# Optional environment variables:
#   CAUSARI_VERSION      pin a specific version (default: latest)
#   CAUSARI_BIN_DIR      install location (default: $HOME/.local/bin)
#   CAUSARI_SKIP_VERIFY  set to 1 to bypass the sha256 check (not recommended)
#
# The binary's sha256 is verified against the signed SHA256SUMS.txt published
# with each GitHub release. Prefer building from source if you want to review
# the code first:  cargo install --git https://github.com/croviatrust/causari
# =============================================================
set -eu

REPO="croviatrust/causari"
VERSION="${CAUSARI_VERSION:-}"
BIN_DIR="${CAUSARI_BIN_DIR:-$HOME/.local/bin}"

say()  { printf '\033[1;36mcausari:\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33mcausari:\033[0m %s\n' "$*" >&2; }
die()  { printf '\033[1;31mcausari:\033[0m %s\n' "$*" >&2; exit 1; }

# ---- detect platform ----
uname_s="$(uname -s)"
uname_m="$(uname -m)"
case "$uname_s" in
  Linux)  os="unknown-linux-gnu" ;;
  Darwin) os="apple-darwin" ;;
  *) die "unsupported OS: $uname_s — try 'cargo install --git https://github.com/$REPO'" ;;
esac
case "$uname_m" in
  x86_64|amd64)   arch="x86_64" ;;
  arm64|aarch64)  arch="aarch64" ;;
  *) die "unsupported arch: $uname_m" ;;
esac
target="${arch}-${os}"

# ---- resolve version ----
if [ -z "$VERSION" ]; then
  VERSION="$(curl -sSfL "https://api.github.com/repos/$REPO/releases/latest" \
    | sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p' | head -n1)"
  [ -n "$VERSION" ] || die "could not fetch latest release tag"
fi
say "installing $REPO $VERSION ($target)"

# ---- download ----
url="https://github.com/$REPO/releases/download/$VERSION/re-${VERSION}-${target}.tar.gz"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
curl -sSfL "$url" -o "$tmp/re.tar.gz" || die "download failed: $url"

# ---- verify sha256 (required by default) ----
if [ "${CAUSARI_SKIP_VERIFY:-0}" = "1" ]; then
  warn "CAUSARI_SKIP_VERIFY=1 set — installing WITHOUT checksum verification"
else
  sums_url="https://github.com/$REPO/releases/download/$VERSION/SHA256SUMS.txt"
  curl -sSfL "$sums_url" -o "$tmp/SHA256SUMS.txt" \
    || die "could not download SHA256SUMS.txt for $VERSION — refusing to install unverified (set CAUSARI_SKIP_VERIFY=1 to override, or build from source: cargo install --git https://github.com/$REPO)"
  expected="$(grep "re-${VERSION}-${target}.tar.gz" "$tmp/SHA256SUMS.txt" | awk '{print $1}')"
  [ -n "$expected" ] || die "no checksum for re-${VERSION}-${target}.tar.gz in SHA256SUMS.txt — refusing to install unverified"
  if command -v sha256sum >/dev/null 2>&1; then
    actual="$(sha256sum "$tmp/re.tar.gz" | awk '{print $1}')"
  elif command -v shasum >/dev/null 2>&1; then
    actual="$(shasum -a 256 "$tmp/re.tar.gz" | awk '{print $1}')"
  else
    die "neither sha256sum nor shasum is available to verify the download — install one, or set CAUSARI_SKIP_VERIFY=1 to override"
  fi
  [ "$actual" = "$expected" ] || die "sha256 mismatch — refusing to install (expected $expected, got $actual)"
  say "sha256 verified ($actual)"
fi

# ---- extract & install ----
tar -xzf "$tmp/re.tar.gz" -C "$tmp"
mkdir -p "$BIN_DIR"
mv "$tmp/re" "$BIN_DIR/re"
chmod +x "$BIN_DIR/re"
say "installed $BIN_DIR/re"

# ---- PATH check ----
case ":$PATH:" in
  *":$BIN_DIR:"*) : ;;
  *)
    warn "$BIN_DIR is not in your PATH"
    warn "add this to your shell profile:"
    warn "    export PATH=\"$BIN_DIR:\$PATH\""
    ;;
esac

# ---- post ----
"$BIN_DIR/re" --version 2>/dev/null || true
say "done. Run: re init"
