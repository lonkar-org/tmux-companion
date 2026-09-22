#!/usr/bin/env bash
# Install tmux-companion: download the release binary for this machine, verify
# it against the published checksums, and put it on PATH. Falls back to
# building from source when no binary matches.
#
# usage: ./scripts/install.sh [--version vX.Y.Z] [--prefix DIR] [--build] [--force]
#          --version   a release tag; default is the latest release
#          --prefix    install root, binary goes in <prefix>/bin
#                      default: /usr/local if writable, else ~/.local
#          --build     skip the download and build from source
#          --force     reinstall even when the wanted version is already there
#
# Reads REPO to point at a fork. Honours the usual CI-friendly variables:
# nothing here is interactive and nothing calls sudo.
#
# Every download is checked against checksums.txt from the same release before
# anything is moved onto PATH. A mismatch stops the script; it does not warn
# and carry on, because the whole point of the file is the case where the
# download was not what the release published.
set -euo pipefail

REPO=${REPO:-lonkar-org/tmux-companion}
BIN=tmux-companion
VERSION=""
PREFIX=""
BUILD_ONLY=0
FORCE=0

while [ $# -gt 0 ]; do
  case $1 in
    --version) VERSION=${2:?--version needs a tag}; shift 2 ;;
    --prefix)  PREFIX=${2:?--prefix needs a directory}; shift 2 ;;
    --build)   BUILD_ONLY=1; shift ;;
    --force)   FORCE=1; shift ;;
    -h|--help) sed -n '2,15p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done

say() { printf 'tmux-companion: %s\n' "$*" >&2; }
die() { printf 'tmux-companion: %s\n' "$*" >&2; exit 1; }

# ── where it goes ────────────────────────────────────────────────────────────
if [ -z "$PREFIX" ]; then
  if [ -w /usr/local/bin ] 2>/dev/null; then PREFIX=/usr/local; else PREFIX="$HOME/.local"; fi
fi
DEST="$PREFIX/bin"

# ── which binary this machine wants ──────────────────────────────────────────
target_triple() {
  local os arch
  os=$(uname -s)
  arch=$(uname -m)
  case "$arch" in
    arm64|aarch64) arch=aarch64 ;;
    x86_64|amd64)  arch=x86_64 ;;
    *) return 1 ;;
  esac
  case "$os" in
    Darwin) printf '%s-apple-darwin' "$arch" ;;
    # musl, so one Linux binary runs on any distribution rather than tracking
    # whichever glibc the build machine happened to have.
    Linux)  printf '%s-unknown-linux-musl' "$arch" ;;
    *) return 1 ;;
  esac
}

sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | cut -d' ' -f1
  elif command -v shasum >/dev/null 2>&1; then shasum -a 256 "$1" | cut -d' ' -f1
  else return 1
  fi
}

build_from_source() {
  command -v cargo >/dev/null 2>&1 ||
    die "no released binary for this machine and no cargo to build one.
  Install Rust 1.85 or newer from https://rustup.rs and run this again."
  local here
  here=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
  [ -f "$here/Cargo.toml" ] ||
    die "run this from a checkout, or install a released binary instead"
  say "building from source in $here"
  ( cd "$here" && cargo build --release )
  mkdir -p "$DEST"
  install -m 755 "$here/target/release/$BIN" "$DEST/$BIN"
  say "installed $DEST/$BIN"
}

if [ "$BUILD_ONLY" = 1 ]; then
  build_from_source
  exit 0
fi

TRIPLE=$(target_triple) || {
  say "no released binary for $(uname -s)/$(uname -m)"
  build_from_source
  exit 0
}

command -v curl >/dev/null 2>&1 || die "curl is needed to download a release"

API="https://api.github.com/repos/$REPO/releases"
if [ -z "$VERSION" ]; then
  VERSION=$(curl -fsSL "$API/latest" 2>/dev/null |
    sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p' | head -1) || true
  [ -n "$VERSION" ] || {
    say "no published release to download"
    build_from_source
    exit 0
  }
fi

if [ "$FORCE" = 0 ] && command -v "$BIN" >/dev/null 2>&1; then
  have=$("$BIN" --version 2>/dev/null | awk '{print $2}' | cut -d+ -f1 || true)
  if [ -n "$have" ] && [ "v$have" = "$VERSION" ]; then
    say "$VERSION is already installed at $(command -v "$BIN")"
    exit 0
  fi
fi

ASSET="$BIN-$VERSION-$TRIPLE.tar.gz"
BASE="https://github.com/$REPO/releases/download/$VERSION"

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

say "downloading $ASSET"
curl -fsSL -o "$TMP/$ASSET" "$BASE/$ASSET" || {
  say "no asset $ASSET in $VERSION"
  build_from_source
  exit 0
}
curl -fsSL -o "$TMP/checksums.txt" "$BASE/checksums.txt" ||
  die "the release has no checksums.txt, refusing to install an unverified binary"

# Compared as strings, field by field, rather than matched as a pattern. The
# asset name carries dots, and in a regex a dot is any character, so a grep
# would also accept a name that merely looked like this one.
want=""
while read -r sum file; do
  [ "${file#\*}" = "$ASSET" ] || continue
  want=$sum
  break
done < "$TMP/checksums.txt"
[ -n "$want" ] || die "$ASSET is not listed in checksums.txt, refusing to install it"
got=$(sha256_of "$TMP/$ASSET") || die "no sha256sum or shasum to verify the download"
[ "$want" = "$got" ] || die "checksum mismatch for $ASSET
  published $want
  download  $got
  Nothing was installed."

tar -xzf "$TMP/$ASSET" -C "$TMP"
[ -f "$TMP/$BIN" ] || die "$ASSET did not contain $BIN"
mkdir -p "$DEST"
install -m 755 "$TMP/$BIN" "$DEST/$BIN"
say "installed $VERSION to $DEST/$BIN"

case ":$PATH:" in
  *":$DEST:"*) ;;
  *) say "$DEST is not on your PATH. Add it, or pass --prefix /usr/local." ;;
esac
