#!/usr/bin/env bash
#
# A static Linux binary of this checkout, cross-built on a Mac, with no
# Docker and no C toolchain for Linux.
#
# usage:
#   scripts/build-linux.sh                              x86_64-unknown-linux-musl
#   scripts/build-linux.sh aarch64-unknown-linux-musl
#   scripts/build-linux.sh x86_64-unknown-linux-musl --lint
#
# Prints the path of the binary on its last line; cargo's output goes to stderr.
# `--lint` runs clippy for that target instead, the way `just lint` runs it
# here, and prints nothing: code under cfg(target_os = "linux") is otherwise
# linted only by CI.
#
# What it works round, once:
# - `cargo` on PATH may be a Homebrew rust whose sysroot has no Linux std
#   ("can't find crate for `core`"), so the pinned rustup toolchain's own
#   cargo and rustc are used by path.
# - rustup's rust-lld on macOS loads @rpath/libLLVM.dylib from bin/../lib,
#   which the toolchain doesn't have there, and DYLD_LIBRARY_PATH set in a
#   wrapper script didn't reach it. A copy of rust-lld under target/lld/bin
#   with lib/libLLVM.dylib linked beside it finds the library on its own.
# - The musl targets ship their own crt objects, so ld.lld links a static-pie
#   with nothing from Linux installed here.
# - rust-objcopy has the same libLLVM problem, and cargo runs it to strip
#   debug info, so the release profile's strip is turned off for this build:
#   the binary is bigger and nothing else changes.
set -euo pipefail

TARGET=${1:-x86_64-unknown-linux-musl}
MODE=${2:-build}
REPO=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$REPO"
CHANNEL=$(sed -n 's/^channel = "\(.*\)"/\1/p' rust-toolchain.toml)
TC=$(rustup toolchain list -v | awk -v c="$CHANNEL" 'index($1, c) == 1 { print $NF; exit }')
if [ -z "$TC" ]; then
  echo "build-linux: rustup has no toolchain $CHANNEL; run: rustup toolchain install $CHANNEL" >&2
  exit 1
fi
rustup target add --toolchain "$CHANNEL" "$TARGET" >/dev/null

HOSTTRIPLE=$("$TC/bin/rustc" -vV | sed -n 's/^host: //p')
LLD=$REPO/target/lld
mkdir -p "$LLD/bin" "$LLD/lib"
cp -f "$TC/lib/rustlib/$HOSTTRIPLE/bin/rust-lld" "$LLD/bin/rust-lld"
ln -sf "$TC/lib/libLLVM.dylib" "$LLD/lib/libLLVM.dylib"
ln -sf rust-lld "$LLD/bin/ld.lld"

VAR=$(echo "$TARGET" | tr 'a-z-' 'A-Z_')
export PATH="$TC/bin:$PATH" RUSTC="$TC/bin/rustc"
export "CARGO_TARGET_${VAR}_LINKER=$LLD/bin/ld.lld"
export "CARGO_TARGET_${VAR}_RUSTFLAGS=-C linker-flavor=ld.lld"
export CARGO_PROFILE_RELEASE_STRIP=false
if [ "$MODE" = "--lint" ]; then
  nice -n 15 "$TC/bin/cargo" clippy --all-targets -j 4 --target "$TARGET" -- -D warnings >&2
  exit 0
fi
nice -n 15 "$TC/bin/cargo" build --release -j 4 --target "$TARGET" >&2
echo "$REPO/target/$TARGET/release/tmux-companion"
