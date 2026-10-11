#!/usr/bin/env bash
#
# bench-capped on another machine over ssh: push a revision there, build it
# there, run it there, and copy the run back here.
#
# usage:
#   scripts/bench-remote.sh HOST                   HEAD, the host's default loads
#   scripts/bench-remote.sh HOST HEAD~1 0 4 8      a revision and these loads
#   scripts/bench-remote.sh HOST 0                 HEAD, load 0 (a bare number is a load)
#   RUNS=2 ROUNDS=1 BAR=auto scripts/bench-remote.sh HOST HEAD 0
#   PREP=1 scripts/bench-remote.sh HOST            install rustup for the user first
#
# env:
#   RUNS ROUNDS ONLY BAR BAR_RUNS LABEL CONFIG
#                 passed to bench-capped.sh on the host when set here
#   REMOTE_DIR    the repository on the host      (default bench/tmux-companion,
#                                                 relative to the host's home)
#   TMUX_VERSION  pinned tmux the host builds and runs, or `system`
#                                                 (default 3.4, the floor)
#   HOST_TAG      prefix of the local copy        (default HOST up to its first dot)
#   PREP          1: install rustup (user-level, no sudo) when cargo is missing
#   HOST_RUST     a toolchain prefix on the host (relative to its home, or
#                 absolute) whose bin/ goes first on PATH for the build, so
#                 rustup and rust-toolchain.toml are bypassed (default none).
#                 For a host where rustup cannot fetch the pinned toolchain:
#                 the first one's rustup download trickled at 5 KB/s from one
#                 CDN node while another served 12 MB/s, and a toolchain
#                 unpacked from the same tarballs (each tarball's install.sh
#                 --prefix=DIR) was the way round. The build then uses that
#                 rustc, not the pinned one, and `cargo --version` in the log
#                 says which.
#
# The run lands in target/bench/<HOST_TAG>-<run>/ here.
#
# What gets there is a commit, not the working tree: the revision is pushed
# with git over the same ssh, into a repository on the host that this script
# creates and then checks out on a branch named bench, so nothing on the way
# needs a forge, a token or a network beyond the ssh link. Uncommitted changes are not benchmarked; commit them first.
#
# The host needs a C compiler, make, git, python3, zoxide, pkg-config,
# libevent and ncurses headers and bison (the last four for the pinned tmux).
# This script never runs sudo: it prints the `sudo apt install` line for what
# is missing, still builds when cargo and a compiler are there, and exits 3
# before the run.
#
# ssh runs with BatchMode, so a host key nobody has accepted, or a key the
# agent does not have, fails at once with ssh's own message on stderr rather
# than a prompt that hangs or a quiet nothing. Accept a new host key by hand,
# once, with a plain `ssh HOST true`.
set -euo pipefail

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
REPO=$(cd "$HERE/.." && pwd)
[ $# -ge 1 ] || { sed -n '3,/^# The run lands/p' "$0" >&2; exit 2; }
HOST=$1
shift
REV=HEAD
if [ $# -gt 0 ] && ! [[ "$1" =~ ^[0-9]+$ ]]; then
  REV=$1
  shift
fi
LOADS=("$@")
REMOTE_DIR=${REMOTE_DIR:-bench/tmux-companion}
TMUX_VERSION=${TMUX_VERSION:-3.4}
HOST_TAG=${HOST_TAG:-${HOST%%.*}}
PREP=${PREP:-0}

SHA=$(git -C "$REPO" rev-parse --verify "$REV^{commit}")
SHORT=$(git -C "$REPO" rev-parse --short "$SHA")
RUN=$(date +%Y%m%d-%H%M%S)-$SHORT${LABEL:+-$LABEL}${BAR:+-$BAR}
LOCAL_OUT=$REPO/target/bench/$HOST_TAG-$RUN

SSH=(ssh -o BatchMode=yes -o ConnectTimeout=15 -o ServerAliveInterval=30)
export GIT_SSH_COMMAND="ssh -o BatchMode=yes -o ConnectTimeout=15"

# One remote shell per step, its script passed as an argument, never on stdin:
# under `bash -s` a child that reads stdin would eat the rest of the script.
# The script travels base64-encoded because ssh hands the command line to the
# host's login shell, which may be a POSIX sh that cannot read bash's $'..'
# quoting; the arguments are plain words (paths, numbers, VAR=value).
remote() {
  local script=$1 b64
  shift
  b64=$(printf '%s' "set -euo pipefail; $script" | base64 | tr -d '\n')
  "${SSH[@]}" -n "$HOST" "bash -c \"\$(printf %s $b64 | base64 -d)\" bench-remote $(printf '%q ' "$@")"
}

echo "== $HOST: repository at ~/$REMOTE_DIR"
remote '
  dir=$HOME/$1
  if [ ! -d "$dir/.git" ]; then
    mkdir -p "$dir"
    git init -q "$dir"
    git -C "$dir" config receive.denyCurrentBranch updateInstead
    echo "created $dir"
  fi' "$REMOTE_DIR"

echo "== push $SHORT"
# Pushed to a ref nothing has checked out, then checked out on a branch: the
# bench times the bar until the branch name shows, and a detached HEAD has
# none, so the first real run reported "never showed the branch".
git -C "$REPO" push -q --force "$HOST:$REMOTE_DIR" "$SHA:refs/heads/incoming"
remote 'git -C "$HOME/$1" checkout -q -f -B bench "$2"; git -C "$HOME/$1" log --oneline -1' \
  "$REMOTE_DIR" "$SHA"

echo "== prerequisites"
# Exit 3 means only packages are missing: the build still runs, since it needs
# nothing but cargo and a C compiler, and the script stops before the run. A
# host is then built and waiting by the time somebody with sudo has run apt.
set +e
remote '
  # Packages, not commands: the message is the apt line to paste, and it
  # names only what this host lacks. The first real host lacked four of
  # eleven, and a line listing all eleven hid which four.
  missing=()
  if [ "$2" != system ]; then
    pkg-config --exists libevent 2>/dev/null || missing+=(libevent-dev)
    pkg-config --exists ncurses 2>/dev/null || pkg-config --exists ncursesw 2>/dev/null \
      || missing+=(libncurses-dev)
  else
    command -v tmux >/dev/null || missing+=(tmux)
  fi
  for pair in cc:build-essential make:make pkg-config:pkg-config bison:bison \
              zoxide:zoxide git:git python3:python3 curl:curl rsync:rsync; do
    command -v "${pair%%:*}" >/dev/null || missing+=("${pair#*:}")
  done
  if [ ${#missing[@]} -gt 0 ]; then
    echo "missing on $(hostname): ${missing[*]}" >&2
    echo "on Debian or Raspberry Pi OS:" >&2
    echo "  sudo apt install ${missing[*]}" >&2
  fi
  # `command -v cargo` is not enough: an interrupted rustup leaves the cargo
  # proxy in place with no default toolchain, and every call then fails.
  [ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
  if [ -n "$3" ]; then
    case $3 in /*) PATH=$3/bin:$PATH ;; *) PATH=$HOME/$3/bin:$PATH ;; esac
  fi
  if ! cargo --version >/dev/null 2>&1; then
    if [ "$1" = 1 ]; then
      if command -v rustup >/dev/null; then
        rustup default stable
      else
        curl --proto =https --tlsv1.2 -sSf https://sh.rustup.rs \
          | sh -s -- -y --profile minimal --no-modify-path
      fi
    else
      echo "no working cargo on $(hostname); rerun with PREP=1 to install rustup for this user" >&2
      exit 1
    fi
  fi
  if [ ${#missing[@]} -gt 0 ]; then
    command -v cc >/dev/null && exit 3
    exit 1
  fi' "$PREP" "$TMUX_VERSION" "${HOST_RUST:-}"
prereq=$?
set -e
[ "$prereq" -eq 0 ] || [ "$prereq" -eq 3 ] || exit "$prereq"

echo "== build (nice 15, 4 jobs)"
remote '
  . "$HOME/.cargo/env" 2>/dev/null || true
  if [ -n "$2" ]; then
    case $2 in /*) rust=$2 ;; *) rust=$HOME/$2 ;; esac
    PATH=$rust/bin:$PATH
  fi
  cd "$HOME/$1"
  mkdir -p target
  cargo --version
  if ! nice -n 15 cargo build --release -j 4 >target/bench-remote-build.log 2>&1; then
    tail -40 target/bench-remote-build.log >&2
    exit 1
  fi
  tail -1 target/bench-remote-build.log
  target/release/tmux-companion --version' "$REMOTE_DIR" "${HOST_RUST:-}"

if [ "$prereq" -eq 3 ]; then
  echo "built; not running until the packages above are installed" >&2
  exit 3
fi

# Only what is set here goes over, so the host keeps bench-capped's defaults.
PASS=()
for v in RUNS ROUNDS ONLY BAR BAR_RUNS LABEL CONFIG; do
  if [ -n "${!v:-}" ]; then PASS+=("$v=${!v}"); fi
done

echo "== bench-capped on $HOST: ${PASS[*]:-defaults} loads ${LOADS[*]:-default}"
set +e
remote '
  cd "$HOME/$1"
  run=$2
  tv=$3
  shift 3
  if [ "$tv" != system ]; then
    PATH="$(nice -n 15 scripts/tmux-build.sh "$tv"):$PATH"
    export PATH
  fi
  env_args=()
  while [ $# -gt 0 ] && [ "$1" != -- ]; do env_args+=("$1"); shift; done
  shift
  env ${env_args[@]+"${env_args[@]}"} OUT="$PWD/target/bench/$run" scripts/bench-capped.sh "$@"' \
  "$REMOTE_DIR" "$RUN" "$TMUX_VERSION" ${PASS[@]+"${PASS[@]}"} -- ${LOADS[@]+"${LOADS[@]}"}
status=$?
set -e

# Copied back whether or not the run finished: a half run still says where it stopped.
echo "== fetch into $LOCAL_OUT"
mkdir -p "$LOCAL_OUT"
if rsync -a -e "${SSH[*]}" "$HOST:$REMOTE_DIR/target/bench/$RUN/" "$LOCAL_OUT/"; then
  echo "wrote $LOCAL_OUT"
else
  echo "nothing fetched from $HOST:$REMOTE_DIR/target/bench/$RUN" >&2
  rmdir "$LOCAL_OUT" 2>/dev/null || true
  [ "$status" -ne 0 ] || status=1
fi
exit "$status"
