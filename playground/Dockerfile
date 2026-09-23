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
FROM rust:1-alpine AS build
RUN apk add --no-cache musl-dev
WORKDIR /src
# Cargo.lock and the manifest first, so a source-only change does not refetch
# the registry.
COPY Cargo.toml Cargo.lock rust-toolchain.toml build.rs ./
COPY src ./src
RUN cargo build --release --locked && strip target/release/tmux-companion


# ── The playground ──────────────────────────────────────────────────────────
FROM alpine:3.21

# git, because the status bar's largest segment reads it. neovim and zsh
# because the tour opens both. zoxide because the project picker asks it which
# directories exist. ncurses-terminfo so tmux knows what the host terminal is.
# mandoc and the tmux man page, because the last thing the tour says is "read
# the manual" and a container where `man tmux` answers "not found" makes a
# liar of it. mandoc rather than man-db: it renders the mdoc both pages are
# written in and costs a tenth as much.
RUN apk add --no-cache \
      tmux tmux-doc zsh bash git neovim zoxide \
      mandoc man-pages \
      ncurses ncurses-terminfo less findutils coreutils grep procps-ng \
 && adduser -D -s /bin/zsh play

ENV HOME=/home/play \
    XDG_CONFIG_HOME=/home/play/.config \
    XDG_DATA_HOME=/home/play/.local/share \
    XDG_STATE_HOME=/home/play/.local/state \
    LANG=C.UTF-8 \
    TERM=xterm-256color \
    EDITOR=nvim \
    PAGER=less

COPY --from=build /src/target/release/tmux-companion /usr/local/bin/tmux-companion
# The manual, where `man tmux-companion` looks for it.
COPY docs/tmux-companion.1 /usr/share/man/man1/tmux-companion.1
COPY docs/tmux.conf.full.example /opt/playground/tmux.conf.full.example
COPY docs/config.example.toml    /opt/playground/config.example.toml
COPY playground/seed.sh playground/tour.sh playground/entrypoint.sh playground/smoke.sh /opt/playground/
COPY playground/tmux.playground.conf /opt/playground/
RUN chmod +x /opt/playground/*.sh

USER play
WORKDIR /home/play

# Everything the tour needs is made now, at build time, so the container starts
# in under a second rather than doing this in front of the person.
RUN /opt/playground/seed.sh

ENTRYPOINT ["/opt/playground/entrypoint.sh"]
