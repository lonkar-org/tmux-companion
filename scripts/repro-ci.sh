#!/usr/bin/env bash
#
# Run the test suite the way the ubuntu CI job runs it, here, in a loop.
#
# usage:
#   scripts/repro-ci.sh                     12 runs of e2e and config_file
#   RUNS=30 scripts/repro-ci.sh             more of them
#   TESTS="--test e2e" scripts/repro-ci.sh  one suite
#
# Needs Docker. Everything happens in a throwaway Ubuntu 24.04 container, which
# is what `ubuntu-latest` is: tmux 3.4 rather than the 3.7 a Mac has, four
# cores rather than sixteen, and Linux paths rather than APFS.
#
# It exists because a test failed on CI for a week and never here. Reruns on a
# laptop prove nothing about a race: this machine has sixteen cores, so four
# spinning loops still leave twelve free, and twenty-two runs of the failing
# test passed. In here it failed twelve times out of twelve and printed the one
# line that explained it -- the tool was talking to the default tmux socket
# instead of the test's server, and had been all along.
#
# --cpuset-cpus and not --cpus. The latter is a CFS quota, so `nproc` still
# reports every core the host has and cargo still starts that many test
# threads, which is not the shape a four-core runner has. The cpuset changes
# what the container can see.
set -euo pipefail

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
REPO=$(cd "$HERE/.." && pwd)
RUNS=${RUNS:-12}
TESTS=${TESTS:---test e2e --test config_file}
CPUS=${CPUS:-0-3}

command -v docker >/dev/null || { echo "docker is not installed" >&2; exit 1; }
docker info >/dev/null 2>&1 || { echo "docker is not running" >&2; exit 1; }

tar=$(mktemp -t tcrepro).tar
trap 'rm -f "$tar"' EXIT
# The tracked files and nothing else: target/ is gigabytes of the wrong
# platform's objects, and demo/ and the recordings are not in git anyway.
git -C "$REPO" ls-files -z | (cd "$REPO" && xargs -0 tar -cf "$tar")

inner=$(mktemp -t tcinner).sh
trap 'rm -f "$tar" "$inner"' EXIT
cat > "$inner" <<'INNER'
set -euo pipefail
export DEBIAN_FRONTEND=noninteractive
apt-get update -qq >/dev/null
apt-get install -y -qq tmux git curl build-essential pkg-config ca-certificates >/dev/null
echo "tmux:  $(tmux -V)"
echo "cores: $(nproc)"

curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
  | sh -s -- -y --default-toolchain none --profile minimal >/dev/null
export PATH="$HOME/.cargo/bin:$PATH"

mkdir -p /w && tar -xf /repo.tar -C /w 2>/dev/null
cd /w
# A repository, because `gst` renders nothing for a directory that is not one
# and a test that asserts the bar drew something then fails for a reason that
# has nothing to do with the code.
git init -q . && git -c user.email=b@e -c user.name=bench commit -qm base --allow-empty
export CARGO_TARGET_DIR=/build CARGO_TERM_COLOR=never
rustup show active-toolchain >/dev/null 2>&1 || rustup toolchain install >/dev/null
echo "rust:  $(rustc --version)"

echo "=== building ==="
# shellcheck disable=SC2086
cargo test --no-run $TESTS >/dev/null 2>&1
echo "built"

fail=0
for i in $(seq 1 "$RUNS"); do
  # shellcheck disable=SC2086
  if out=$(cargo test $TESTS --no-fail-fast 2>&1); then
    printf .
  else
    printf F
    fail=$((fail + 1))
    if [ "$fail" = 1 ]; then
      printf '\n=== first failure, run %s ===\n' "$i"
      echo "$out" | grep -E "FAILED|panicked at|^ +[a-z_ ]+:" -A 2 | head -40
    fi
  fi
done
printf '\n=== %s of %s runs failed ===\n' "$fail" "$RUNS"
INNER

echo "running $RUNS times on cpuset $CPUS"
docker run --rm --cpuset-cpus="$CPUS" --memory=8g \
  -e "RUNS=$RUNS" -e "TESTS=$TESTS" \
  -v "$tar":/repo.tar:ro -v "$inner":/repro.sh:ro \
  ubuntu:24.04 bash /repro.sh
