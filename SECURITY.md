# Security Policy

## Supported Versions

Fixes land on the latest release. I don't backport to older lines, so
upgrading is the fix.

| Version | Supported          |
| ------- | ------------------ |
| >= 0.7  | :white_check_mark: |
| < 0.7   | :x:                |

## Reporting a Vulnerability

Please don't open a public issue for it. Report it privately through GitHub
instead:

<https://github.com/lonkar-org/tmux-companion/security/advisories/new>

Tell me what you found, the version (`tmux-companion --version`), your OS and
tmux version, and the steps to reproduce it. The output of
`tmux-companion doctor` usually covers most of that, but check it for paths
or hostnames you'd rather not share first.

What happens next:

- I'll acknowledge the report within 7 days. This is maintained by one
  person, so it's sometimes faster and occasionally not.
- I'll tell you whether I can reproduce it and whether I consider it a
  vulnerability, and why if I don't.
- If it is one, I'll work on a fix in a private fork, agree a disclosure date
  with you, and publish a release and a GitHub security advisory together.
  You'll be credited in the advisory unless you'd rather not be.

## What's in scope

Things I'd especially like to hear about:

- The unix socket: another local user being able to connect to it, read from
  it, or make the daemon do something.
- `open`, `run` and the pickers: text on the screen, a URL, a file path, a
  branch name or a shell history entry that ends up executed as a command.
- `scripts/install.sh`: anything that defeats the checksum verification or
  writes somewhere it shouldn't.
- The config file or theme files leading to code execution beyond what the
  file says it does.

Out of scope: anything that needs an attacker who is already your user on
your machine, and bugs in tmux, zoxide, fzf or your shell themselves. Report
those upstream, though I'm glad to hear about them if tmux-companion makes
them worse.
