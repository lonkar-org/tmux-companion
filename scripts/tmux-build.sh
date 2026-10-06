#!/usr/bin/env bash
#
# Build an exact tmux from its release tarball, once, and print the directory
# its binary is in.
#
# usage:
#   scripts/tmux-build.sh 3.4             build if missing, print .../3.4/bin
#   scripts/tmux-build.sh --versions      the versions the suite is tested on
#   PATH="$(scripts/tmux-build.sh 3.5a):$PATH" cargo test --test e2e
#
# The cache is $TC_TMUX_CACHE, or $XDG_CACHE_HOME/tmux-companion/tmux, or
# ~/.cache/tmux-companion/tmux, one directory per version. CI keeps the same
# directory in actions/cache, so a runner builds each version once.
#
# Needs a C compiler, make, pkg-config, libevent and ncurses headers, and a
# yacc: configure refuses to run without one although the tarball ships
# cmd-parse.c already generated. On macOS that is Homebrew's libevent; on
# Ubuntu `build-essential pkg-config libevent-dev libncurses-dev bison`.
#
# Why this exists: CI installed whatever tmux apt and Homebrew shipped, 3.4 on
# ubuntu-24.04 and 3.7c on macOS, the laptop had 3.7c, and nothing ran two
# versions. On 2026-10-06 that hid two bugs only 3.4 to 3.6 have -- key
# routing losing the key typed during a hold, and the note parser misreading
# list-keys -N -- until a push, after which CI failed three attempts running
# and every local run was green. Testing named versions on purpose, here and
# on CI from one list, is the fix; the list below is that list.
set -euo pipefail

# version sha256 of tmux-<version>.tar.gz, as GitHub lists the release asset.
# 3.4 is ubuntu-24.04's packaged tmux and the floor; 3.5a the first the key
# router supports; 3.7c current, and the playground's.
PINNED=(
  "3.4  551ab8dea0bf505c0ad6b7bb35ef567cdde0ccb84357df142c254f35a23e19aa"
  "3.5a 16216bd0877170dfcc64157085ba9013610b12b082548c7c9542cc0103198951"
  "3.7c 7c60cae9a0e25288e2e24750aafc9e8800fc7fd4555e447e1b29ee4201cfb3bf"
)

if [[ "${1:-}" == "--versions" ]]; then
  for row in "${PINNED[@]}"; do echo "${row%% *}"; done
  exit 0
fi

VERSION="${1:?usage: scripts/tmux-build.sh VERSION | --versions}"
SHA=""
for row in "${PINNED[@]}"; do
  read -r v s <<<"$row"
  [[ "$v" == "$VERSION" ]] && SHA="$s"
done
if [[ -z "$SHA" ]]; then
  echo "tmux-build: $VERSION is not pinned; pinned are: $("$0" --versions | tr '\n' ' ')" >&2
  exit 2
fi

CACHE="${TC_TMUX_CACHE:-${XDG_CACHE_HOME:-$HOME/.cache}/tmux-companion/tmux}"
PREFIX="$CACHE/$VERSION"
if [[ -x "$PREFIX/bin/tmux" ]] && "$PREFIX/bin/tmux" -V | grep -qx "tmux $VERSION"; then
  echo "$PREFIX/bin"
  exit 0
fi

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
TARBALL="$WORK/tmux-$VERSION.tar.gz"
echo "tmux-build: fetching tmux $VERSION" >&2
curl -fsSL --retry 3 -o "$TARBALL" \
  "https://github.com/tmux/tmux/releases/download/$VERSION/tmux-$VERSION.tar.gz"
if command -v sha256sum >/dev/null; then
  GOT=$(sha256sum "$TARBALL" | awk '{print $1}')
else
  GOT=$(shasum -a 256 "$TARBALL" | awk '{print $1}')
fi
if [[ "$GOT" != "$SHA" ]]; then
  echo "tmux-build: tmux-$VERSION.tar.gz is $GOT, pinned $SHA; not building it" >&2
  exit 1
fi

tar -xzf "$TARBALL" -C "$WORK"
cd "$WORK/tmux-$VERSION"
# Homebrew's libevent is not on the default pkg-config path on macOS.
if command -v brew >/dev/null && brew --prefix libevent >/dev/null 2>&1; then
  export PKG_CONFIG_PATH="$(brew --prefix libevent)/lib/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
fi
echo "tmux-build: building tmux $VERSION into $PREFIX" >&2
./configure --prefix="$PREFIX" --disable-utf8proc >"$WORK/configure.log" 2>&1 \
  || { tail -30 "$WORK/configure.log" >&2; exit 1; }
JOBS=$(getconf _NPROCESSORS_ONLN 2>/dev/null || echo 2)
make -j"$JOBS" >"$WORK/make.log" 2>&1 || { tail -30 "$WORK/make.log" >&2; exit 1; }
rm -rf "$PREFIX"
make install >/dev/null
"$PREFIX/bin/tmux" -V >&2
echo "$PREFIX/bin"
