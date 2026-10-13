# The link between two tmux-companion daemons on two machines.
#
# The local daemon runs a host command (`ssh vm`, or a wrapper that ends in
# ssh) with `tmux-companion bridge` as the remote command, and the two speak
# Cap'n Proto RPC over that session's stdin and stdout. The bridge's bootstrap
# object is a `Bridge`, and the allowlist is this interface: a method that is
# not here does not exist on the link, so nothing the local side sends can run
# a command on the host or stop its daemon.
#
# docs/dev/design-remote-sessions.md is the design. The Rust beside this file,
# link_capnp.rs, is generated from it by `just capnp` and committed, so no
# build needs the capnp tool; `just capnp-check` fails when it is stale.
#
# Evolving it: add fields and methods at the end with the next ordinal, never
# renumber or reuse one. An older end ignores a field it does not know and
# answers a method it does not have with `unimplemented`; the hello lists the
# methods a bridge serves so the local side never has to find out by asking.

@0xdc78c4415e46c163;

# One session on the host, as its daemon's pane poll last read it. Sessions
# carrying `@tc-mirror` (the grouped sessions another machine's mirror
# sessions attach to) are never sent.
struct Session {
  name @0 :Text;          # `#{session_name}`
  id @1 :Text;            # `#{session_id}`, the `$N`
  windows @2 :UInt32;     # `#{session_windows}`
  attached @3 :UInt32;    # clients attached on the host
  lastAttached @4 :UInt64; # unix seconds, 0 for never
  activity @5 :UInt64;    # unix seconds
  used @6 :UInt64;        # `@tc_ses_used`, the switcher's stamp, 0 for none
  path @7 :Text;          # `#{session_path}`
}

# One segment of a session's bar, structured so the receiving side builds the
# tmux string itself: nothing here is tmux markup, and every string is text.
struct Segment {
  role @0 :Text;          # `git`, `agents` or `health`
  text @1 :Text;
  fg @2 :Text;            # a colour name, empty for the bar's own
  bg @3 :Text;
  glyph @4 :Text;         # an icons.rs constant's name, empty for none
}

# One window of a grouped session, for the mirror session's bar to list in
# place of its own one window. Text, never markup, like a segment.
struct Window {
  index @0 :UInt32;       # `#{window_index}`
  name @1 :Text;          # `#{window_name}`
  active @2 :Bool;        # the grouped session's current window
  bell @3 :Bool;          # `#{window_bell_flag}`
}

# The theme a grouped session shows, for the mirror session to show too: the
# theme file's stem, `@theme-file`, and the `@theme-*` settings that file
# makes. Text, never markup: the receiving side checks every key and value
# and sets them as arguments, it never sources what a host sent.
struct Theme {
  file @0 :Text;          # `amber-light`, empty when the theme has no file
  settings @1 :List(Setting);
}

struct Setting {
  key @0 :Text;           # `@theme-color-main-1`
  value @1 :Text;         # `colour172`
}

# One agent waiting on the host, as its inbox holds it.
struct Entry {
  pane @0 :Text;          # `#{pane_id}`
  at @1 :Text;            # `session:window.pane`
  program @2 :Text;
  path @3 :Text;
  since @4 :UInt64;       # unix seconds
  state @5 :Text;         # `asked`, `done` or `waiting`
  question @6 :Text;
}

# What the local side hands the bridge with `watch`: the one object the host
# may call. The bridge calls it when something changed on its daemon's own
# poll, so the local side hears of a change without asking every few seconds.
# A method added here later is answered `unimplemented` by an older local
# side, which the bridge takes as that side not wanting it.
interface SessionSink {
  # The host's sessions, as `sessions` answers them, when the list changed.
  sessions @0 (polledAt :UInt64, sessions :List(Session)) -> ();

  # The agents waiting on the host, as `inbox` answers them, when they
  # changed.
  inbox @1 (entries :List(Entry)) -> ();

  # The segments of one grouped session the local side's mirror has a client
  # in, as `segments` answers them, when they changed; an empty list when the
  # session lost its client or went. An older bridge sends no theme.
  segments @2 (session :Text, polledAt :UInt64, segments :List(Segment),
               windows :List(Window), theme :Theme) -> ();
}

interface Bridge {
  # The first call on every link. `build` is the tmux-companion build id,
  # `protocol` this schema's number (1), `methods` every method this bridge
  # answers, `startedDaemon` whether the bridge had to start the host's
  # daemon, and `tmux` the host's `tmux -V`.
  hello @0 (build :Text, protocol :UInt16)
        -> (build :Text, protocol :UInt16, methods :List(Text), startedDaemon :Bool, tmux :Text);

  # The host's sessions, and when its daemon read them (unix seconds).
  sessions @1 () -> (polledAt :UInt64, sessions :List(Session));

  # The segments of one session's active pane: git, agents and health; and
  # its windows and theme, which a bridge from before them leaves empty.
  segments @2 (session :Text)
           -> (polledAt :UInt64, segments :List(Segment), windows :List(Window),
               theme :Theme);

  # The agents waiting on the host, longest wait first.
  inbox @3 () -> (entries :List(Entry));

  # Phase 3: the host's own configured directory source.
  dirs @4 () -> (paths :List(Text));

  # Pushed changes instead of a poll: the bridge calls `sink` with the
  # sessions, the inbox and the segments of the grouped sessions whose
  # `@tc-mirror` is `machine` and that have a client, first all of them and
  # then each when it changes. The call itself stays unanswered while the
  # link lives, so its end is the local side's sign that the link went.
  watch @5 (sink :SessionSink, machine :Text) -> ();

  # Nothing, answered: the local side asks every `ping_secs` while watching,
  # since with nothing polled a link that died quietly would go unnoticed.
  ping @6 () -> ();
}

# What the local daemon keeps on disk per host, `hosts/<name>.bin` in its
# state directory: the last `sessions` answer and when it arrived, so the
# first picker of the day lists a host before any ssh has run. Never sent over
# the link.
struct HostCache {
  heardAt @0 :UInt64;     # unix seconds the answer arrived
  polledAt @1 :UInt64;    # unix seconds the host's daemon read it
  build @2 :Text;         # the host's build, from its hello
  sessions @3 :List(Session);
}
