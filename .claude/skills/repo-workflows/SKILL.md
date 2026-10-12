---
name: repo-workflows
description: "tmux-companion maintenance workflows: building and deploying the docs site (Starlight under site/), releasing the Claude Code plugin (skills/tmux-companion, its own tag namespace), and changing icon codepoints in src/tmux/icons.rs. Load before touching site/, the plugin manifests or icons.rs."
---

# Repository workflows

Moved out of CLAUDE.md so they load only when needed.

## The docs site

<https://with-love.lonkar.org/tmux-companion/> is `docs/` built with Starlight
from `site/`. It is a project site under the org's Pages domain, so it has a
base path, `/tmux-companion`, set once in `site/scripts/sync.mjs`. `site/scripts/sync.mjs` turns the docs into pages at build time: each
H1 becomes the page title, relative links become routes or GitHub links, the
man page goes through mandoc, and the example configs get a page each. Nothing
it generates is committed, so `docs/` stays the one source and reads the same
on GitHub. Only tutorial, how-to, reference and explanation are published;
`dev/` and `backlog/` are not.

`.github/workflows/docs.yml` deploys it to GitHub Pages on every push to
main that touches `docs/`, the README, the changelog or `site/`, so a docs fix
needs no release. While CHANGELOG.md's Unreleased section has entries, every
page carries a banner naming the latest release, since the site can describe
what brew can't install yet; cutting a release empties Unreleased and the
banner goes. A pull request that touches the docs builds the site without
deploying. `just docs-dev` serves it with live reload; `just docs-build`
builds it the way CI does.

A new page needs its file under one of the four directories, with an H1 on
its first line. The tutorial, how-to and explanation sidebars are generated
from their directories; reference lists its pages in `site/astro.config.mjs`,
so a new reference page gets a line there too. A link to a file outside the
four becomes a GitHub link at the commit the site was built from.

## The skill

`skills/tmux-companion/SKILL.md` is the instruction sheet a coding agent loads
before it touches this tmux server, and the repository ships it as a Claude Code
plugin: `.claude-plugin/plugin.json` and `.claude-plugin/marketplace.json` make
the checkout its own marketplace, and `.claude/skills/tmux-companion` is a
symlink so a session started here picks it up with no install.

An agent acts on that file without checking it, so a command removed from the
CLI has to leave the skill in the same commit.
`the_skill_names_no_command_that_went_away` in `tests/e2e.rs` reads every
backticked span and fenced line in the file and holds the commands, subcommands
and long flags against `--help`, which is the same trick the manual tests use.

The plugin has its own version and its own tag namespace,
`tmux-companion--v0.3.0`, because SKILL.md changes on a different schedule from
the binary and `release.yml` triggers on `v*`:

```sh
just plugin-check              # is the skill ahead of its last tag?
just plugin-release 0.3.0      # bump both manifests, validate, commit, tag, push
```

## Changing icon codepoints

Edit `src/tmux/icons.rs`. Icon constants are used directly in tests via the
constant name, so a codepoint change automatically propagates to all tests —
no manual expected-string updates needed.

After changing a codepoint that appears in tmux output, rebuild and do a
side-by-side visual check with the previous tool:

```sh
just build
./target/release/tmux-companion restart   # not pkill; see "Running manually"
./target/release/tmux-companion gst /some/repo
yrl gst /some/repo   # or the previous shell script
```
