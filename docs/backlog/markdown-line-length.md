# Markdown line length

Deferred 2026-10-07. When `screen-reader-mode` lands, this file gets its row in
that branch's `docs/backlog/README.md`.

**What.** markdownlint's MD013, the 80-column line length, turned on in
`.markdownlint.jsonc` with tables, code blocks and headings exempt, and every
Markdown file rewrapped to pass it. Every other default rule is on already and
`markdown.yml` holds the tree to it.

**Why it waits.** With those exemptions it still fails about 300 lines, nearly
all in files written one paragraph per line, so passing means rewrapping whole
documents: `docs/dev/accessibility-research-and-checklist.md` alone is 78,
`docs/dev/design-key-routing.md` 68 and `docs/reference/cli.md` 28.
`screen-reader-mode` edits or moves several of the same files
(`companion-first.md`, `cli.md`, the accessibility research), so a rewrap on
main now turns that merge into a conflict in every paragraph they share.

**What brings it back.** `screen-reader-mode` merged. Then one commit that
rewraps and nothing else, so `git blame --ignore-rev` can skip it, and MD013
on in the same commit. Or a decision that one paragraph per line is the
convention, which turns MD013 off for good and deletes this file.
