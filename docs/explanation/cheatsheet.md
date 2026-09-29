# Why the cheat sheet forgets

<!-- @Yogesh(check): the premise; is a lookup really "not learned yet" for you, or also "forgot it"? -->
A binding you press doesn't need a cheat sheet. The one you open `prefix ?` to
search for does, because you looked it up instead of pressing it and that's the
only signal the tool gets about what you haven't learned yet. So the sheet
isn't a score of what you use. It's the list of keys you still search for.

## Looked up, then learned

<!-- @Yogesh(check): the 14-day default and the "learned drops off" rule -->
Each pick in the key search goes into the usage log with its time. A binding
looked up in the last 14 days (`[usage] learned_after_days`) sits at the top of
its box with a `▸` and its count, most lookups first. After 14 days without a
lookup it drops off the sheet and the line under the sheet counts it as
learned. Looking it up again brings it back. With the days set to 0 nothing is
ever learned.

<!-- @Yogesh(check): old logs with no times count as learned; right call, or should they show once? -->
A log written before picks carried a time has counts and no dates. Those
bindings count as learned, since a count with no date says nothing about this
week. The next lookup puts a date on them.

## Three boxes and one more

Your own bindings, the ones noted `companion: `, fill three boxes by the word
after the prefix: panes, windows, sessions and projects in one, copy mode,
opening and searching in the next, and config, help and anything else in the
third. The ones you never looked up follow the marked ones in alphabetical
order. The fourth box holds tmux's own bindings and the ones plugins add, but
only those you've looked up. tmux ships about a hundred noted defaults and
listing them all is the man page again. A default shows tmux's note and a
plugin key bound with no note shows its command.

## What gets recorded

Nothing, until `[usage] enabled = true`. With it off the sheet lists your
bindings unranked, the fourth box stays empty and one line says it learns only
with the log on. With it on each pick writes one line to
`$XDG_STATE_HOME/tmux-companion/keys-usage.tsv` with the key table, the key and
the time, as `prefix	%	@1790000000`. It's a plain file on your machine that
nothing else reads. Past five thousand lines it's rewritten as one line per
binding with the count and the first and last time.
