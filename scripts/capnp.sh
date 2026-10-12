#!/usr/bin/env bash
# Regenerate schema/link_capnp.rs from schema/link.capnp, or check it is fresh.
#
# usage: scripts/capnp.sh            write schema/link_capnp.rs
#        scripts/capnp.sh --check    regenerate into a temporary directory and
#                                    fail when it differs from the committed file
#
# The generated Rust is committed so that no build needs the capnp tool: a
# source install, the brew formula and crates.io build with cargo alone. That
# leaves one way for it to go wrong, a schema edited without regenerating, and
# --check is what catches it. `just ci` runs it, and so does ci.yml's capnp job.
#
# Two tools: `capnp`, the schema compiler (brew install capnp, apt install
# capnproto), and `capnpc-rust`, its Rust plugin, which is the `capnpc` crate's
# binary. The plugin is installed into target/capnpc at the version CAPNPC_VERSION
# names, which has to match the `capnp` runtime's minor version in Cargo.toml:
# generated code from one minor does not compile against another's runtime.
#
# Without `capnp` on PATH --check prints one line and exits 0, so a contributor
# who never touches the schema can still run `just ci`; with CI set it fails
# instead, the same rule tests/e2e.rs follows for a missing tmux.
#
# The output is written beside the schema rather than under src/: lib.rs
# include!s it inside `mod link_capnp`, which keeps rustfmt off a file nobody
# edits by hand and keeps the generated `crate::link_capnp` paths valid.
set -euo pipefail

CAPNPC_VERSION=0.27.0

case "${1:-}" in
  --check) MODE=check ;;
  "")      MODE=write ;;
  -h|--help) sed -n '2,25p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
  *) echo "usage: $0 [--check]" >&2; exit 2 ;;
esac

root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"

if ! command -v capnp >/dev/null 2>&1; then
  if [ -n "${CI:-}" ]; then
    echo "capnp is not installed, and CI is set: the staleness check cannot be skipped there" >&2
    exit 1
  fi
  if [ "$MODE" = check ]; then
    echo "capnp-check: skipped, no capnp on PATH (brew install capnp)"
    exit 0
  fi
  echo "no capnp on PATH: brew install capnp, or apt install capnproto" >&2
  exit 1
fi

tool="$root/target/capnpc"
plugin="$tool/bin/capnpc-rust"
if [ ! -x "$plugin" ] || ! grep -qx "$CAPNPC_VERSION" "$tool/version" 2>/dev/null; then
  # RUSTFLAGS cleared: ci.yml sets -D warnings for this repository's code,
  # and a warning in the plugin's own source is not this repository's to fix.
  RUSTFLAGS="" nice -n 15 cargo install --quiet --locked --root "$tool" --version "$CAPNPC_VERSION" capnpc
  echo "$CAPNPC_VERSION" > "$tool/version"
fi

out=$(mktemp -d)
trap 'rm -rf "$out"' EXIT
capnp compile --src-prefix=schema -o"$plugin:$out" schema/link.capnp

if [ "$MODE" = write ]; then
  cp "$out/link_capnp.rs" schema/link_capnp.rs
  echo "wrote schema/link_capnp.rs"
  exit 0
fi

# The header names the capnp binary's version, which is brew's here and apt's
# on the CI runner; the code under it is the same, so that one line is left
# out of the comparison.
strip() { grep -v '^// capnp binary version:' "$1"; }
if ! diff -u <(strip schema/link_capnp.rs) <(strip "$out/link_capnp.rs") >/dev/null; then
  echo "schema/link_capnp.rs is stale against schema/link.capnp: run \`just capnp\` and commit it" >&2
  diff -u <(strip schema/link_capnp.rs) <(strip "$out/link_capnp.rs") | head -40 >&2 || true
  exit 1
fi
echo "capnp-check: schema/link_capnp.rs matches schema/link.capnp"
