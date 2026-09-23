# Common tasks. `just` on its own lists them.
#
# Everything here runs nice -n 15 with four jobs, because a full cargo build
# otherwise saturates every core on a machine somebody is using.
#
# The one to reach for before pushing is `just ci`, which runs what the CI
# workflow runs, in the same order, with the same flags. `just act-ci` runs the
# workflow itself in a container when the difference between "the same commands"
# and "the same environment" is the thing under investigation.

set shell := ["bash", "-euo", "pipefail", "-c"]

NICE := "nice -n 15"
JOBS := "-j 4"

default:
    @just --list

# ── building and checking ────────────────────────────────────────────────────

# Release binary
build:
    {{NICE}} cargo build --release {{JOBS}}

# --no-fail-fast is not a preference. `cargo test` stops at the first test
# binary that fails, so two red unit tests kept the whole e2e suite and the
# whole socket suite from running at all, and three separate failures sat
# behind one for weeks. Plain `cargo test` reports green far too easily.

# Every test, including the ones a failure would otherwise hide
test:
    {{NICE}} cargo test {{JOBS}} --all-targets --no-fail-fast

# Exactly what CI runs, so a warning here is a red build there.
lint:
    {{NICE}} cargo clippy --all-targets {{JOBS}} -- -D warnings

# Format everything
fmt:
    cargo fmt --all

# Fail if anything is unformatted, the way CI does
fmt-check:
    cargo fmt --all --check

# rustdoc is a CI job of its own and fails on a broken intra-doc link, which is
# what a rename leaves behind. Running clippy and tests does not cover it.

# Build the docs, the way the rustdoc job does
doc:
    {{NICE}} cargo doc --no-deps {{JOBS}}

# The four CI jobs, in the order CI runs them, without pushing anything.
ci: fmt-check lint test doc
    @echo "rustfmt, clippy, tests and rustdoc all passed"

# Worth it when the failure is about the environment and not the code: CI runs
# tmux 3.4 where this laptop has 3.7, and a bare `-t 0` target means different
# things to the two of them. --pull=false keeps it off the network once the
# runner image is local.

# Run the CI workflow itself, in a container
act-ci *ARGS:
    act --pull=false {{ARGS}}

# One job, when only one of them is red.
act-job JOB *ARGS:
    act --pull=false -j '{{JOB}}' {{ARGS}}

# ── the manual ───────────────────────────────────────────────────────────────

# Read the manual page as it will ship
man:
    man ./docs/tmux-companion.1

# ── things that need a real tmux ─────────────────────────────────────────────

# The hook installing cleanly says nothing about whether it works: zsh redraws
# the prompt line after precmd and took the mark with it, so this was broken on
# the author's own shell while every byte was being printed correctly.

# Does shell-init give tmux prompts it can actually jump between?
prompt-marks *SHELLS:
    ./scripts/check-prompt-marks.sh {{SHELLS}}

# ── the playground container ─────────────────────────────────────────────────

# Build if needed, then run the guided tour container
playground:
    ./scripts/playground.sh

# Build the playground image
playground-build:
    ./scripts/playground.sh build

# Check the image is what the tour claims
playground-smoke:
    ./scripts/playground.sh smoke

# ── the recordings ───────────────────────────────────────────────────────────
#
# demo/ is gitignored, so these only work in a checkout that has it.

# Record a reel: usage or gif
record REEL="usage":
    ./demo/record-demo.sh --reel {{REEL}}

# Named chapters, for working on one without a four minute wait
record-chapters +CHAPTERS:
    ./demo/record-demo.sh {{CHAPTERS}}

# Did the reel show what it claims, and is anything in it unpublishable?
check-cast CAST="demo/recordings/usage.cast":
    ./demo/check-cast.py {{CAST}}

# Render the README loop from gif.cast
gif:
    ./demo/make-gif.sh

# Times come from the cast rather than the driver's clock, because the driver
# starts before asciinema does. `--print` gives the list asciinema.org takes.

# A marker per chapter
markers CAST="demo/recordings/usage.cast" *ARGS:
    ./demo/add-markers.py {{CAST}} {{ARGS}}
