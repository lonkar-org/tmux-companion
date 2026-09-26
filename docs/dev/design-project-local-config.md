# Design: a `.tmux-companion.toml` in the project root

Status: proposed, 2026-09-26. Queue item 7. Nothing here is built.

## The problem

A checkout can't say which layout it opens with. `[[project.override]]` matches
a path in one person's `config.toml`, so the same repository on a second machine,
or in a colleague's home, opens as whatever that machine's default is, and the
window names that make `M-a` and the status bar readable are gone with it.

`docs/dev/after-port-checklist.md` held this as item 4 because a file in a
checkout that can run commands is a file anybody who can push to the repository
can use to run commands on your machine, and a trust list was the only answer
on the table.

## Today

`saved::resolve` decides in this order: the saved layout file under the state
dir, then `[[project.override]]` by path prefix, then `[project] layout`, then
nothing, which is a plain shell. Every one of those lives outside the
repository.

## Options

1. **The full layout in the repository, behind a trust list.** `[[window]]` with
   `command`, the same shape as `[[layout]]`, and a per-path list in
   `config.toml` saying which checkouts are allowed to run what they say. The
   feature the held item described. The trust list is a second config to
   maintain and the first time somebody clones a repo they trust and it opens
   `nvim` they didn't ask for, they'll want to know why.

2. **Names only.** The file may say `layout = "<name>"`, naming a `[[layout]]`
   the person's own config defines, and `[[window]]` rows with `name` alone,
   so a checkout can say "two windows, `edit` and `ai`" and never what runs in
   them. A `command`, a `pane` table or any other key is a parse error that
   names the key, the same way `config check` reports one. Nothing in the file
   can start a process, so there's nothing to trust.

3. **Nothing.** Keep the override table, and say so in the docs.

## Recommendation

Option 2. It removes the reason the item was held and keeps the half people
ask for, which is the window names and the shape. Precedence: the saved file
still wins, because it was captured from this machine; then the repository
file; then `[[project.override]]`; then the default. `project show` names the
repository file when it decided, the same way it names a saved one, and names
it when it was ignored and why. `config check` learns the file as a second
input so a bad key is reported before anybody opens the project.

The cost is small: one parser with `deny_unknown_fields`, one step in
`resolve`, a paragraph in the manual and a `[[window]]` table in
`docs/config.example.toml` marked as the subset the repo file takes.

## What would change my mind

If a name-only file turns out to be a file nobody writes, because the layout
it names still has to exist in each person's config, then the window names
alone have to carry the feature. I think they do, since a plain shell called
`edit` beside one called `ai` is already what `M-a` and the bar need, and I
haven't measured that against anybody but me.

## Left open

The trust list, and with it commands from a checkout. Option 2 doesn't close
that door; it's the same file with a key it refuses today.
