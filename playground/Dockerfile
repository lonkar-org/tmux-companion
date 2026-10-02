# syntax=docker/dockerfile:1
#
# A tmux-companion playground: tmux, the binary, a config with everything
# turned on, five fake projects and a guided tour.
#
#   docker run --rm -it lonkarorg/tmux-companion:playground
#
# Nothing here touches the host. No volumes, no ports, and the container is
# meant to be thrown away, so anything typed inside it dies with it.
#
# Build it from the repository root, not from this directory:
#
#   docker build -f playground/Dockerfile -t tmux-companion:playground .
#
# or use scripts/playground.sh, which builds and runs in one step.

# ── The binary ──────────────────────────────────────────────────────────────
#
# Built from source rather than downloaded, so the image can be built from any
# checkout, including one with unreleased changes. musl comes free: the crate
# has no C dependencies, so the alpine toolchain links it static.
#
# Both bases are pinned by the digest of their multi-arch index, with the tag
# kept for whoever reads this. A tag moves under you; a digest builds the same
# bytes until somebody changes it on purpose, and .github/dependabot.yml is
# what does that, as a pull request. Resolve one by hand with
#   docker buildx imagetools inspect rust:1-alpine
FROM rust:1-alpine@sha256:7cc1c22d77d9432f7fe012a70e6d3e555af54c2a6832700ed7d553f1769ae89f AS build
RUN apk add --no-cache musl-dev
WORKDIR /src
# Cargo.lock and the manifest first, so a source-only change does not refetch
# the registry.
COPY Cargo.toml Cargo.lock rust-toolchain.toml build.rs ./
COPY src ./src
RUN cargo build --release --locked && strip target/release/tmux-companion


# ── The playground ──────────────────────────────────────────────────────────
FROM alpine:3.21@sha256:ce64758a109eb420d874a118f87920e625e12d3634e03b4a5573fd9f6e5d3507

# git, because the status bar's largest segment reads it. neovim and zsh
# because the tour opens both. zoxide because the project picker asks it which
# directories exist. ncurses-terminfo so tmux knows what the host terminal is.
# mandoc and the tmux man page, because the last thing the tour says is "read
# the manual" and a container where `man tmux` answers "not found" makes a
# liar of it. mandoc rather than man-db: it renders the mdoc both pages are
# written in and costs a tenth as much. procps-ng because `sessions save` and
# `kill` ask ps for what busybox's ps does not have (-ax, -t, tpgid).
#
# Nothing else. find, grep and the coreutils are busybox's, which is all the
# tour and smoke.sh ask of them, and man-pages (the Linux section 2 to 7
# pages) went because nothing in the tour opens one. No ss or lsof either, so
# `ports` has nothing to ask: the playground gave its key to the project
# picker.
#
# The user gets a fixed uid and gid, and USER below names them by number, so
# a runtime checking runAsNonRoot can tell it is not root without reading
# /etc/passwd.
RUN apk add --no-cache \
      tmux tmux-doc zsh bash git neovim zoxide \
      mandoc \
      ncurses ncurses-terminfo less procps-ng \
 && addgroup -g 1000 play \
 && adduser -D -u 1000 -G play -s /bin/zsh play

ENV HOME=/home/play \
    XDG_CONFIG_HOME=/home/play/.config \
    XDG_DATA_HOME=/home/play/.local/share \
    XDG_STATE_HOME=/home/play/.local/state \
    LANG=C.UTF-8 \
    TERM=xterm-256color \
    EDITOR=nvim \
    PAGER=less

# Everything on PATH and under /opt/playground stays root's and read-only to
# play: the tour can be broken from inside, the tool it teaches cannot.
COPY --from=build /src/target/release/tmux-companion /usr/local/bin/tmux-companion
# The manual, where `man tmux-companion` looks for it.
COPY docs/tmux-companion.1 /usr/share/man/man1/tmux-companion.1
COPY docs/tmux.conf.full.example /opt/playground/tmux.conf.full.example
COPY docs/config.example.toml    /opt/playground/config.example.toml
COPY --chmod=0755 playground/seed.sh playground/tour.sh playground/entrypoint.sh playground/smoke.sh playground/a11y.sh /opt/playground/
COPY playground/tmux.playground.conf playground/a11y-walkthrough.txt /opt/playground/

# Everything the tour needs is made now, at build time, so the container starts
# in under a second rather than doing this in front of the person.
#
# It is seeded as play and into /home/play, because what it writes holds
# absolute paths (zoxide's database, anvil-infra's remote) and they have to be
# the ones the container runs with. Then the whole home moves to
# /opt/playground/home, root's and read-only, and the entrypoint copies it
# back into an empty /home/play on start. That is 1.9 MB of cp, 12 ms onto a
# tmpfs and 33 ms onto the container's own layer, and it is what
# lets the container run with --read-only and a tmpfs on /home/play: a tmpfs
# mounted over a seeded home would hide every file in it. One RUN, so the
# seeded home lands in one layer rather than two.
#
# Last, as root before USER drops it, nothing keeps a setuid or setgid bit.
# Alpine ships none today; this makes that a property of the image rather than
# of whichever packages happen to be in it.
RUN su play -s /bin/sh -c /opt/playground/seed.sh \
 && mv /home/play /opt/playground/home \
 && chown -R root:root /opt/playground/home \
 && chmod -R a+rX,go-w /opt/playground/home \
 && install -d -o 1000 -g 1000 -m 0750 /home/play \
 && find / -xdev -perm /6000 -type f -exec chmod a-s {} +

USER 1000:1000
WORKDIR /home/play

ENTRYPOINT ["/opt/playground/entrypoint.sh"]
