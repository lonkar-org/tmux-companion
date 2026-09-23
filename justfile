# Every command this repository has: building it, checking it, running the
# playground, and making the recordings. The cargo lines had been retyped from
# memory all week, with the nice level and the job count remembered differently
# each time, which is the shape of a thing that wants a recipe.
#
# The ci-mirroring recipes run the same commands .github/workflows/ci.yml runs,
# in the same order, so a green `just ci` means the workflow's *steps* are
# right. It says nothing about the *runner*. That gap is not theoretical here:
# CI installs tmux 3.4 and this laptop has 3.7c, and a bare `-t 0` target means
# a session to one and a window index to the other, so a test passed locally
# and failed on CI for two pushes. The act-* recipes run the real workflow in
# the real runner image and are the only local check that covers it.
#
# One more that `just ci` catches and a bare `cargo test` does not: cargo stops
# at the first test binary that fails, so two red unit tests hid the whole e2e
# suite, which hid the whole socket suite. Three separate failures sat behind
# one while every local run reported green. `test` passes --no-fail-fast for
# that reason and not as a preference.

# ORDER: `default` first, then every recipe alphabetically. Component-first
# names - act-*, demo-*, playground-* - so related recipes sort into a block
# without a header claiming a grouping that sorting would scatter.

# `just --list` prints only the LAST line of a recipe's comment block, so each
# one below ends with a one-line summary and keeps its reasoning above a blank
# line. Write a four-line explanation with no summary and the recipe list
# advertises itself as "cover it."

set shell := ["bash", "-uc"]

# Pinned above the alphabetical run, not sorted into it: a bare `just` runs
# whichever recipe comes FIRST, and sorting this one into place would put
# `act-ci` there and make the no-argument command spin up Docker.

# Show the recipe list.
default:
    @just --list

# The act-* recipes need Docker running. Get the runner image with
# `just act-refresh` before the first run; they will not fetch it for you.
#
# --pull=false on every one of them. act defaults to forcePull=true, so each
# run re-checks the runner image against the registry before doing any work,
# which is a needless round trip before every pre-push check.
#
# The cost is that the local image never ages out on its own, so it can drift
# from what GitHub runs, which is the drift act is here to catch. Refresh
# deliberately, not never.
#
# -P names the runner image on every recipe. act asks for it interactively the
# first time on a machine that has no actrc, and a non-interactive caller gets
# `level=fatal msg=EOF` instead of a question, which reads like a network
# failure. Naming it here means these recipes depend on nothing outside the
# repository but the image itself.

# ci.yml, every job, in containers.
act-ci:
    act --pull=false -P ubuntu-latest=catthehacker/ubuntu:act-latest -W .github/workflows/ci.yml push

# The slow one: a cold cargo build with no cache, plus tmux installed into the
# container before anything runs.

# ci.yml, the clippy and tests job.
act-check:
    act --pull=false -P ubuntu-latest=catthehacker/ubuntu:act-latest -W .github/workflows/ci.yml push -j check

# ci.yml, the rustdoc job.
act-docs:
    act --pull=false -P ubuntu-latest=catthehacker/ubuntu:act-latest -W .github/workflows/ci.yml push -j docs

# ci.yml, the rustfmt job. Seconds, and the one worth running first.
act-format:
    act --pull=false -P ubuntu-latest=catthehacker/ubuntu:act-latest -W .github/workflows/ci.yml push -j format

# What would run, without running it.
act-list:
    act --pull=false -P ubuntu-latest=catthehacker/ubuntu:act-latest -W .github/workflows/ci.yml push --list

# Needed once before the first local run, and worth re-running when GitHub's
# runners have moved - otherwise the act-* recipes keep using whatever was
# pulled last. Pulls directly rather than through act, so a stuck credential
# helper shows up as a hung `docker pull` rather than a silent act.

# Fetch or refresh the runner image the act-* recipes use.
act-refresh:
    docker pull catthehacker/ubuntu:act-latest

# nice -n 15 and four jobs on everything that compiles, because a full build
# otherwise saturates every core on a machine somebody is working on.

# The release binary.
build:
    nice -n 15 cargo build --release -j 4

# Everything ci.yml runs, native.
ci: fmt-check lint test doc
    @echo "rustfmt, clippy, tests and rustdoc all passed"

# Throw away the build output.
clean:
    cargo clean

# Both arms, from inside a real tmux: a key is pressed, the popup opens, and
# the clock runs until the first row of results reaches the client. The before
# arm is the actual zsh, read out of mysetup at the commit that replaced it,
# so this needs the author's machine; elsewhere it measures the new arm alone
# and says so.

# Every benchmark, both arms.
bench WHAT="all":
    ./scripts/bench.sh {{WHAT}}

# What the status bar costs per second.
bench-bar:
    ./scripts/bench.sh bar

# Keypress to first row, and CPU per press.
bench-pickers:
    ./scripts/bench.sh pickers

# Daemon CPU per request, today's binary only.
bench-segments:
    ./scripts/bench.sh segments

# Did the reel show what it claims, and is anything in it unpublishable?
demo-cast-check CAST="demo/recordings/usage.cast":
    ./demo/check-cast.py {{CAST}}

# Named chapters, for working on one without a four minute wait.
demo-chapters +CHAPTERS:
    ./demo/record-demo.sh {{CHAPTERS}}

# Render the README loop from gif.cast.
demo-gif:
    ./demo/make-gif.sh

# Times come from replaying the cast rather than the driver's clock, because
# the driver starts before asciinema does and waits for a client. `--print`
# gives the list asciinema.org's Markers box takes, which is seconds with one
# decimal and not mm:ss.

# A marker per chapter of a cast.
demo-markers CAST="demo/recordings/usage.cast" *ARGS:
    ./demo/add-markers.py {{CAST}} {{ARGS}}

# demo/ is gitignored, so this and the other demo-* recipes only work in a
# checkout that has it.

# Record a reel: usage or gif.
demo-record REEL="usage":
    ./demo/record-demo.sh --reel {{REEL}}

# rustdoc is a CI job of its own and fails on a broken intra-doc link, which is
# what a rename leaves behind. Neither clippy nor the tests cover it.

# Build the docs, the way the docs job does.
doc:
    nice -n 15 cargo doc --no-deps -j 4

# Format everything.
fmt:
    cargo fmt --all

# Fail if anything is unformatted, the way the format job does.
fmt-check:
    cargo fmt --all --check

# Clippy with -D warnings, the way the check job does.
lint:
    nice -n 15 cargo clippy --all-targets -j 4 -- -D warnings

# Read the manual page as it will ship.
man:
    man ./docs/tmux-companion.1

# Build if needed, then run the guided tour container.
playground:
    ./scripts/playground.sh

# Build the playground image.
playground-build:
    ./scripts/playground.sh build

# Check the image is what the tour claims.
playground-smoke:
    ./scripts/playground.sh smoke

# The hook installing cleanly says nothing about whether it works. zsh redraws
# the prompt line after precmd and takes the mark with it, so this was broken
# on the author's own shell for months while every byte was printed correctly.
# Only driving a real tmux and watching the cursor catches it.

# Does shell-init give tmux prompts it can actually jump between?
prompt-marks *SHELLS:
    ./scripts/check-prompt-marks.sh {{SHELLS}}

# --no-fail-fast is not a preference. See the note at the top of this file.

# Every test, including the ones a failure would otherwise hide.
test:
    nice -n 15 cargo test -j 4 --all-targets --no-fail-fast

# A green run is not a quiet one. Four actions sat on Node 20 for weeks with
# every run reporting success, because the warning lives in the annotations
# block that `gh run view` does not print. This waits, then prints both.
#
# It stops rather than hangs when a job is parked on an environment approval,
# which is what release.yml's publish job does on purpose.

# Wait for a workflow run and print its jobs and annotations.
watch-run RUN="":
    ./scripts/watch-run.sh {{RUN}}
