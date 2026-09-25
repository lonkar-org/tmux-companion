# Give an agent the skill

`skills/tmux-companion/SKILL.md` is the instruction sheet for a coding agent
sharing your tmux server: which command owns what, which pickers it must never
open, how the two stores differ, and what `resurrect` is allowed to run. It is
written against the binary rather than against a snapshot of the flags, so it
tells an agent to ask `tmux-companion <cmd> --help` instead of carrying a list
that goes stale.

## Claude Code

The repository is its own marketplace, so nothing else has to be published:

```sh
claude
/plugin marketplace add lonkar-org/tmux-companion
/plugin install tmux-companion@tmux-companion
```

A checkout works the same way, which is how you try a change before pushing it:

```sh
/plugin marketplace add ~/lonkar-org/tmux-companion
```

`.claude/skills/tmux-companion` in this repository is a symlink to the same file,
so a session started inside the checkout picks it up with no install at all.

## Anything else

The file is plain Markdown with YAML frontmatter and nothing in it is specific to
one vendor, so the install is a copy:

```sh
mkdir -p ~/.claude/skills/tmux-companion
curl -fsSL https://raw.githubusercontent.com/lonkar-org/tmux-companion/main/skills/tmux-companion/SKILL.md \
  -o ~/.claude/skills/tmux-companion/SKILL.md
```

For an agent that reads one context file, `AGENTS.md` or `.cursorrules` or
whatever it calls it, a line pointing at the path is enough:

```md
When working inside tmux, follow ~/.claude/skills/tmux-companion/SKILL.md.
```

## Checking it did anything

The last section of the skill is the test. A session that used it stayed in the
project session it found, kept one agent pane per repository, named the window
after the task, and did not open a picker without `--print`. If the agent created
a session called `0`, or resurrected over a live server, it never read the file.

`claude plugin validate .` checks the manifests after an edit, and
`claude plugin details tmux-companion` prints what the plugin loads and what it
costs in tokens.

## Releasing a change to it

The skill carries its own version and its own tags, `tmux-companion--v0.3.0`,
which `release.yml` does not match, so cutting one never builds a binary:

```sh
just plugin-check              # is the skill ahead of its last tag?
just plugin-release 0.3.0      # bump both manifests, validate, commit, tag, push
```

Pass `--dry-run` through for the steps without the consequences,
`just plugin-release 0.3.0 --dry-run`.
