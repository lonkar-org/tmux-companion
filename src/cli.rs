//! The command-line surface: the `Cmd` enum clap parses into, and the dispatch
//! that turns one of its variants into a request to the server.

use std::path::PathBuf;

use crate::proto::{ClientsArgs, GstArgs, Request, ShJobsArgs, StatusRightArgs};
use clap::{Parser, Subcommand};

/// The parsed command line.
///
/// `--version` reports the build stamp rather than the crate version, which is
/// what `doctor` and the daemon handshake compare and what the installer reads
/// to decide whether it has anything to do. During development every build
/// carries the same version number, so the crate version on its own answers the
/// wrong question.
///
/// `long_about` is spelled out rather than left to clap, and that is the point
/// of it: clap takes a doc comment's first line as `about` and the rest as
/// `long_about`, so the paragraph above -- a note to whoever is editing this
/// file -- was what `tmux-companion --help` opened with. The first thing
/// somebody sees after installing it read like somebody else's memo.
#[derive(Parser)]
#[command(
    name = "tmux-companion",
    about = "A status line, a set of pickers and a project manager for tmux",
    long_about = "A status line, a set of pickers and a project manager for tmux.

One daemon answers for all of it. `server` is that daemon; every other command is a client that connects to it, starting one if nothing is listening, prints what it gets back and exits. The point of the split is the status bar: tmux spawns a process per `#()` per refresh per attached client, so the bill is the number of distinct commands rather than what they do.

`tmux-companion doctor` prints what a bug report needs. The manual page, `man tmux-companion`, has the rest.",
    version = crate::proto::BUILD_ID
)]
pub struct Cli {
    /// The subcommand to run, which decides whether this process is the server
    /// or a client.
    #[command(subcommand)]
    pub command: Cmd,
}

/// Every subcommand. `server` is the daemon; the rest are clients.
///
/// Three short flags mean different things on different commands and stay
/// that way, because renaming one breaks a binding somebody wrote for no
/// request: `-s` is `--style` on gst and status-right, the selection on open
/// and the start path on window; `-n` is `--dry-run` on open, the name on
/// window and the count on probe keys; `-i` is the index on window and
/// `--choose` on open.
#[derive(Subcommand, Debug)]
#[command(rename_all = "kebab-case")]
pub enum Cmd {
    /// Run as persistent background server
    Server,

    /// Git status segment
    Gst {
        /// Path to git repository (defaults to current directory)
        path: Option<PathBuf>,
        /// Pane pid, which adds the suspended-editor marker
        pane_pid: Option<u32>,
        /// Bypass the cache and force a fresh git status fetch
        #[arg(short = 'f', long, action = clap::ArgAction::SetTrue)]
        force: bool,
        /// Colour style
        #[arg(short = 's', long, default_value = "outline-bright")]
        style: StyleArg,
        /// Omit the trailing end-cap glyph (for use at the start of status-right)
        #[arg(long, action = clap::ArgAction::SetTrue)]
        no_cap: bool,
        /// Middle-ellipsize the branch name when longer than this. Without the
        /// flag, `[git] branch_max_len` in config.toml decides, 20 out of the box
        #[arg(long, value_name = "N")]
        branch_max_len: Option<usize>,
        /// Show the git glyph before the branch name (off by default)
        #[arg(long, action = clap::ArgAction::SetTrue)]
        branch_icon: bool,
        /// Seconds a cached status stays fresh (0 disables the cache)
        #[arg(long, default_value = "5")]
        ttl: f64,
        /// Compute it here instead of asking a daemon; no socket, no cache
        #[arg(long, action = clap::ArgAction::SetTrue)]
        no_daemon: bool,
        /// Write ANSI escapes instead of tmux markup, for use outside tmux
        #[arg(long, action = clap::ArgAction::SetTrue)]
        no_tmux: bool,
    },

    /// Whole right-hand status side in one call: git status, bandwidth and
    /// battery, computed concurrently and returned with the tmux literals that
    /// used to sit between them in the config.
    StatusRight {
        /// Path to the current pane's directory (defaults to current directory)
        path: Option<PathBuf>,
        /// Colour style
        #[arg(short = 's', long, default_value = "outline-bright")]
        style: StyleArg,
        /// Middle-ellipsize the branch name when longer than this. Without the
        /// flag, `[git] branch_max_len` in config.toml decides, 20 out of the box
        #[arg(long, value_name = "N")]
        branch_max_len: Option<usize>,
        /// Show the git glyph before the branch name (off by default)
        #[arg(long, action = clap::ArgAction::SetTrue)]
        branch_icon: bool,
        /// Bypass the git cache and force a fresh git status fetch
        #[arg(short = 'f', long, action = clap::ArgAction::SetTrue)]
        force: bool,
        /// Seconds a cached git status stays fresh (0 disables the cache).
        /// Applies to the git segment only -- bandwidth is always live.
        #[arg(long, default_value = "5")]
        ttl: f64,
    },

    /// Print sample git segments in every color style (local, no server)
    Preview,

    /// Battery status segment
    Battery,

    /// Network bandwidth segment
    Net {
        /// Compute it here instead of asking a daemon; the previous counter
        /// reading is kept in a file under the state directory
        #[arg(long, action = clap::ArgAction::SetTrue)]
        no_daemon: bool,
        /// Write ANSI escapes instead of tmux markup, for use outside tmux
        #[arg(long, action = clap::ArgAction::SetTrue)]
        no_tmux: bool,
    },

    /// Multi-client indicator segment
    Clients {
        /// #{session_attached}
        session_attached: u32,
        /// #{window_active_clients}
        window_active_clients: u32,
    },

    /// Jobs stopped or running under a pane
    ShJobs {
        /// Pane whose descendants to look through
        pane_pid: u32,
    },

    /// Background nvim indicator segment.
    ///
    /// Deprecated: this is `sh-jobs` with one hardcoded job. Kept for one
    /// release because it is in at least one tmux.conf, and hidden so a new
    /// reader is not offered two names for the same thing.
    #[command(hide = true)]
    VimBg {
        /// Pane whose descendants to look through
        pane_pid: u32,
    },

    /// Window status segment
    Window {
        /// This is the active window (#{window_active})
        #[arg(short = 'c', action = clap::ArgAction::SetTrue)]
        current: bool,
        /// Window index (#{window_index})
        #[arg(short = 'i')]
        index: u32,
        /// tmux window id (#{window_id})
        #[arg(short = 'I')]
        window_id: Option<String>,
        /// Window name (#{window_name})
        #[arg(short = 'n', default_value = "")]
        name: String,
        /// Active pane's directory (#{pane_current_path})
        #[arg(short = 'w')]
        path: Option<PathBuf>,
        /// Command running in the active pane (#{pane_current_command})
        #[arg(short = 'p', default_value = "")]
        process: String,
        /// Directory the window started in (#{pane_start_path})
        #[arg(short = 's')]
        start_path: Option<PathBuf>,
        /// Window flags (#{window_flags})
        #[arg(short = 'f', default_value = "")]
        flags: String,
        /// Index of the last window, so the row knows where it ends
        #[arg(short = 'l', default_value = "0")]
        last: u32,
        /// Number of panes in the window (#{window_panes})
        #[arg(short = 'P', default_value = "1")]
        pane_count: u32,
        /// Index of the window's active pane (#{pane_index})
        #[arg(short = 'A', default_value = "0")]
        pane_index: u32,
    },

    /// Bare client round trip with no server work, for benchmarking.
    #[command(hide = true)]
    Noop,

    /// Print what somebody would otherwise have to ask you for
    Doctor,

    /// What the tool offers, whether each is on for you, and a way to add what
    /// is missing
    Setup {
        /// Print the rows as tab-separated columns and exit, opening nothing
        #[arg(long)]
        print: bool,
    },

    /// Searchable key bindings
    #[command(args_conflicts_with_subcommands = true)]
    Keys {
        /// Find out what each layer binds, or where they collide
        #[command(subcommand)]
        action: Option<KeysAction>,
        /// Show every binding, including the ones tmux ships
        #[arg(long)]
        all: bool,
        /// Only bindings whose note or chord contains this
        #[arg(long, default_value = "companion: ")]
        query: String,
        /// Rebuild from tmux rather than using what the daemon holds
        #[arg(long)]
        refresh: bool,
        /// Print the rows and exit, instead of opening the picker
        #[arg(long)]
        print: bool,
        /// Only the bindings you wrote that the usage log has never seen
        /// pressed. Not with --all: tmux's own bindings are not yours to prune
        #[arg(long, conflicts_with = "all")]
        unused: bool,
    },

    /// Open a URL or file found in text
    Open {
        /// The text to scan. Reads stdin when there is none
        text: Vec<String>,
        /// Scan the tmux selection instead
        #[arg(short = 's')]
        selection: bool,
        /// Resolve relative paths against this directory
        #[arg(short = 'd')]
        base_dir: Option<String>,
        /// Print what would be opened, and open nothing
        #[arg(short = 'n', long)]
        dry_run: bool,
        /// The pane this is for, as `#{pane_id}` from the binding that ran it
        #[arg(long)]
        pane: Option<String>,
        /// The cursor column, as `#{copy_cursor_x}`, to pick what is under it
        #[arg(long)]
        cursor_x: Option<usize>,
        /// Pick which application opens it, from `[[open.application]]`
        #[arg(short = 'i', long)]
        choose: bool,
    },

    /// Close a project session by letting every window exit
    ///
    /// Deprecated: this is `project close`. Kept for one release because it is
    /// in at least one tmux.conf, and hidden so a new reader is not offered two
    /// names for the same thing.
    #[command(hide = true)]
    CloseProject {
        /// The session, defaulting to the current one
        session: Option<String>,
        /// Quit editors with :qa! and throw away unsaved work
        #[arg(long)]
        discard: bool,
        /// Close without capturing the layout, leaving any saved one alone
        #[arg(long)]
        no_save: bool,
    },

    /// Open a new window, here or at any directory, from the directory picker
    ///
    /// The picker lists the same directories `project` does. The query starts
    /// on the pane's own directory, so pressing the key and then enter is
    /// "another window here" and nothing has to be typed for the common case.
    /// A directory the source has never seen can be typed in full.
    NewWindow {
        /// Print the directories as tab-separated columns and exit, opening
        /// nothing
        #[arg(long)]
        print: bool,
    },

    /// Print the shell code that emits the OSC 133 prompt marks
    ///
    /// tmux's own next-prompt and previous-prompt do nothing until a shell
    /// says where a prompt begins, and this is the line that tells it.
    ShellInit {
        /// zsh, bash or fish, defaulting to $SHELL
        shell: Option<String>,
    },

    /// Copy to the system clipboard, whatever this platform calls it
    Clipboard {
        /// Read stdin rather than the tmux buffer
        #[arg(long)]
        stdin: bool,
    },

    /// Clear everything but the pane you are working in
    ///
    /// `zoom` is the old name and still works, for one more release.
    #[command(alias = "zoom")]
    Zen {
        /// The pane to keep, as `#{pane_id}` from the binding that ran it
        #[arg(long)]
        pane: Option<String>,
    },

    /// Ask the terminal what it does
    Probe {
        /// Which probe
        #[command(subcommand)]
        what: ProbeAction,
    },

    /// Run a command from history in a pane beside this one
    Run {
        /// List the history and exit, instead of opening the picker
        #[arg(long)]
        print: bool,
        /// The pane this is for, as `#{pane_id}` from the binding that ran it
        #[arg(long)]
        pane: Option<String>,
        /// Internal: run this command in this pane and show the exit dialog
        #[arg(long, hide = true)]
        exec: Option<String>,
        /// Internal: draw the exit dialog for this status and write the answer
        #[arg(long, hide = true)]
        dialog: Option<i32>,
        /// Internal: the file the dialog writes its answer to
        #[arg(long, hide = true)]
        out: Option<String>,
    },

    /// Move between windows or sessions in the order you used them, Cmd-Tab's
    /// shape: a press moves, a pause or enter goes, esc stays
    Switch {
        #[command(subcommand)]
        what: SwitchWhat,
    },

    /// Removed in favour of `switch windows`, which it now runs. Kept, hidden,
    /// for one release so a 0.9.1 binding on M-a keeps working until it is
    /// rebound; drop it in the release after
    #[command(hide = true)]
    Toggle {
        /// The session the binding passed
        session: Option<String>,
        /// Ignored, as it was in 0.9.1
        #[arg(hide = true)]
        window: Option<String>,
        /// Ignored: the first press of `switch windows` is already the flip
        #[arg(long, hide = true)]
        last: bool,
    },

    /// Deprecated: the `[autosave]` script timer, which `[sessions] autosave`
    /// replaces. With no flag it reports the last save, like --status
    Autosave {
        /// Run the save script now and exit
        #[arg(long)]
        once: bool,
        /// Print when the last save happened
        #[arg(long)]
        status: bool,
    },

    /// The way in: attach to what you were doing, or pick a project
    ///
    /// `tmux` on its own drops you in a session called `0` holding one bare
    /// shell, and everything this tool does is one keystroke further on from
    /// there. This is that keystroke, from outside tmux: it opens the same
    /// picker `M-s` opens, with live sessions first and every directory the
    /// source knows under them, and attaches to whatever is chosen.
    Start {
        /// Go straight to this directory instead of opening the picker
        dir: Option<String>,
        /// Attach to the session used most recently, without asking
        #[arg(long, short)]
        last: bool,
        /// For the `client-attached` hook in tmux.conf: open the picker only
        /// when this is a session tmux named itself with nothing happening in it
        #[arg(long)]
        hook: bool,
    },

    /// Switch to a project, or start one
    #[command(args_conflicts_with_subcommands = true)]
    Project {
        /// Save, forget or explain this project's layout
        #[command(subcommand)]
        action: Option<ProjectAction>,
        /// Go straight to this directory instead of opening the picker
        dir: Option<String>,
        /// Print the rows and exit, opening nothing
        #[arg(long, conflicts_with = "dir")]
        print: bool,
        /// With DIR: make its session and print the session's name, and
        /// switch to nothing and attach nothing. What a mirror session runs
        /// on its host before the attach
        #[arg(long, requires = "dir")]
        detached: bool,
    },

    /// Snapshots of every session, kept in generations
    Sessions {
        /// What to do
        #[command(subcommand)]
        action: SessionsAction,
    },

    /// Stop the tmux-companion daemon only; tmux and its sessions are left
    /// alone (`sessions shutdown` is the one that stops tmux)
    Shutdown,

    /// Restart the tmux-companion daemon only, so it rereads config.toml; tmux
    /// is left alone (`sessions restart` is the one that restarts tmux)
    Restart,

    /// Every pane on the server, with what it runs and whether it has gone
    /// quiet; enter jumps there
    Panes {
        /// Only the panes running one of `[agents] programs`
        #[arg(long)]
        agents: bool,
        /// Print the rows as tab-separated columns and exit, opening nothing
        #[arg(long)]
        print: bool,
        /// Only this session's panes
        #[arg(short = 't', long, value_name = "SESSION")]
        target: Option<String>,
    },

    /// Play the sound an event makes, now, whether sounds are on or not
    Earcon {
        /// asked, done or health
        event: crate::config::EarconEvent,
    },

    /// Type a few characters of anything on screen, then the label beside
    /// it: copy mode lands there, in whichever pane of the window it is
    Jump {
        /// The pane the binding was pressed in, as `#{pane_id}`
        #[arg(long, value_name = "ID")]
        pane: Option<String>,
        /// The client to open the overlay on, as `#{client_name}`
        #[arg(long, value_name = "NAME")]
        client: Option<String>,
        /// Be the overlay rather than open the popup it runs in
        #[arg(long, hide = true)]
        overlay: bool,
    },

    /// Every line of every pane's scrollback, newest first; enter goes to
    /// the pane and the line
    Search {
        /// Only the lines this matches: a regular expression, any case
        /// unless it holds a capital
        pattern: Option<String>,
        /// Take the pattern as text, not as a regular expression
        #[arg(short = 'F', long, requires = "pattern")]
        fixed: bool,
        /// A stored search in place of a pattern
        #[arg(short, long, value_enum, conflicts_with = "pattern")]
        kind: Option<crate::search::Kind>,
        /// Only this session's panes
        #[arg(short = 't', long, value_name = "SESSION")]
        target: Option<String>,
        /// Only this pane, as `#{pane_id}` from the binding
        #[arg(long, value_name = "ID")]
        pane: Option<String>,
        /// How many lines of each pane's history to read
        #[arg(long, value_name = "N", default_value_t = crate::search::DEFAULT_LINES)]
        lines: u32,
        /// Print the rows as tab-separated columns and exit, opening nothing
        #[arg(long)]
        print: bool,
        /// Go to the newest line that matches without showing the list
        #[arg(long, conflicts_with = "print")]
        first: bool,
    },

    /// The ports something is listening on and the pane that started each;
    /// enter jumps there
    Ports {
        /// The ports no pane started as well
        #[arg(long)]
        all: bool,
        /// UDP sockets that are bound and not connected as well
        #[arg(long)]
        udp: bool,
        /// Print the rows as tab-separated columns and exit, opening nothing
        #[arg(long)]
        print: bool,
        /// Only this session's panes
        #[arg(short = 't', long, value_name = "SESSION")]
        target: Option<String>,
        /// Enter stops the program listening, TERM and then KILL, where it
        /// would have gone to the pane
        #[arg(long, conflicts_with = "print")]
        kill: bool,
        /// With --kill, the seconds between TERM and KILL
        #[arg(long, value_name = "SECS", default_value_t = crate::kill::DEFAULT_GRACE_SECS)]
        grace: u64,
    },

    /// Give a pane a session of its own, named for the directory it is in
    Promote {
        /// The session's name, where the directory's would not do
        name: Option<String>,
        /// The pane, defaulting to the one the key was pressed in
        #[arg(long, value_name = "ID")]
        pane: Option<String>,
    },

    /// Stop what runs in front in a pane: TERM, and KILL if it is still
    /// there after the grace
    Kill {
        /// The pane, defaulting to the one the key was pressed in
        #[arg(long, value_name = "ID")]
        pane: Option<String>,
        /// The seconds between TERM and KILL
        #[arg(long, value_name = "SECS", default_value_t = crate::kill::DEFAULT_GRACE_SECS)]
        grace: u64,
        /// Confirm in tmux by name and pid first
        #[arg(long)]
        ask: bool,
    },

    /// What a mouse click on a segment of the bar does: bound to
    /// `MouseDown1StatusRight` with `#{mouse_status_range}`
    Click {
        /// The range name tmux hands the binding
        range: String,
    },

    /// What needs you, on one screen: the agents waiting and what they
    /// asked, the health reasons, the sessions idle for days, the numbers
    Brief {
        /// Print and exit without waiting for a key
        #[arg(long)]
        print: bool,
        /// For the `client-attached` hook: open the screen in a popup only
        /// when something is waiting or wrong, and say nothing otherwise
        #[arg(long)]
        hook: bool,
    },

    /// A shell you pull out beside this pane and put away, process and
    /// scrollback kept, by the same key
    Pocket {
        /// Which pocket, when a session keeps more than one
        name: Option<String>,
        /// The pane the key was pressed in, as `#{pane_id}` from the binding
        #[arg(long, value_name = "ID")]
        pane: Option<String>,
    },

    /// What an agent is doing, said by the agent itself from a hook: busy,
    /// asked or done; `hooks` prints the settings block that wires it up
    Agent {
        /// What to say, or `hooks`
        #[command(subcommand)]
        action: crate::agent::AgentAction,
    },

    /// The agents waiting on you, each with the question it asked; enter
    /// jumps there
    Inbox {
        /// Print the rows as tab-separated columns and exit, opening nothing
        #[arg(long)]
        print: bool,
    },

    /// What happened in each project: the commands that ran long, the
    /// questions the agents asked, the sessions opened and closed
    Journal {
        /// Print the rows as tab-separated columns and exit, opening nothing
        #[arg(long)]
        print: bool,
        /// Only this session's events
        #[arg(short = 't', long, value_name = "SESSION")]
        target: Option<String>,
        /// How many days back, default one
        #[arg(long, default_value_t = 1)]
        days: u64,
    },

    /// Quiet hours: no notifications, no nudges and no agent count on the
    /// bar for a while; the health mark says `quiet` instead
    Quiet {
        /// How long: `45m`, `2h`, `90s`, or a bare number of minutes;
        /// nothing asks how long is left
        duration: Option<String>,
        /// End quiet hours now; a Focus that is on stays on
        #[arg(long, conflicts_with = "duration")]
        off: bool,
        /// A Focus started: quiet until `--focus-off`, named on the health
        /// line. For a Shortcuts automation, or any hook, to call
        #[arg(
            long,
            value_name = "NAME",
            num_args = 0..=1,
            default_missing_value = crate::quiet::UNNAMED_FOCUS,
            conflicts_with_all = ["duration", "off", "focus_off"]
        )]
        focus: Option<String>,
        /// The Focus ended
        #[arg(long, conflicts_with_all = ["duration", "off"])]
        focus_off: bool,
    },

    /// The chunk clock: how long you've been at tmux this sitting, and
    /// how long until, or since, the break; with no action, `status`
    Chunk {
        /// status, snooze, reset, close or budget
        #[command(subcommand)]
        action: Option<ChunkAction>,
    },

    /// Why the health mark is up, a reason a line, or `ok`; `ack` forgets
    /// the failures once they've been read
    Health {
        /// `ack`, or nothing to only ask
        #[command(subcommand)]
        action: Option<HealthAction>,
    },

    /// Leave a one-line note on a pane, shown by `panes` and in the pane
    /// border; with nothing to say it prints the note that is there
    Note {
        /// The note; empty clears it
        text: Option<String>,
        /// The pane, defaulting to the one the key was pressed in
        #[arg(long, value_name = "ID")]
        pane: Option<String>,
        /// Take the note off
        #[arg(long, conflicts_with = "text")]
        clear: bool,
    },

    /// A cheat sheet of the bindings you keep looking up, in four boxes
    Cheatsheet {
        /// List the entries one per line, tab-separated, instead of drawing
        /// the sheet
        #[arg(long, alias = "plain")]
        print: bool,
    },

    /// Theme tools
    Theme {
        /// What to do
        #[command(subcommand)]
        action: ThemeAction,
    },

    /// Other machines' tmux-companion: each host, its link and its
    /// sessions, one line each; `add` adds one, `connect` brings one's link up
    Hosts {
        /// `add` or `connect`, or nothing to list
        #[command(subcommand)]
        action: Option<crate::hosts::HostsAction>,
    },

    /// One other machine's session, from this one: `host attach vm api` is
    /// what a mirror session's pane runs
    Host {
        #[command(subcommand)]
        action: crate::remote::HostAction,
    },

    /// Serve the link on stdin and stdout, for another machine's daemon:
    /// what a host command runs on this machine, not something to type
    Bridge {
        /// Serve only these methods, comma-separated, as an older bridge
        /// would; for tests
        #[arg(long, hide = true)]
        methods: Option<String>,
    },

    /// Inspect the configuration file
    Config {
        /// What to do with it
        #[command(subcommand)]
        action: ConfigAction,
    },
}

/// What `keys` can do besides open the bindings picker.
#[derive(Subcommand, Debug)]
#[command(rename_all = "kebab-case")]
pub enum KeysAction {
    /// Read what tmux and the apps in its panes bind, and save it for collide
    Discover {
        /// Only this layer: tmux or nvim
        #[arg(long, value_parser = clap::builder::PossibleValuesParser::new(crate::keyroute::discover::LAYERS))]
        layer: Option<String>,
        /// Print the rows as tab-separated columns as well as saving them
        #[arg(long)]
        print: bool,
    },
    /// Keys tmux's root table and an app both bind, and the keys nobody does
    Collide {
        /// Print the report as JSON instead of opening the picker
        #[arg(long)]
        json: bool,
        /// Discover again first rather than reading the last discovery
        #[arg(long)]
        refresh: bool,
    },
    /// Wrap tmux's root bindings so a key an app claims is held, or with
    /// [keys] route off, put them back; the last line of tmux.conf runs it
    // The text is clap's --help, where `[keys]` is the config section and not
    // a link, so rustdoc is told not to resolve it.
    #[allow(rustdoc::broken_intra_doc_links)]
    Route {
        /// Print the tmux script and what it would do, and apply nothing
        #[arg(long)]
        dry_run: bool,
        /// Say what was wrapped and what was left alone
        #[arg(long)]
        print: bool,
    },
    /// Tell tmux which keys the app in a pane wants, for a hook or a wrapper
    Claim {
        /// Name the app, for one whose process name doesn't say what it is
        #[arg(long)]
        app: Option<String>,
        /// Record the pane's current command as the claim's owner, so the
        /// claim stops counting once something else is in front
        #[arg(long)]
        owner: bool,
        /// The pane, `%3`; $TMUX_PANE when not given
        #[arg(short = 't', long)]
        pane: Option<String>,
        /// Say nothing and exit 0 when there is no pane or tmux refuses, for
        /// a hook that runs outside tmux as often as in it
        #[arg(long)]
        quiet: bool,
        /// Keys in tmux's spelling, M-a, C-Tab, F5
        keys: Vec<String>,
    },
    /// Take back every claim on a pane
    Release {
        /// The pane, `%3`; $TMUX_PANE when not given
        #[arg(short = 't', long)]
        pane: Option<String>,
        /// Say nothing and exit 0 when there is no pane or tmux refuses
        #[arg(long)]
        quiet: bool,
    },
}

/// The two probes.
#[derive(Subcommand, Debug)]
#[command(rename_all = "kebab-case")]
pub enum ProbeAction {
    /// Show the exact bytes the terminal sends for a key
    Keys {
        /// Stop after this many keys
        #[arg(short = 'n')]
        count: Option<usize>,
    },
    /// Ask how many cells the terminal advances for a string
    Cells {
        /// The strings to measure, or a built-in set
        strings: Vec<String>,
    },
}

/// What `theme` can do.
#[derive(Subcommand, Debug)]
#[command(rename_all = "kebab-case")]
pub enum ThemeAction {
    /// Pick a theme and apply it
    Pick {
        /// Apply to this target rather than whatever is current
        #[arg(short = 't')]
        target: Option<String>,
        /// Remember the pick for a session by name instead of applying it,
        /// for a session that does not exist yet
        #[arg(short = 'r', value_name = "SESSION")]
        register: Option<String>,
        /// Where the theme files are
        #[arg(long)]
        themes: Option<String>,
        /// List the themes and exit, instead of opening the picker
        #[arg(long)]
        print: bool,
    },

    /// Apply the theme a session should have, without asking
    Apply {
        /// The session to resolve a theme for. Omitted with --all
        session: Option<String>,
        /// Apply to this target rather than whatever is current
        #[arg(short = 't')]
        target: Option<String>,
        /// Where the theme files are
        #[arg(long)]
        themes: Option<String>,
        /// Repaint every session rather than one
        #[arg(long)]
        all: bool,
    },

    /// Write the starter themes and the two files that apply them
    ///
    /// Nothing is overwritten. Run `theme gen --apply --shades` afterwards to
    /// mint a lighter and a darker sibling of each and to measure the borders
    /// against this terminal's background.
    Init {
        /// Where the files go
        #[arg(long)]
        themes: Option<String>,
    },

    /// Write a theme from a background colour, and optionally a text colour
    ///
    /// One colour is enough: the text colour is computed from it, and
    /// `theme gen --apply` adds the border afterwards. Give `--fg` to choose
    /// the text colour yourself.
    Add {
        /// The block's colour: a name, colourN, or #rrggbb
        #[arg(long)]
        bg: String,
        /// The text on it. Computed from the background when left out
        #[arg(long)]
        fg: Option<String>,
        /// What to call it. Taken from the colour when left out
        #[arg(long)]
        name: Option<String>,
        /// Write it even when the pair is under WCAG AA
        #[arg(long)]
        force: bool,
        /// Where the file goes
        #[arg(long)]
        themes: Option<String>,
    },

    /// Print every colour tmux takes, with a swatch
    #[command(alias = "list-colors", alias = "list-all-colors")]
    ListColours {
        /// One per line with no swatch, for piping somewhere
        #[arg(long, alias = "plain")]
        print: bool,
    },

    /// Compute each theme's readable text colour and a visible border
    Gen {
        /// Write the files. Without this, report what would change and touch
        /// nothing
        #[arg(long)]
        apply: bool,
        /// Which colours to generate, as a contrast rung: aa, aaa, a4, a5, a6.
        /// Defaults to aaa when the flag is given with no value
        #[arg(long, num_args = 0..=1, default_missing_value = "aaa")]
        shades: Option<String>,
        /// Where the theme files are
        #[arg(long)]
        themes: Option<String>,
        /// The terminal background to measure borders against, as #rrggbb.
        /// Without it, `ghostty +show-config` is asked, and when ghostty is not
        /// installed the xterm default of colour232 stands in
        #[arg(long)]
        background: Option<String>,
    },
}

/// The three questions anybody asks about a config file.
#[derive(Subcommand, Debug)]
#[command(rename_all = "kebab-case")]
pub enum ConfigAction {
    /// Print which file is being read, and nothing else
    Path,
    /// Parse the file and report what is wrong with it
    Check {
        /// Check this file instead of searching the usual places
        path: Option<PathBuf>,
    },
    /// Print every setting with its default, as a config file
    Dump,
    /// Write a short starter config where `config path` would read it
    Init {
        /// Overwrite a file that is already there
        #[arg(long)]
        force: bool,
    },
}

/// `--style`, as clap checks it.
///
/// A value enum rather than a string, so a typo is refused with the list of
/// values rather than quietly drawn in the default style, which is what
/// `Style::parse(..).unwrap_or_default()` did and what made `--style bogus`
/// look like a working flag. The wire type stays `Style` in `tmux::format`;
/// this is the same three names with clap's derive on them.
#[derive(clap::ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
#[value(rename_all = "kebab-case")]
pub enum StyleArg {
    /// Solid state-coloured background
    Fill,
    /// State colour in the foreground, the bar colour behind
    Outline,
    /// Outline with the icon colours lightened for a dark bar
    #[value(alias = "bright")]
    OutlineBright,
}

impl From<StyleArg> for crate::tmux::format::Style {
    fn from(s: StyleArg) -> Self {
        use crate::tmux::format::Style;
        match s {
            StyleArg::Fill => Style::Fill,
            StyleArg::Outline => Style::Outline,
            StyleArg::OutlineBright => Style::OutlineBright,
        }
    }
}

/// Run one subcommand: start the server, or send one request and print it.
pub async fn run(command: Cmd) -> anyhow::Result<()> {
    match command {
        Cmd::Server => {
            crate::server::run().await?;
        }
        Cmd::Gst {
            path,
            pane_pid,
            force,
            style,
            no_cap,
            branch_max_len,
            branch_icon,
            ttl,
            no_daemon,
            no_tmux,
        } => {
            if no_daemon {
                let config = local_config();
                let opts = crate::segments::git::GstOptions {
                    path,
                    pane_pid,
                    force,
                    style: style.into(),
                    no_cap,
                    // The flag wins, and the config decides without it, the
                    // same rule the daemon applies in `handlers::gst`.
                    branch_max_len: branch_max_len.or(Some(config.git.branch_max_len)),
                    branch_icon,
                    ttl: std::time::Duration::ZERO,
                    parts: config.git.parts.clone(),
                    bar_bg: config.bar.background.clone(),
                };
                print_segment(crate::local::gst(&opts, &config).await?, no_tmux);
                return Ok(());
            }
            let args = GstArgs {
                path,
                pane_pid,
                force,
                style: style.into(),
                no_cap,
                branch_max_len,
                branch_icon,
                ttl_secs: ttl,
            };
            send_segment(Request::build("gst", &args), no_tmux).await?;
        }
        Cmd::StatusRight {
            path,
            style,
            branch_max_len,
            branch_icon,
            force,
            ttl,
        } => {
            let args = StatusRightArgs {
                path,
                style: style.into(),
                branch_max_len,
                branch_icon,
                force,
                ttl_secs: ttl,
            };
            crate::client::send_and_print(Request::build("status-right", &args)).await?;
        }
        Cmd::Preview => {
            print!("{}", crate::preview::render());
        }
        Cmd::Battery => {
            crate::client::send_and_print(Request::build("battery", &())).await?;
        }
        Cmd::Net { no_daemon, no_tmux } => {
            if no_daemon {
                let config = local_config();
                print_segment(crate::local::net(&config).await?, no_tmux);
                return Ok(());
            }
            send_segment(Request::build("net", &()), no_tmux).await?;
        }
        Cmd::Clients {
            session_attached,
            window_active_clients,
        } => {
            let args = ClientsArgs {
                session_attached,
                window_active_clients,
            };
            crate::client::send_and_print(Request::build("clients", &args)).await?;
        }
        Cmd::ShJobs { pane_pid } => {
            crate::client::send_and_print(Request::build("sh-jobs", &ShJobsArgs { pane_pid }))
                .await?;
        }
        Cmd::VimBg { pane_pid } => {
            eprintln!("tmux-companion: `vim-bg` is now `sh-jobs`; the old name works for now");
            crate::client::send_and_print(Request::build("sh-jobs", &ShJobsArgs { pane_pid }))
                .await?;
        }
        Cmd::Window {
            current,
            index,
            window_id,
            name,
            path,
            process,
            start_path,
            flags,
            last,
            pane_count,
            pane_index,
        } => {
            let args = crate::segments::window::WindowArgs {
                current,
                index,
                window_id,
                name: (!name.is_empty()).then_some(name),
                path,
                process: (!process.is_empty()).then_some(process),
                start_path,
                // The old `json!` block sent this as a bare string, so an
                // empty `--flags` arrived as `Some("")` rather than `None`.
                // Kept exactly, because the window renderer distinguishes them.
                flags: Some(flags),
                last,
                pane_count,
                pane_index,
            };
            crate::client::send_and_print(Request::build("window", &args)).await?;
        }
        Cmd::Noop => {
            crate::client::send_and_print(Request::build("noop", &())).await?;
        }
        // Answered in this process rather than by the daemon: the daemon holds
        // the config it started with, and the question here is what a *fresh*
        // read of the file says, which is what somebody debugging one wants.
        Cmd::Config { action } => run_config(action)?,
        Cmd::Keys {
            action: Some(action),
            ..
        } => run_keys_action(action).await?,
        Cmd::Keys {
            action: None,
            all,
            query,
            refresh,
            print,
            unused,
        } => run_keys(all, query, refresh, print, unused).await?,
        Cmd::Open {
            text,
            selection,
            base_dir,
            dry_run,
            pane,
            cursor_x,
            choose,
        } => run_open(text, selection, base_dir, dry_run, pane, cursor_x, choose).await?,
        Cmd::CloseProject {
            session,
            discard,
            no_save,
        } => {
            eprintln!(
                "tmux-companion: `close-project` is now `project close`; the old name works for now"
            );
            run_close_project(session, discard, !no_save).await?
        }
        Cmd::NewWindow { print } => run_new_window(print).await?,
        Cmd::ShellInit { shell } => run_shell_init(shell)?,
        Cmd::Clipboard { stdin } => run_clipboard(stdin).await?,
        Cmd::Zen { pane } => run_zen(pane).await?,
        Cmd::Probe { what } => run_probe(what)?,
        Cmd::Run {
            print,
            pane,
            exec,
            dialog,
            out,
        } => match dialog {
            Some(code) => run_dialog(code, out)?,
            None => run_command(print, pane, exec).await?,
        },
        Cmd::Switch { what } => match what {
            SwitchWhat::Windows {
                session,
                reverse,
                client,
                print,
                pressed,
            } => run_switch_windows(session, client, reverse, print, pressed).await?,
            SwitchWhat::Sessions { reverse, print, at } => {
                run_switch_sessions(reverse, print, at).await?
            }
        },
        // Silent on purpose: from run-shell anything printed opens a view
        // over the pane on every press. The CHANGELOG says to rebind.
        Cmd::Toggle { session, .. } => {
            run_switch_windows(session, None, false, false, false).await?
        }
        Cmd::Autosave { once, status } => run_autosave(once, status).await?,
        Cmd::Start { dir, last, hook } => run_start(dir, last, hook).await?,
        Cmd::Project {
            action,
            dir,
            print,
            detached,
        } => match action {
            Some(ProjectAction::Save { no_commands }) => run_project_save(!no_commands).await?,
            Some(ProjectAction::Forget { dir }) => run_project_forget(dir).await?,
            Some(ProjectAction::Show { dir }) => run_project_show(dir).await?,
            Some(ProjectAction::Close {
                session,
                discard,
                no_save,
            }) => run_close_project(session, discard, !no_save).await?,
            None => run_project(dir, print, detached).await?,
        },
        Cmd::Sessions { action } => match action {
            SessionsAction::Export { file, stamp } => {
                crate::sessions::portable::export(stamp, file).await?
            }
            SessionsAction::Import { file, no_map } => {
                crate::sessions::portable::import(file, no_map).await?
            }
            SessionsAction::Save {
                skip_pane_history,
                exclude,
            } => run_sessions_save(skip_pane_history, &exclude, false).await?,
            SessionsAction::Resurrect {
                stamp,
                only,
                exclude,
                merge,
                dry_run,
                yes,
                detach,
            } => {
                let code = run_sessions_resurrect(ResurrectOptions {
                    stamp,
                    only,
                    exclude,
                    merge,
                    dry_run,
                    yes,
                    detach,
                    hand_back: false,
                })
                .await;
                if code != 0 {
                    std::process::exit(code);
                }
            }
            SessionsAction::Shutdown {
                exclude,
                daemon_too,
                dry_run,
            } => {
                let code = run_lifecycle(Lifecycle {
                    restart: false,
                    exclude,
                    daemon: daemon_too,
                    dry_run,
                })
                .await;
                if code != 0 {
                    std::process::exit(code);
                }
            }
            SessionsAction::Restart {
                exclude,
                keep_daemon,
                dry_run,
            } => {
                let code = run_lifecycle(Lifecycle {
                    restart: true,
                    exclude,
                    // Bouncing the daemon is the default here, because
                    // config.toml is read once at its start and a restart that
                    // left it running would hand back a new binary with
                    // yesterday's configuration.
                    daemon: !keep_daemon,
                    dry_run,
                })
                .await;
                if code != 0 {
                    std::process::exit(code);
                }
            }
            SessionsAction::Autosave { once, status } => {
                run_sessions_autosave(once, status).await?
            }
            SessionsAction::Start { name, dir, detach } => {
                let code = crate::sessions::start::run(&name, dir, detach).await;
                if code != 0 {
                    std::process::exit(code);
                }
            }
            SessionsAction::Continue { last, print } => {
                let code = crate::sessions::start::run_continue(last, print).await;
                if code != 0 {
                    std::process::exit(code);
                }
            }
            SessionsAction::List { json } => run_sessions_list(json)?,
            SessionsAction::Rename { new, session } => {
                match crate::link::mirror::rename_command(session, &new).await {
                    Ok(said) => println!("{said}"),
                    Err(e) => {
                        eprintln!("{e:#}");
                        std::process::exit(1);
                    }
                }
            }
            SessionsAction::Show { stamp, json } => run_sessions_show(stamp, json)?,
            SessionsAction::Idle { days, print } => {
                // The picker answers with a name and the close is the same
                // `project close`, layout capture included, so a session shut
                // from here comes back from the project picker as it was.
                if let Some(name) = crate::sessions::idle::run(days, print).await? {
                    run_close_project(Some(name), false, true).await?;
                }
            }
        },
        Cmd::Shutdown => run_daemon_shutdown().await?,
        Cmd::Restart => run_daemon_restart().await?,
        // No `Request::build` and no args struct in proto.rs: this one reads
        // tmux and draws in this terminal, and the daemon has neither the
        // list nor the screen. See `panes::run`.
        Cmd::Panes {
            agents,
            print,
            target,
        } => crate::panes::run(agents, print, target).await?,
        Cmd::Earcon { event } => crate::earcons::run(event)?,
        // Client-side for the same reasons as `panes`: see `jump::run`.
        Cmd::Jump {
            pane,
            client,
            overlay,
        } => crate::jump::run(pane, client, overlay).await?,
        Cmd::Search {
            pattern,
            fixed,
            kind,
            target,
            pane,
            lines,
            print,
            first,
        } => {
            crate::search::run(crate::search::Asked {
                pattern,
                fixed,
                kind,
                target,
                pane,
                lines,
                print,
                first,
            })
            .await?
        }
        Cmd::Ports {
            all,
            udp,
            print,
            target,
            kill,
            grace,
        } => {
            let stop = kill.then(|| std::time::Duration::from_secs(grace));
            crate::ports::run(all, udp, print, target, stop).await?
        }
        Cmd::Kill { pane, grace, ask } => crate::kill::run(pane, grace, ask).await?,
        Cmd::Promote { name, pane } => crate::promote::run(name, pane).await?,
        Cmd::Click { range } => crate::click::run(range).await?,
        Cmd::Brief { print, hook } => crate::brief::run(print, hook).await?,
        Cmd::Pocket { name, pane } => crate::pocket::run(name, pane).await?,
        Cmd::Agent { action } => crate::agent::run(action).await?,
        Cmd::Inbox { print } => crate::inbox::run(print).await?,
        Cmd::Journal {
            print,
            target,
            days,
        } => crate::journal::run(print, target, days).await?,
        Cmd::Quiet {
            duration,
            off,
            focus,
            focus_off,
        } => {
            let change = match (focus, focus_off) {
                (Some(name), _) => Some(crate::quiet::FocusChange::On(name)),
                (None, true) => Some(crate::quiet::FocusChange::Off),
                (None, false) => None,
            };
            crate::quiet::run(duration, off, change).await?
        }
        Cmd::Chunk { action } => crate::chunk::run(action).await?,
        Cmd::Health { action } => run_health(action.is_some()).await?,
        Cmd::Note { text, pane, clear } => crate::note::run(text, pane, clear).await?,
        Cmd::Cheatsheet { print } => run_cheatsheet(print).await?,
        Cmd::Doctor => crate::doctor::run().await?,
        Cmd::Hosts { action } => crate::hosts::run(action).await?,
        Cmd::Host { action } => crate::remote::run(action).await?,
        Cmd::Bridge { methods } => crate::link::bridge::run(methods).await?,
        // No `Request::build` and no args struct in proto.rs: the checklist
        // reads tmux and the config itself and draws in this terminal. The
        // daemon is asked one raw question, `__setup`, for what only it saw.
        Cmd::Setup { print } => crate::setup::run(print).await?,
        Cmd::Theme { action } => crate::theme::cli::run(action)?,
    }

    Ok(())
}

/// The config for a `--no-daemon` run.
///
/// A broken config is a warning on stderr and the defaults, not a failure: the
/// no-daemon path is what somebody's shell prompt calls, and a prompt that
/// stops printing because a TOML key was misspelled is worse than a prompt
/// drawn with the defaults. `config check` is where the error is meant to be
/// read.
fn local_config() -> crate::config::Config {
    match crate::config::load() {
        Ok((c, _)) => c,
        Err(e) => {
            eprintln!("tmux-companion: {e}");
            crate::config::Config::default()
        }
    }
}

/// The config for every other client-side command, or the defaults, said once.
///
/// This replaces a bare `unwrap_or_default()` at every site that reads the
/// config on the client side, because that was silent: a misspelled key in
/// config.toml put every picker back on its defaults and nothing said so, and
/// the person went looking in the daemon, which had refused the file and said
/// so in its own log. One line on stderr is enough to send them to
/// `config check`; once per process, because a command reads the config more
/// than once and a keypress that prints the same warning three times reads
/// as three problems. Still the defaults rather than a failure, for the same
/// reason as [`local_config`]: a key somebody already pressed should do
/// something.
pub(crate) fn config_or_default() -> crate::config::Config {
    if CACHE_CONFIG.load(std::sync::atomic::Ordering::Relaxed) {
        static ONCE: std::sync::OnceLock<crate::config::Config> = std::sync::OnceLock::new();
        return ONCE.get_or_init(config_or_default_now).clone();
    }
    config_or_default_now()
}

/// Whether [`config_or_default`] reads the file once for the whole process.
///
/// Set by a client, which lives for one command and read the same file two
/// and three times on the way to a picker's first frame (the command, the
/// paint, the picker's look). Never set in the daemon: its loops call
/// `config_or_default` on every tick precisely so an edit takes effect
/// without a restart.
static CACHE_CONFIG: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Read the configuration once for the rest of this process; see
/// `config_or_default`.
pub fn cache_config_for_this_process() {
    CACHE_CONFIG.store(true, std::sync::atomic::Ordering::Relaxed);
}

fn config_or_default_now() -> crate::config::Config {
    static SAID: std::sync::Once = std::sync::Once::new();
    match crate::config::load() {
        Ok((c, _)) => c,
        Err(_) => {
            SAID.call_once(|| {
                eprintln!(
                    "tmux-companion: config.toml does not parse, using defaults; \
                     run tmux-companion config check"
                );
            });
            crate::config::Config::default()
        }
    }
}

/// Print a rendered segment, translated out of tmux markup when asked.
///
/// Nothing is printed for an empty segment, not even a newline, because these
/// are written into a prompt or a bar where a stray line is visible.
fn print_segment(rendered: String, no_tmux: bool) {
    if no_tmux {
        print!("{}", crate::tmux::format::to_ansi(&rendered));
    } else {
        print!("{rendered}");
    }
}

/// Ask the daemon and print what comes back, through the same translation.
///
/// A daemon error is returned rather than printed, so the process exits 1 and
/// a prompt or a script can tell a failed segment from an empty one. tmux
/// ignores the exit status of a `#()`, so a status bar sees no difference.
async fn send_segment(req: crate::proto::Request, no_tmux: bool) -> anyhow::Result<()> {
    if !no_tmux {
        return crate::client::send_and_print(req).await;
    }
    let resp = crate::client::send(req).await?;
    if let Some(err) = resp.error {
        anyhow::bail!("{err}");
    }
    print_segment(resp.output, true);
    Ok(())
}

/// `config path`, `config check` and `config dump`.
fn run_config(action: ConfigAction) -> anyhow::Result<()> {
    use crate::config;

    match action {
        ConfigAction::Path => {
            let (_, source) = config::load().unwrap_or_else(|e| {
                // Even a broken file is the file being read, which is the
                // question that was asked.
                (config::Config::default(), config::Source::File(e.path))
            });
            println!("{source}");
        }
        ConfigAction::Check { path } => {
            let result = match &path {
                Some(p) => config::load_from(p),
                None => config::load(),
            };
            match result {
                Ok((config, source)) => {
                    println!("{source}: ok");
                    // keys.toml was read and merged by the load above; saying
                    // so is the only way to know it was found at all.
                    let config_file = match &source {
                        config::Source::File(p) => Some(p.as_path()),
                        _ => None,
                    };
                    if let Some(keys) = config::keys_file_for(config_file).filter(|k| k.exists()) {
                        println!("{}: ok", keys.display());
                    }
                    for note in config::deprecations(&config) {
                        println!("  {note}");
                    }
                }
                Err(e) => {
                    eprintln!("{e}");
                    std::process::exit(1);
                }
            }
            // The checkout's own file, when this runs inside one: the second
            // input `project` reads, checked the same way so a bad key is
            // found here rather than by opening the project.
            let cwd = std::env::current_dir()
                .map(|p| p.display().to_string())
                .unwrap_or_default();
            match crate::repofile::load(&cwd) {
                crate::repofile::RepoFile::Absent => {}
                crate::repofile::RepoFile::Layout { file, .. } => {
                    println!("{}: ok", file.display());
                }
                crate::repofile::RepoFile::Ignored { file, why } => {
                    eprintln!("{}: {why}", file.display());
                    std::process::exit(1);
                }
            }
        }
        ConfigAction::Dump => print!("{}", config::dump_defaults()),
        ConfigAction::Init { force } => {
            // The first path in the search order, which is what `config path`
            // reports once the file exists: `TMUX_COMPANION_CONFIG` when set,
            // else the XDG one. A file already there is somebody's settings,
            // and a starter written over them is a worse start than a refusal.
            let Some(path) = config::search_paths().into_iter().next() else {
                anyhow::bail!("nowhere to write: neither XDG_CONFIG_HOME nor HOME is set");
            };
            if path.exists() && !force {
                anyhow::bail!("{} is already there; --force overwrites it", path.display());
            }
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            std::fs::write(&path, config::STARTER)?;
            println!("{}", path.display());
        }
    }
    Ok(())
}

/// `keys`: fetch the rows, pick one, run it.
///
/// The daemon holds the rows and the picker runs here, because a daemon has no
/// terminal. `--print` skips the picker entirely, which is what a script wants
/// and what makes the whole path testable without a tty.
///
/// `--unused` is the same picker over the rows the usage log has never seen,
/// so `run_keys_unused` below is the one place the two paths part.
async fn run_keys(
    all: bool,
    query: String,
    refresh: bool,
    print: bool,
    unused: bool,
) -> anyhow::Result<()> {
    use crate::keys::KeyRow;

    let args = crate::proto::KeysArgs {
        // The picker does its own matching, so the daemon is asked for
        // everything and the opening query is applied here. That is what makes
        // ctrl-a able to widen beyond what was fetched.
        query: String::new(),
        refresh,
    };
    let resp = crate::client::send(Request::build("keys", &args)).await?;
    if let Some(e) = resp.error {
        anyhow::bail!(e);
    }

    let rows: Vec<KeyRow> = resp
        .output
        .lines()
        .filter_map(|line| {
            let mut f = line.splitn(5, '\t');
            Some(KeyRow {
                table: f.next()?.to_string(),
                key: f.next()?.to_string(),
                shown: f.next()?.to_string(),
                note: f.next()?.to_string(),
                command: f.next()?.to_string(),
            })
        })
        .collect();

    let opening = if all { String::new() } else { query };

    // `--all` shows tmux's own bindings too, so there is something to draw
    // even when none of them carries the note.
    if !all && nothing_noted(&rows, print).await {
        return Ok(());
    }

    // A broken config should not swallow a keypress somebody already made, so
    // the defaults stand in here rather than the pick being dropped.
    let config = config_or_default();

    if unused {
        return run_keys_unused(&rows, &opening, print, &config).await;
    }

    if print {
        for row in crate::keys::filter(&rows, &opening) {
            println!(
                "{}\t{}\t{}\t{}\t{}",
                row.table, row.key, row.shown, row.note, row.command
            );
        }
        return Ok(());
    }

    let chrome = crate::picker::Chrome {
        title: "[ Keys ]".into(),
        icon: crate::tmux::icons::KEY.into(),
        footer: "enter run   ctrl-a show all   esc cancel".into(),
        preview_title: "[ What it runs ]".into(),
        ..Default::default()
    }
    .configured(&config.picker, crate::config::Picker::Keys);

    let Some(index) = crate::picker::run(key_items(rows.iter()), &opening, &chrome)? else {
        return Ok(());
    };
    let Some(row) = rows.get(index) else {
        return Ok(());
    };
    run_binding(row, &config).await
}

/// `keys --unused`: the bindings somebody wrote that the log has never seen.
///
/// The scope is what the opening query selects, `companion: ` unless `--query`
/// said otherwise, because a binding tmux ships is not one anybody wrote and
/// so not one anybody would prune. That is also why the picker opens with no
/// query and no ctrl-a hint: the rows it holds are already the whole answer.
///
/// The log has no dates, so "never" means since the log began, and the line
/// under the list says how many presses that is so a reader can weigh it. A
/// fresh machine lists every binding with `no usage log yet` beside it, which
/// is the truth rather than an error.
async fn run_keys_unused(
    rows: &[crate::keys::KeyRow],
    opening: &str,
    print: bool,
    config: &crate::config::Config,
) -> anyhow::Result<()> {
    let in_scope = crate::keys::filter(rows, opening);
    let counts = std::fs::read_to_string(usage_path(config))
        .ok()
        .map(|t| crate::keys::usage_counts(&t));
    let never = crate::keys::unused(
        in_scope.iter().copied(),
        counts.as_ref().unwrap_or(&std::collections::HashMap::new()),
    );
    let summary = crate::keys::unused_summary(
        never.len(),
        in_scope.len(),
        counts.as_ref().map(|c| c.values().sum()),
    );

    if print {
        // The summary goes to stderr so stdout stays one row per line for
        // whatever is parsing it, the same shape `keys --print` gives.
        eprintln!("{summary}");
        for row in &never {
            println!("{}\t{}\t{}", row.table, row.key, row.note);
        }
        return Ok(());
    }

    // The layout comes from the config like the ordinary keys picker, but the
    // title and footer are set after, because a `[picker.keys]` label or hint
    // written for that picker would otherwise replace the one line here that
    // says what this list is and how much the log knows.
    let mut chrome = crate::picker::Chrome {
        preview_title: "[ What it runs ]".into(),
        icon: crate::tmux::icons::KEY.into(),
        ..Default::default()
    }
    .configured(&config.picker, crate::config::Picker::Keys);
    chrome.title = "[ Keys never used ]".into();
    chrome.footer = format!("enter run   esc cancel   {summary}");

    let Some(index) = crate::picker::run(key_items(never.iter().copied()), "", &chrome)? else {
        return Ok(());
    };
    let Some(row) = never.get(index) else {
        return Ok(());
    };
    run_binding(row, config).await
}

/// `keys discover` and `keys collide`. Both run here in the client: they
/// start or connect to editors, which the daemon never does.
async fn run_keys_action(action: KeysAction) -> anyhow::Result<()> {
    use crate::keyroute::discover;

    let config = config_or_default();
    let path = discover::path()
        .ok_or_else(|| anyhow::anyhow!("no state dir: neither XDG_STATE_HOME nor HOME is set"))?;
    let fresh = |layer: Option<String>| {
        let apps = config.keys.app.clone();
        let path = path.clone();
        async move {
            let found = discover::discover(layer.as_deref(), &apps).await;
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            std::fs::write(&path, serde_json::to_string_pretty(&found)?)?;
            anyhow::Ok(found)
        }
    };

    match action {
        KeysAction::Route { dry_run, print } => {
            return run_keys_route(&config, &path, dry_run, print).await;
        }
        KeysAction::Claim {
            app,
            owner,
            pane,
            quiet,
            keys,
        } => {
            let done = run_keys_claim(app, owner, pane, keys).await;
            return if quiet { Ok(()) } else { done };
        }
        KeysAction::Release { pane, quiet } => {
            let done = run_keys_release(pane).await;
            return if quiet { Ok(()) } else { done };
        }
        KeysAction::Discover { layer, print } => {
            let found = fresh(layer).await?;
            if print {
                for r in &found.rows {
                    println!(
                        "{}\t{}\t{}\t{}\t{}\t{}\t{}",
                        r.layer,
                        r.mode,
                        r.key,
                        r.rung.as_str(),
                        r.pane,
                        r.desc,
                        r.source
                    );
                }
            }
            for note in &found.notes {
                eprintln!("{note}");
            }
            eprintln!("{} rows written to {}", found.rows.len(), path.display());
        }
        KeysAction::Collide { json, refresh } => {
            let found = match (refresh, discover::load(&path)) {
                (false, Some(found)) => found,
                _ => fresh(None).await?,
            };
            let report = crate::keyroute::collide::report(&found, &config.keys.key);
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
                return Ok(());
            }
            // The layout comes from the keys picker's config, the words and
            // the column order do not: a `[picker.keys]` hint says enter runs
            // a binding, and its `column_order` is written for that picker's
            // two columns, so it would drop two of these four.
            let mut chrome = crate::picker::Chrome {
                icon: crate::tmux::icons::KEY.into(),
                preview_title: "[ Who wants it ]".into(),
                ..Default::default()
            }
            .configured(&config.picker, crate::config::Picker::Keys);
            chrome.look.column_order = Vec::new();
            chrome.title = "[ Key collisions ]".into();
            chrome.preview_title = "[ Who wants it ]".into();
            chrome.footer = format!(
                "esc close   {} collisions   {} drift   {} free",
                report.collisions.len(),
                report.drift.len(),
                report.free.len()
            );
            crate::picker::run(collide_items(&report), "", &chrome)?;
        }
    }
    Ok(())
}

/// `keys route`: read the live root table and wrap it, rewrap it or put it
/// back, by one sourced script.
///
/// Quiet on success without `--print`, because tmux.conf runs it on every
/// source and `run-shell` puts whatever it prints in front of you.
async fn run_keys_route(
    config: &crate::config::Config,
    discovered: &std::path::Path,
    dry_run: bool,
    print: bool,
) -> anyhow::Result<()> {
    use crate::keyroute::route;

    let version = crate::tmux::command().arg("-V").output().await?;
    let version = route::tmux_version(&String::from_utf8_lossy(&version.stdout));
    let mut enabled = config.keys.route;
    if enabled && version.is_none_or(|v| v < route::MIN_TMUX) {
        eprintln!(
            "keys route: [keys] route is on but this tmux is older than {}.{}, which send-keys -K needs; nothing is wrapped",
            route::MIN_TMUX.0,
            route::MIN_TMUX.1
        );
        enabled = false;
    }

    let listing = crate::tmux::command().arg("list-keys").output().await?;
    let listing = route::parse_listing(&String::from_utf8_lossy(&listing.stdout));
    let notes = crate::tmux::command()
        .args(["list-keys", "-a", "-N", "-T", "root"])
        .output()
        .await?;
    let notes = route::root_notes(&String::from_utf8_lossy(&notes.stdout), &listing);
    let rows = crate::keyroute::discover::load(discovered)
        .map(|d| d.rows)
        .unwrap_or_default();
    let claims = route::static_claims(&rows, &config.keys.app);
    let plan = route::plan(
        &listing,
        &notes,
        enabled,
        config.keys.hold_ms,
        &claims,
        &config.keys.key,
    );

    if dry_run {
        print!("{}", plan.script);
    }
    if dry_run || print {
        if !plan.wrapped.is_empty() {
            println!(
                "wrapped, hold {} ms: {}",
                config.keys.hold_ms,
                plan.wrapped.join(" ")
            );
        }
        if !plan.unwrapped.is_empty() {
            println!("put back: {}", plan.unwrapped.join(" "));
        }
        for (key, why) in &plan.skipped {
            println!("left alone: {key}, {why}");
        }
    }
    if dry_run || plan.script.is_empty() {
        return Ok(());
    }

    // Kept, not a temporary file: when a wrapper misbehaves, the script that
    // made it is the first thing to read.
    let script = discovered.with_file_name("keys-route.tmux");
    if let Some(dir) = script.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&script, &plan.script)?;
    let out = crate::tmux::command()
        .arg("source-file")
        .arg(&script)
        .output()
        .await?;
    if !out.status.success() {
        anyhow::bail!(
            "tmux refused {}: {}",
            script.display(),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(())
}

/// The pane a claim is about: the flag, else the pane this runs in.
fn claim_pane(pane: Option<String>) -> anyhow::Result<String> {
    pane.or_else(|| std::env::var("TMUX_PANE").ok())
        .filter(|p| !p.is_empty())
        .ok_or_else(|| anyhow::anyhow!("no pane: pass -t, or run this inside tmux"))
}

/// `keys claim`: set the pane options the router reads, spelled and piped the
/// way its conditions match them, so a hook never formats them by hand.
async fn run_keys_claim(
    app: Option<String>,
    owner: bool,
    pane: Option<String>,
    keys: Vec<String>,
) -> anyhow::Result<()> {
    let pane = claim_pane(pane)?;
    if keys.is_empty() && app.is_none() && !owner {
        anyhow::bail!("nothing to claim: name keys, --app or --owner");
    }
    let mut spelled = Vec::new();
    for key in &keys {
        if !crate::keyroute::spell::in_scope(key) {
            anyhow::bail!(
                "{key} is not a key tmux's root table could hold: Ctrl, Alt or an F key, in tmux's spelling"
            );
        }
        spelled.push(key.as_str());
    }
    let mut args: Vec<String> = Vec::new();
    let mut set = |name: &str, value: &str| {
        if !args.is_empty() {
            args.push(";".into());
        }
        args.extend(["set-option", "-p", "-t", &pane, name, value].map(String::from));
    };
    if !spelled.is_empty() {
        set("@kc_claim", &format!("|{}|", spelled.join("|")));
    }
    if let Some(app) = &app {
        if !crate::keyroute::route::plain_name(app) {
            anyhow::bail!("{app}: an app's name is letters, digits, '-', '_' and '.'");
        }
        set("@kc_app", app);
    }
    if owner {
        let out = crate::tmux::command()
            .args([
                "display-message",
                "-p",
                "-t",
                &pane,
                "#{pane_current_command}",
            ])
            .output()
            .await?;
        let command = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if command.is_empty() {
            anyhow::bail!("tmux has no pane {pane}");
        }
        set("@kc_owner", &command);
    }
    tmux_or_bail(&args).await
}

/// `keys release`: unset every claim option on the pane.
async fn run_keys_release(pane: Option<String>) -> anyhow::Result<()> {
    let pane = claim_pane(pane)?;
    let mut args: Vec<String> = Vec::new();
    for name in ["@kc_claim", "@kc_owner", "@kc_app"] {
        if !args.is_empty() {
            args.push(";".into());
        }
        args.extend(["set-option", "-p", "-u", "-t", &pane, name].map(String::from));
    }
    tmux_or_bail(&args).await
}

/// One tmux call, its complaint as the error when it fails.
async fn tmux_or_bail(args: &[String]) -> anyhow::Result<()> {
    let out = crate::tmux::command().args(args).output().await?;
    if !out.status.success() {
        anyhow::bail!("tmux: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(())
}

/// The picker rows for a collision report: collisions, then free keys, then
/// what discovery could not read.
///
/// The tmux cell is cut short: a binding with no note is described by its
/// command, and a navigator's `if-shell` is wider than the picker. The whole
/// command is in the preview.
fn collide_items(report: &crate::keyroute::collide::Report) -> Vec<crate::picker::Item> {
    use crate::picker::{Cell, Item};
    const TMUX_CELL: usize = 40;
    let short = |text: &str| -> String {
        if text.chars().count() <= TMUX_CELL {
            text.to_string()
        } else {
            format!(
                "{}\u{2026}",
                text.chars().take(TMUX_CELL - 1).collect::<String>()
            )
        }
    };
    let mut items: Vec<Item> = report
        .collisions
        .iter()
        .map(|c| {
            let apps: Vec<String> = c
                .apps
                .iter()
                .map(crate::keyroute::collide::claim_line)
                .collect();
            let mut preview = format!("{}  route {}\n\ntmux root\n  {}\n", c.key, c.route, c.tmux);
            for (claim, line) in c.apps.iter().zip(&apps) {
                preview.push_str(&format!("{line}\n  found {}\n", claim.rung.as_str()));
            }
            Item::with_preview(
                format!("collision {} {} {}", c.key, c.tmux, apps.join(" ")),
                preview,
            )
            .in_cells(vec![
                Cell::new("collision", crate::picker::Tone::Waiting),
                Cell::strong(c.key.clone()),
                Cell::dim(short(&c.tmux)),
                Cell::plain(apps.join("  |  ")),
            ])
        })
        .collect();
    items.extend(report.drift.iter().map(|d| {
        let found = if d.found.is_empty() {
            "nothing".to_string()
        } else {
            d.found.join(", ")
        };
        Item::with_preview(
            format!("drift {} {} {} {found}", d.key, d.layer, d.expected),
            format!(
                "{}\n\nthe registry says {} should be\n  {}\nit is\n  {found}\n",
                d.key, d.layer, d.expected
            ),
        )
        .in_cells(vec![
            Cell::new("drift", crate::picker::Tone::Failed),
            Cell::strong(d.key.clone()),
            Cell::dim(d.layer.clone()),
            Cell::plain(format!("wants {}, is {found}", d.expected)),
        ])
    }));
    items.extend(report.free.iter().map(|k| {
        Item::new(format!("free {k}")).in_cells(vec![Cell::dim("free"), Cell::strong(k.clone())])
    }));
    items.extend(report.notes.iter().map(|n| {
        Item::new(format!("note {n}")).in_cells(vec![Cell::dim("note"), Cell::plain(n.clone())])
    }));
    items
}

/// The picker rows for a set of bindings.
fn key_items<'a>(rows: impl Iterator<Item = &'a crate::keys::KeyRow>) -> Vec<crate::picker::Item> {
    rows.map(|r| {
        // A plugin's binding often has no note, and its command is the only
        // description it has.
        let described = crate::keys::described(r);
        crate::picker::Item::with_preview(
            format!("{} {described}", r.shown),
            format!("{}\n\n{}", r.shown, r.command),
        )
        // Columns rather than one padded string: the picker measures them
        // across every row, so the notes line up whatever the widest chord
        // turns out to be. The chord is what you press, so it is bold; the
        // `companion: pane` in front of a note is filing, so it is grey and
        // the words after it are what the eye lands on. It stays in the row
        // rather than going, because the opening query matches on it.
        .in_cells({
            let (filed, rest) = crate::keys::filed_under(described);
            vec![
                crate::picker::Cell::strong(r.shown.clone()),
                crate::picker::Cell::dim(filed),
                crate::picker::Cell::plain(rest),
            ]
        })
        .with_icon(crate::tmux::icons::KEY, crate::picker::Tone::Dim)
    })
    .collect()
}

/// Run a picked binding, noting the pick first.
///
/// Recorded before it runs, because the command may replace this process's
/// terminal and never come back to us.
async fn run_binding(
    row: &crate::keys::KeyRow,
    config: &crate::config::Config,
) -> anyhow::Result<()> {
    if config.usage.enabled {
        crate::keys::record_use(&usage_path(config), &row.table, &row.key);
    }

    crate::tmux::command()
        .args(["run-shell", "-C", &row.command])
        .status()
        .await?;
    Ok(())
}

/// Say so when no binding carries the note the pickers key on, and return
/// whether that was the case.
///
/// `keys` opens on `companion: ` and `cheatsheet` shows only rows with that
/// prefix, so a tmux.conf whose bindings have no `-N "companion: ..."` note gets
/// an empty picker or four empty boxes, and both look like the tool is broken
/// rather than like the config is missing a word. One hint on stderr and exit
/// 0: nothing failed, there is just nothing to show yet.
async fn nothing_noted(rows: &[crate::keys::KeyRow], print: bool) -> bool {
    if !crate::keys::filter(rows, "companion: ").is_empty() {
        return false;
    }
    crate::picker::say_nothing_to_show(
        "no bindings carry a -N \"companion: ...\" note; keys and cheatsheet list only those. \
         See docs/tmux.conf.starter.example",
        print,
    )
    .await;
    true
}

/// Where the usage log lives.
fn usage_path(config: &crate::config::Config) -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    config.usage.log_path(&home, crate::server::state_dir())
}

/// What `chunk` can be told to do.
#[derive(Subcommand, Debug, Clone)]
pub enum ChunkAction {
    /// The sitting in words, `38 min, 12 min for break`, or nothing when
    /// no sitting is running
    Status {
        /// Every number, as JSON
        #[arg(long)]
        json: bool,
    },
    /// Past the budget, one extension of `[chunk] snooze`, once a sitting
    Snooze,
    /// A new sitting at 0, the budget back to the file's
    Reset,
    /// End the sitting; nothing counts until you've been away and back
    Close,
    /// This sitting's budget, until `reset` or a daemon restart
    Budget {
        /// `45m`, `1h`, `90s`
        duration: String,
    },
}

/// What `health` can be told to do.
#[derive(Subcommand, Debug, Clone, Copy)]
pub enum HealthAction {
    /// Forget the timers and segments that failed, so the mark comes down
    /// now and not an hour after the last of them
    Ack,
}

/// `health`: the daemon's reasons, or with `ack` what it forgot.
///
/// The daemon writes both, since it holds the failures and the moment it
/// started, and a client has neither.
async fn run_health(ack: bool) -> anyhow::Result<()> {
    let args = crate::proto::HealthArgs { ack };
    let resp = crate::client::send(Request::build("health", &args)).await?;
    if let Some(e) = resp.error {
        anyhow::bail!(e);
    }
    // `ok` in green with the check; a reason in the colour the health mark
    // is drawn in on the bar, since that mark is why anybody asks.
    use crate::picker::Tone;
    let paint = crate::picker::Paint::for_stdout(false);
    for line in resp.output.lines() {
        if line.trim() == "ok" {
            println!(
                "{}{}",
                paint.icon(crate::tmux::icons::CHECK, Tone::Ok),
                paint.ink(line, Tone::Ok)
            );
        } else {
            println!(
                "{}{}",
                paint.icon(crate::tmux::icons::HEALTH, Tone::Waiting),
                paint.ink(line, Tone::Waiting)
            );
        }
    }
    if resp.output.is_empty() {
        println!();
    }
    Ok(())
}

/// `cheatsheet`: the same rows the picker uses, laid out in four boxes.
async fn run_cheatsheet(print: bool) -> anyhow::Result<()> {
    let resp =
        crate::client::send(Request::build("keys", &crate::proto::KeysArgs::default())).await?;
    if let Some(e) = resp.error {
        anyhow::bail!(e);
    }

    let rows: Vec<crate::keys::KeyRow> = resp
        .output
        .lines()
        .filter_map(|line| {
            let mut f = line.splitn(5, '\t');
            Some(crate::keys::KeyRow {
                table: f.next()?.to_string(),
                key: f.next()?.to_string(),
                shown: f.next()?.to_string(),
                note: f.next()?.to_string(),
                command: f.next()?.to_string(),
            })
        })
        .collect();

    if nothing_noted(&rows, print).await {
        return Ok(());
    }

    // With the log off it is not read at all, even when an old one is there:
    // off means the sheet learns nothing, not that it learns from a file
    // somebody stopped writing.
    let config = config_or_default();
    let (usage, learning) = if config.usage.enabled {
        let usage = std::fs::read_to_string(usage_path(&config))
            .map(|t| crate::keys::usage(&t))
            .unwrap_or_default();
        let learning = crate::cheatsheet::Learning::On {
            now: crate::panes::now_secs(),
            learned_after_days: config.usage.learned_after_days,
        };
        (usage, learning)
    } else {
        (Default::default(), crate::cheatsheet::Learning::Off)
    };
    let sheet = crate::cheatsheet::sheet(&rows, &usage, learning);
    let footer = crate::cheatsheet::footer(&sheet);

    if print {
        // One row per line on stdout, the shape `keys --print` gives, and the
        // footer on stderr so a script reading stdout never sees it.
        print!("{}", crate::cheatsheet::tsv(&sheet));
        if !footer.is_empty() {
            eprintln!("{footer}");
        }
        return Ok(());
    }

    let (cols, lines) = terminal_size();
    let paint = crate::picker::Paint::for_stdout(false);
    // The keys where every picker has them, from the same setting. The two
    // lines the sheet holds back are this one and the footer.
    let hint_at = config
        .picker
        .resolved(crate::config::Picker::Keys)
        .look
        .hint_position;
    let hint = "any key to close";
    if hint_at == crate::picker::Edge::Top {
        println!(" {}", paint.ink(hint, crate::picker::Tone::Dim));
    }
    print!(
        "{}",
        crate::cheatsheet::render_in(&sheet.boxes, cols, lines, &paint)
    );

    use std::io::Write;
    let mut tail: Vec<&str> = Vec::new();
    if !footer.is_empty() {
        tail.push(&footer);
    }
    if hint_at == crate::picker::Edge::Bottom {
        tail.push(hint);
    }
    if !tail.is_empty() {
        print!("\n  {} ", tail.join("   "));
    }
    let _ = std::io::stdout().flush();
    wait_for_a_key();
    Ok(())
}

/// The terminal size, or a sensible guess.
///
/// `display-popup` gives the popup its own size, and a guess that is too small
/// costs a cramped sheet rather than a broken one.
fn terminal_size() -> (usize, usize) {
    match ratatui::crossterm::terminal::size() {
        Ok((c, l)) => (c as usize, l as usize),
        Err(_) => (120, 40),
    }
}

/// Block until somebody presses something, in raw mode so it takes one key
/// rather than a whole line.
fn wait_for_a_key() {
    use ratatui::crossterm::{
        event::{self, Event},
        terminal::{disable_raw_mode, enable_raw_mode},
    };

    if enable_raw_mode().is_err() {
        // No tty: there is nothing to wait for and nothing to restore.
        return;
    }
    loop {
        match event::read() {
            Ok(Event::Key(_)) => break,
            Ok(_) => continue,
            Err(_) => break,
        }
    }
    let _ = disable_raw_mode();
}

/// `project`: pick a project, then switch to its session or build one.
/// `start`: the front door, for a shell that is not in tmux yet.
///
/// Everything here already existed behind `project`; what was missing was a
/// name for it that reads like the first thing you type rather than like a
/// binding you press once you are already inside.
async fn run_start(dir: Option<String>, last: bool, hook: bool) -> anyhow::Result<()> {
    if hook {
        return run_start_hook().await;
    }
    if dir.is_some() {
        return run_project(dir, false, false).await;
    }
    if last {
        let listing = tmux_capture(&[
            "list-sessions",
            "-F",
            "#{session_last_attached} #{session_name}",
        ])
        .await;
        if let Some(name) = crate::project::most_recent_session(&listing) {
            return focus_session(&name).await;
        }
        // Nothing to go back to, so ask rather than failing: `--last` on a
        // machine that has just booted is the same situation as no flag.
    }
    run_project(None, false, false).await
}

/// `start --hook`: the `client-attached` half, which decides for itself.
///
/// The decision was a tmux format condition in the config first:
///
///   if-shell -F "#{?#{==:#{session_name},#{s|[0-9]||:#{session_name}}},0,1}" ...
///
/// which is right as a format -- `display-message -p` prints exactly what it
/// should -- and never ran the command from inside a hook, for any session.
/// Asking tmux one question and deciding here costs a few milliseconds on
/// attach and is something that can be read and tested.
async fn run_start_hook() -> anyhow::Result<()> {
    let answer = tmux_capture(&[
        "display-message",
        "-p",
        "#{session_name}\t#{session_windows}\t#{window_panes}\t#{pane_current_command}",
    ])
    .await;
    if !crate::project::is_an_untouched_default_session(&answer) {
        return Ok(());
    }
    // This binary by its own path, not the bare name. A popup runs its command
    // through a shell whose PATH is the tmux *server's*, which was inherited
    // from wherever the server happened to start -- often a login shell that
    // has never seen ~/.local/bin. The popup then opened, failed to find
    // `tmux-companion`, and closed again too fast to see.
    let me = std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "tmux-companion".to_string());
    tmux(&[
        "display-popup",
        "-E",
        "-w",
        "80%",
        "-h",
        "70%",
        &format!("{me} project"),
    ])
    .await;
    Ok(())
}

async fn run_project(dir: Option<String>, print: bool, detached: bool) -> anyhow::Result<()> {
    use crate::project::Kind;

    let config = config_or_default();
    let home = std::env::var("HOME").unwrap_or_default();

    // Straight to a directory, which is what the shell alias does.
    if let Some(dir) = dir {
        let cwd = std::env::current_dir()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        let Some(path) = crate::project::resolve_typed(&dir, &cwd, &home) else {
            anyhow::bail!("no such directory: {dir}");
        };
        // The session made and its name said, nothing switched: on a host,
        // the attach that follows is the other machine's mirror session.
        if detached {
            let name = crate::project::session_name(&path);
            build_project_session(&name, &path, &config, &home).await;
            println!("{name}");
            return Ok(());
        }
        return open_project(&path, &config, &home).await;
    }

    let chrome = crate::picker::Chrome {
        title: "[ Project ]".into(),
        icon: crate::tmux::icons::SESSION.into(),
        footer: "enter open   alt-s last session   esc cancel   type a path for a new one".into(),
        preview_title: "[ Where ]".into(),
        ..Default::default()
    }
    .configured(&config.picker, crate::config::Picker::Project);
    // The picker on screen before its rows: collecting them reads the session
    // list, the directory source and every theme file.
    let _loading = (!print).then(|| crate::picker::loading(&chrome, ""));

    let found = project_rows(&config, &home).await;
    let ProjectRows {
        sessions,
        remote,
        views,
        dirs,
        here,
        dirs_failed,
    } = found;
    let now = crate::sessions::idle::now();

    if print {
        let print_row = |row: &crate::project::Row| {
            println!(
                "{}\t{}\t{}\t{}",
                match row.kind {
                    Kind::Session => "session",
                    Kind::Directory => "dir",
                },
                row.label,
                crate::project::short_path(&row.path, &home),
                row.colour.clone().unwrap_or_default()
            );
        };
        sessions.iter().for_each(print_row);
        // A host's group line says where it is, its reason when it can't
        // be reached included; its sessions say they are the host's.
        for r in &remote {
            match &r.target {
                crate::remote::Target::Group(h) => {
                    let line = views
                        .iter()
                        .find(|v| v.name == *h)
                        .map_or(r.line.clone(), |v| crate::remote::group_line(v, now, true));
                    println!("host\t{line}\t\t");
                }
                crate::remote::Target::Session { host, name, .. } => {
                    let path = views
                        .iter()
                        .find(|v| v.name == *host)
                        .and_then(|v| v.sessions.iter().find(|s| s.name == *name))
                        .map(|s| s.path.clone())
                        .unwrap_or_default();
                    println!("session\t{}\t{path}\t", r.line);
                }
            }
        }
        if let Some(why) = &dirs_failed {
            println!("dirs\t{why}\t\t");
        }
        dirs.iter().for_each(print_row);
        return Ok(());
    }
    if let Some(why) = &dirs_failed {
        crate::remote::say(why).await;
    }

    let remote = crate::remote::for_picker(remote, &crate::remote::open_mirrors(&sessions));
    let n_local = sessions.len();
    let n_remote = remote.len();
    let mut items = project_items(&sessions, &config, &home);
    items.extend(crate::remote::items(&remote, &views, now));
    items.extend(project_items(&dirs, &config, &home));
    let mut rows = sessions;
    rows.extend(dirs);

    let chrome = crate::picker::Chrome {
        // M-s opens this picker, so M-s again goes back where you were.
        alt_picks: crate::project::previous_session(&rows)
            .map(|i| vec![('s', i)])
            .unwrap_or_default(),
        ..chrome
    };

    let outcome = crate::picker::run_with_query(items, "", &chrome)?;
    // A host's row: its session opens as a mirror session, its group line
    // retries the link, as in the session switcher.
    if let crate::picker::Outcome::Chosen(i) = outcome
        && (n_local..n_local + n_remote).contains(&i)
    {
        return match &remote[i - n_local].target {
            crate::remote::Target::Group(host)
            | crate::remote::Target::Session {
                host,
                reachable: false,
                ..
            } => {
                crate::remote::retry(host).await;
                Ok(())
            }
            crate::remote::Target::Session { host, name, .. } => {
                crate::remote::open(host, name, false, false).await
            }
        };
    }
    let outcome = match outcome {
        crate::picker::Outcome::Chosen(i) if i >= n_local + n_remote => {
            crate::picker::Outcome::Chosen(i - n_remote)
        }
        o => o,
    };
    let cwd = std::env::current_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    match project_pick(&outcome, &rows, &cwd, &home, here.as_deref()) {
        // Switched to by its own name: deriving one from the path is wrong for
        // any session whose path is not a project directory.
        ProjectPick::Session(name) => focus_session(&name).await,
        ProjectPick::Directory(path) => open_project(&path, &config, &home).await,
        ProjectPick::HostDirectory(host, path) => crate::remote::open_project(&host, &path).await,
        ProjectPick::Missing(typed) => {
            crate::picker::say_nothing_to_show(&format!("no such directory: {typed}"), false).await;
            Ok(())
        }
        ProjectPick::Nothing => Ok(()),
    }
}

/// What the project picker lists, in its order: this server's sessions,
/// each host's group line and sessions, then the directories of one place.
pub(crate) struct ProjectRows {
    /// This server's sessions, most recently used first.
    sessions: Vec<crate::project::Row>,
    /// Every host's rows, as the session switcher shows them.
    remote: Vec<crate::remote::Row>,
    /// The hosts the remote rows came from.
    views: Vec<crate::link::manager::HostView>,
    /// The directories: this machine's source, or inside a mirror session
    /// that host's, through its link.
    dirs: Vec<crate::project::Row>,
    /// The host whose mirror session the picker was opened in.
    here: Option<String>,
    /// Why a host's directories aren't listed, when they aren't.
    dirs_failed: Option<String>,
}

/// The host a mirror session stands on, when the session the picker opened
/// from is one: the first row, which recency puts there. Only inside tmux,
/// since outside it no session is the one you are in.
pub(crate) fn here_host(sessions: &[crate::project::Row]) -> Option<String> {
    std::env::var_os("TMUX")?;
    sessions
        .first()
        .and_then(|r| r.mirror.as_ref())
        .map(|(host, _)| host.clone())
}

/// Collect [`ProjectRows`]. The local directory source starts beside the
/// session list, as it always has, and is dropped unread when the session
/// list says the picker is in a mirror session, whose directories are its
/// host's.
async fn project_rows(config: &crate::config::Config, home: &str) -> ProjectRows {
    let local_dirs = {
        let (config, home) = (config.clone(), home.to_string());
        tokio::spawn(async move { crate::project::directories(&config, &home).await })
    };
    let mut sessions = crate::project::sessions(&[]).await;
    let views = crate::remote::views_for_picker();
    crate::remote::host_paths(&mut sessions, &views);
    let now = crate::sessions::idle::now();
    let open = crate::remote::open_mirrors(&sessions);
    let remote = crate::remote::rows(&views, &open, now);
    let here = here_host(&sessions);
    let (sessions, dirs, dirs_failed) = match &here {
        Some(host) => {
            local_dirs.abort();
            match crate::remote::host_dirs(host).await {
                Ok(paths) => (
                    sessions,
                    crate::remote::dir_rows(host, &paths, &views),
                    None,
                ),
                Err(why) => (sessions, Vec::new(), Some(why)),
            }
        }
        None => {
            let found = local_dirs.await.unwrap_or_default();
            // `merge`, as the picker always has: a directory a session
            // stands for goes, and so does a second session in one place.
            let (sessions, dirs) = crate::project::merge(sessions, found)
                .into_iter()
                .partition(|r| r.kind == crate::project::Kind::Session);
            (sessions, dirs, None)
        }
    };
    ProjectRows {
        sessions,
        remote,
        views,
        dirs,
        here,
        dirs_failed,
    }
}

/// The project picker's rows: sessions bold, directories plain, each with
/// its path, an idle column when it has been quiet a day, and its theme
/// colour as a block. Shared with the session switcher.
pub(crate) fn project_items(
    rows: &[crate::project::Row],
    config: &crate::config::Config,
    home: &str,
) -> Vec<crate::picker::Item> {
    use crate::project::Kind;
    let mut items: Vec<crate::picker::Item> = Vec::with_capacity(rows.len());
    for r in rows {
        use crate::picker::{Cell, Tone};
        // The kind of row is the icon rather than a word in its own column:
        // `session` down half the list said the same thing on every line.
        // A live session is what you switch to, so its name is bold; a
        // directory is somewhere you could go, so it is plain.
        let (icon, name) = match r.kind {
            Kind::Session => (crate::tmux::icons::SESSION, Tone::Strong),
            Kind::Directory => (crate::tmux::icons::PROJECT, Tone::Plain),
        };
        let mark = match r.kind {
            Kind::Session => "session",
            Kind::Directory => "dir",
        };
        let where_it_is = crate::project::short_path(&r.path, home);
        // A detached session quiet for a day or more says so in a fourth
        // column, so the stale ones show without leaving the list. The column
        // is only pushed when there is something to say: an empty last cell
        // would still pad the path out to the widest row.
        let idle = crate::project::idle_column(r);
        let mut label = format!("{mark} {} {}", r.label, where_it_is);
        let mut cells = match (&r.kind, &r.mirror) {
            // A host's directory: `api@vm` in the words, with `@vm` in the
            // host tone as a remote session's row has it.
            (Kind::Directory, Some((host, _))) => {
                let base = r
                    .label
                    .strip_suffix(&format!("@{host}"))
                    .unwrap_or(&r.label)
                    .to_string();
                vec![
                    Cell::new(base, name),
                    Cell::new(format!("@{host}"), Tone::Host),
                    Cell::dim(where_it_is),
                ]
            }
            // A mirror session the same, so it reads as the host's whether it
            // is open here or not.
            (Kind::Session, Some((host, _))) => {
                let base = r
                    .label
                    .strip_suffix(&format!("@{host}"))
                    .unwrap_or(&r.label)
                    .to_string();
                vec![
                    Cell::new(base, name),
                    Cell::new(format!("@{host}"), Tone::Host),
                    Cell::dim(where_it_is),
                ]
            }
            _ => vec![Cell::new(r.label.clone(), name), Cell::dim(where_it_is)],
        };
        // A mirror session wears the ssh glyph its host's remote rows do.
        let (icon, icon_tone) = match (&r.kind, &r.mirror) {
            (Kind::Session, Some(_)) => (crate::tmux::icons::SSH, Tone::Host),
            _ => (icon, Tone::Dim),
        };
        if let Some(idle) = idle {
            // In the label too, so typing `idle` filters down to them.
            label.push(' ');
            label.push_str(&idle);
            cells.push(Cell::dim(idle));
        }
        // Built when the row is selected, not here: two tmux calls per session
        // and a listing per directory, awaited one after another for every
        // row, were most of the time this picker took to open.
        let (row, preferred) = (r.clone(), config.project.preview_window.clone());
        items.push(
            crate::picker::Item::with_lazy_preview(label, move || {
                project_preview(&row, &preferred)
            })
            .in_cells(cells)
            .with_icon(icon, icon_tone)
            // The project's own theme colour as a block in front of the row.
            // Painting the whole line in it, which is what this did before,
            // makes a dark project colour unreadable on a dark popup.
            .with_swatch(r.colour.clone()),
        );
    }
    items
}

/// What the project picker's answer opens.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ProjectPick {
    /// A live session, by name.
    Session(String),
    /// A directory, listed or typed, to open a session in.
    Directory(String),
    /// A directory on a host, listed or typed inside one of its mirror
    /// sessions: the host, then the path as the host will read it.
    HostDirectory(String, String),
    /// A typed path that is not a directory, as it was typed.
    Missing(String),
    /// Cancelled, or enter on an empty query.
    Nothing,
}

/// A row picked, or the query when enter matched nothing (or alt-enter asked
/// for the query itself): the one way into a directory the source has never
/// listed, which the footer has always promised.
///
/// `here` is the host whose mirror session the picker opened in: a path
/// typed there is the host's to read, so it goes to the host as typed.
pub(crate) fn project_pick(
    outcome: &crate::picker::Outcome,
    rows: &[crate::project::Row],
    cwd: &str,
    home: &str,
    here: Option<&str>,
) -> ProjectPick {
    use crate::picker::Outcome;
    use crate::project::{Kind, Row};
    match outcome {
        Outcome::Chosen(i) => match rows.get(*i) {
            Some(r) if r.kind == Kind::Session => ProjectPick::Session(r.label.clone()),
            Some(Row {
                mirror: Some((host, _)),
                path,
                ..
            }) => ProjectPick::HostDirectory(host.clone(), path.clone()),
            Some(r) => ProjectPick::Directory(r.path.clone()),
            None => ProjectPick::Nothing,
        },
        Outcome::Typed(q) if q.trim().is_empty() => ProjectPick::Nothing,
        Outcome::Typed(q) if here.is_some() => {
            ProjectPick::HostDirectory(here.unwrap_or_default().to_string(), q.trim().to_string())
        }
        Outcome::Typed(q) => match crate::project::resolve_typed(q, cwd, home) {
            Some(path) => ProjectPick::Directory(path),
            None => ProjectPick::Missing(q.trim().to_string()),
        },
        Outcome::Cancelled => ProjectPick::Nothing,
    }
}

/// Switch this client to a session, or attach when run from outside tmux.
pub(crate) async fn focus_session(name: &str) -> anyhow::Result<()> {
    let inside = std::env::var_os("TMUX").is_some();
    let verb = if inside {
        "switch-client"
    } else {
        "attach-session"
    };
    crate::tmux::attach_command()
        .args([verb, "-t", &format!("={name}")])
        .status()
        .await?;
    Ok(())
}

/// Create the session for a directory if it is not there, then switch to it.
async fn open_project(
    path: &str,
    config: &crate::config::Config,
    home: &str,
) -> anyhow::Result<()> {
    let name = crate::project::session_name(path);
    build_project_session(&name, path, config, home).await;
    focus_session(&name).await
}

/// Create a session called `name` in a project directory, with the layout
/// that project gets, unless a session of that name is already there.
pub(crate) async fn build_project_session(
    name: &str,
    path: &str,
    config: &crate::config::Config,
    home: &str,
) {
    let name = name.to_string();

    // Through `session_exists`, which sends tmux's stderr nowhere: asked
    // bare, a first open printed `can't find session: x` before going on to
    // create exactly that session, so every success opened with a complaint.
    if !session_exists(&format!("={name}")).await {
        crate::project::record_visit(config, path).await;

        let repo = crate::repofile::load(path);
        let (windows, _) =
            crate::saved::resolve(config, load_saved(path).await.layout(), &repo, path, home);

        let spec = crate::project::SessionSpec {
            name: &name,
            path,
            home,
            pane_base: pane_base_index().await,
            windows: &windows,
        };
        for cmd in crate::project::session_commands(&spec) {
            let borrowed: Vec<&str> = cmd.iter().map(String::as_str).collect();
            tmux(&borrowed).await;
        }
        if config.journal.enabled {
            crate::journal::append(&crate::journal::Event {
                at: crate::panes::now_secs(),
                session: name.clone(),
                path: crate::project::short_path(path, home),
                kind: crate::journal::Kind::Opened,
                detail: String::new(),
            });
        }
    }
}

/// The server's `pane-base-index`, defaulting to 0 the way tmux does.
///
/// Read rather than assumed: a config that sets it to 1 makes every
/// `window.0` target miss, and tmux answers a missed target by doing nothing
/// rather than by saying so, so the session would come up with its commands
/// quietly absent.
async fn pane_base_index() -> usize {
    crate::tmux::command()
        .args(["show-option", "-gv", "pane-base-index"])
        .output()
        .await
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0)
}

/// What `project <verb>` does instead of opening the picker.
///
/// A directory whose name is one of these verbs has to be written as a path,
/// `project ./save`, because clap reads a bare `save` as the subcommand. That
/// is the cost of the spelling in the documentation, and it is cheaper than a
/// flag nobody remembers.
/// What `switch` moves between.
#[derive(clap::Subcommand, Debug, Clone)]
pub enum SwitchWhat {
    /// This session's windows, most recently used first. From run-shell it is
    /// a press: the first goes to the window you were on before, a second
    /// opens the list (`[switcher]` first). In a terminal it is the list
    Windows {
        /// The session, by name or `$id`; the client's when only --client is
        /// given
        session: Option<String>,
        /// Go the other way: the oldest first
        #[arg(long)]
        reverse: bool,
        /// The client the key was pressed on, as `#{client_name}` from the
        /// binding, for the popup and the message line
        #[arg(long)]
        client: Option<String>,
        /// Print the windows, index and name, most recent first, and exit
        #[arg(long)]
        print: bool,
        /// Internal: opened by a press, which left where the presses began
        #[arg(long, hide = true)]
        pressed: bool,
    },
    /// Every session, most recently used first, with the cursor on the one
    /// you were in before; typing searches the project directories too
    Sessions {
        /// Start on the oldest session instead
        #[arg(long)]
        reverse: bool,
        /// Print the session names, most recent first, then each host's
        /// line and its sessions, and exit
        #[arg(long)]
        print: bool,
        /// Internal: open with the cursor on this row, `host:vm`,
        /// `remote:vm:api` or `local:api`, after a question on the message
        /// line closed the popup
        #[arg(long, hide = true)]
        at: Option<String>,
    },
}

#[derive(clap::Subcommand, Debug, Clone)]
pub enum ProjectAction {
    /// Capture this session's windows and panes as this project's layout
    Save {
        /// Record the geometry and leave every pane a plain shell
        #[arg(long)]
        no_commands: bool,
    },
    /// Delete this project's saved layout and fall back to the config
    Forget {
        /// The project directory, instead of the session this runs in
        dir: Option<String>,
    },
    /// Which layout this project gets, and which file decided
    Show {
        /// The project directory, instead of the session this runs in
        dir: Option<String>,
    },
    /// Close this project by letting every window exit
    Close {
        /// The session, defaulting to the current one
        session: Option<String>,
        /// Quit editors with :qa! and throw away unsaved work
        #[arg(long)]
        discard: bool,
        /// Close without capturing the layout, leaving any saved one alone
        #[arg(long)]
        no_save: bool,
    },
}

/// What `sessions` can do to the snapshot store.
///
/// A project's layout is a catalogue entry, kept for good and edited by hand.
/// A snapshot is a moment, kept in generations and thrown away oldest first.
/// The two are separate commands because they answer different questions, and
/// `sessions` is plural because it is always about every session at once.
#[derive(clap::Subcommand, Debug, Clone)]
pub enum SessionsAction {
    /// Write a generation as one file that reads on another machine: paths
    /// under home spelled ~, the project colours bundled, screens left out
    Export {
        /// Where to write it; none or `-` prints it
        file: Option<String>,
        /// Which generation, defaulting to the newest
        #[arg(long)]
        stamp: Option<String>,
    },
    /// Read a file from `export` and store it here as a new generation, adding
    /// the project colours this machine does not have
    Import {
        /// The file `sessions export` wrote
        file: String,
        /// Leave the project map alone
        #[arg(long)]
        no_map: bool,
    },
    /// Rename a session, its mirrors following: on a host, a session's
    /// grouped mirror sessions become NEW~machine and each machine's mirror
    /// NEW@host; inside a mirror's grouped session, the session renamed is
    /// the one it stands for. The project colour moves with it
    Rename {
        /// The new name
        new: String,
        /// The session to rename, defaulting to the one this runs in
        #[arg(long)]
        session: Option<String>,
    },
    /// Capture every session now, as a new generation
    Save {
        /// Record the sessions and skip what was on each pane's screen
        ///
        /// The expensive half at 9.3 ms a pane against 1 ms for the metadata of
        /// a whole server, and the half that holds whatever you printed.
        #[arg(long)]
        skip_pane_history: bool,
        /// Sessions not to capture, comma separated
        ///
        /// Added to `[sessions] exclude` rather than replacing it.
        #[arg(long, value_delimiter = ',')]
        exclude: Vec<String>,
    },
    /// Rebuild a server from a snapshot
    Resurrect {
        /// Which generation, defaulting to the newest
        stamp: Option<String>,
        /// Only these sessions, comma separated
        #[arg(long, value_delimiter = ',')]
        only: Vec<String>,
        /// Never these sessions, comma separated
        #[arg(long, value_delimiter = ',')]
        exclude: Vec<String>,
        /// Add the sessions that are missing instead of refusing a busy server
        #[arg(long)]
        merge: bool,
        /// Print the tmux commands this would run, and exit
        #[arg(long)]
        dry_run: bool,
        /// Do not ask about panes the restore table did not claim
        #[arg(long)]
        yes: bool,
        /// Leave the server running rather than attaching to it
        #[arg(long)]
        detach: bool,
    },
    /// Save every session, then stop the tmux server (the daemon stays up
    /// unless --daemon-too; plain `shutdown` is the one that stops the daemon)
    Shutdown {
        /// Sessions not to save, comma separated
        ///
        /// Stopping the server takes every session with it, so a session left
        /// out here is one that does not come back.
        #[arg(long, value_delimiter = ',')]
        exclude: Vec<String>,
        /// Stop the tmux-companion daemon as well. Off by default: the daemon
        /// keeps running
        #[arg(long)]
        daemon_too: bool,
        /// Print what this would do, and exit
        #[arg(long)]
        dry_run: bool,
    },
    /// Save, stop the tmux server, and bring it back with what it had (the
    /// daemon is restarted too unless --keep-daemon; plain `restart` is the one
    /// that restarts only the daemon)
    Restart {
        /// Sessions not to save, comma separated
        #[arg(long, value_delimiter = ',')]
        exclude: Vec<String>,
        /// Leave the daemon running with the config it started with. Off by
        /// default: the daemon is restarted so it rereads config.toml
        #[arg(long)]
        keep_daemon: bool,
        /// Print what this would do, and exit
        #[arg(long)]
        dry_run: bool,
    },
    /// The snapshot timer the daemon runs. With no flag it reports, like
    /// --status
    Autosave {
        /// Take one now
        #[arg(long)]
        once: bool,
        /// Say when the last one happened, and what the timer is set to
        #[arg(long)]
        status: bool,
    },
    /// Every generation, newest first
    List {
        /// Print JSON rather than a table
        #[arg(long)]
        json: bool,
    },
    /// What one generation holds
    Show {
        /// Which generation, defaulting to the newest
        stamp: Option<String>,
        /// Print JSON rather than a listing
        #[arg(long)]
        json: bool,
    },
    /// Open a session by name: the live one, else the newest snapshot that
    /// holds it, else the project directory it names, else DIR or a
    /// directory you pick
    Start {
        /// The session's name
        name: String,
        /// Where to start it when nothing else knows the name
        dir: Option<String>,
        /// Start or restore it and leave it running, attaching nothing
        #[arg(long)]
        detach: bool,
    },
    /// Go back to the session you left: the one used most recently that no
    /// client is in, by the switcher's hooks or tmux's attach time; with no
    /// record of either, the oldest session
    Continue {
        /// The youngest session other than this one, ignoring every record
        #[arg(long)]
        last: bool,
        /// Print the session and why it was chosen, and exit
        #[arg(long)]
        print: bool,
    },
    /// Sessions nobody is attached to that have been quiet for days; picking
    /// one runs `project close` on it
    Idle {
        /// Quiet for longer than this many days
        #[arg(long, default_value_t = 3)]
        days: u64,
        /// Print the rows as TSV and exit, opening nothing
        #[arg(long)]
        print: bool,
    },
}

/// What the project picker shows beside a row.
///
/// A session with an `ai` window shows that window's screen, so the picker
/// answers "what is the agent doing over there" without switching to it. That
/// was the whole point of the preview in the script this replaced, and it is
/// the reason the preview is worth having at all. Anything else falls back to
/// a directory listing, which is all there is to say about a project that is
/// not open yet.
fn project_preview(row: &crate::project::Row, preferred: &str) -> String {
    use crate::project::Kind;
    if row.kind == Kind::Session {
        let target = format!("={}", row.label);

        // The preferred window first, because "what is the agent doing over
        // there" is the question worth answering without switching, and on
        // this machine that window is called `ai`. Failing that, the window
        // the session is currently on, so every session previews something:
        // the script this replaced only ever looked for `ai`, and a session
        // without one showed a directory listing whose contents were already
        // on the row above.
        let named = if preferred.is_empty() {
            false
        } else {
            tmux_capture_sync(&["list-windows", "-t", &target, "-F", "#{window_name}"])
                .lines()
                .any(|w| w.trim() == preferred)
        };
        // A trailing colon, which is the difference between a session target
        // and a pane one. `capture-pane -t =alpha` answers "can't find pane:
        // =alpha" -- the `=` means an exact session name, and capture-pane
        // wants a pane -- so every live session's preview came back empty and
        // fell through to a directory listing whose contents were already on
        // the row above. `=alpha:` is that session's current window and its
        // active pane, which is the screen somebody wants to see.
        let pane = if named {
            format!("={}:{preferred}", row.label)
        } else {
            format!("={}:", row.label)
        };
        // -e keeps whatever colours are on that screen.
        let screen = tmux_capture_sync(&["capture-pane", "-p", "-e", "-t", &pane]);
        let tail = crate::project::tail_of_screen(&screen, 40);
        if !tail.is_empty() {
            return tail;
        }
    }
    // A mirror session's path is the host's, not one to list here.
    if row.mirror.is_some() {
        return String::new();
    }
    crate::project::listing_of(std::path::Path::new(&row.path), 40)
}

/// `new-window`: pick a directory, open a window there.
///
/// The tmux default for prefix+c opens a window in the pane's directory and
/// gives you no say in it. This keeps that as the zero-keystroke case and adds
/// the rest: the frecency list, and any path at all typed in full.
async fn run_new_window(print: bool) -> anyhow::Result<()> {
    let home = std::env::var("HOME").unwrap_or_default();

    // Asking tmux rather than reading the process's own directory: a popup
    // inherits the pane's directory, but a binding run with -d somewhere else
    // does not, and the answer has to be the pane somebody is looking at.
    //
    // Targeted at the attached client's session, because "the pane somebody is
    // looking at" is exactly what an untargeted question does not answer from
    // inside a popup: it answers for the session the server touched last, so
    // the query came up prefilled with a directory from the project you had
    // just switched away from.
    let config = config_or_default();
    let chrome = crate::picker::Chrome {
        title: "[ New window at ]".into(),
        icon: crate::tmux::icons::FOLDER.into(),
        footer: "enter open window   ctrl-a clear   esc cancel   type a path not listed".into(),
        preview_title: "[ Directory ]".into(),
        ..Default::default()
    }
    .configured(&config.picker, crate::config::Picker::Window);
    // On screen before the directory source and the pane's directory are
    // asked for; the query fills in when the rows arrive.
    let _loading = (!print).then(|| crate::picker::loading(&chrome, ""));

    let client = client_target().await;
    let pane_dir = {
        let d = tmux_display_at(client.as_deref(), "#{pane_current_path}").await;
        if std::path::Path::new(&d).is_dir() {
            d
        } else {
            std::env::current_dir()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|_| home.clone())
        }
    };

    let paths: Vec<String> = crate::project::source_dirs(&config, &home).await;

    let prefill = crate::project::short_path(&pane_dir, &home);

    // The list as text: `here` on the pane's own directory, then the short
    // path and the full one, for a screen reader or a script, which can then
    // run `tmux new-window -c` on the one it wants.
    if print {
        if !paths.iter().any(|p| p == &pane_dir) {
            println!("here\t{prefill}\t{pane_dir}");
        }
        for p in &paths {
            let here = if p == &pane_dir { "here" } else { "" };
            println!("{here}\t{}\t{p}", crate::project::short_path(p, &home));
        }
        return Ok(());
    }

    // The pane's own directory goes on the list, not only into the query.
    // Without it, a directory the source has never seen -- which is most of them
    // the first time somebody runs this -- prefilled a query that matched no
    // row, so the list came up empty and the default looked broken. Enter
    // still opens it either way; now you can also see what Enter would do.
    let mut items: Vec<crate::picker::Item> = Vec::with_capacity(paths.len() + 1);
    // What each row opens, in the same order as the rows, because a pick
    // comes back as an index and the extra row below would otherwise shift
    // every listed path by one.
    let mut targets: Vec<String> = Vec::with_capacity(paths.len() + 1);
    // What is in the directory, which is what `--preview="ls -A1 {2}"` showed.
    // The preview used to be the path itself, which the row already says: a
    // pane of one line repeating the line you are looking at.
    // Listed when the row is selected rather than all before the first frame.
    let listing = |path: &str| {
        let path = path.to_string();
        move || crate::project::listing_of(std::path::Path::new(&path), 200)
    };
    if !paths.iter().any(|p| p == &pane_dir) {
        items.push(
            crate::picker::Item::with_lazy_preview(prefill.clone(), listing(&pane_dir))
                .in_cells(vec![
                    crate::picker::Cell::dim("here"),
                    crate::picker::Cell::plain(prefill.clone()),
                ])
                .with_icon(crate::tmux::icons::FOLDER, crate::picker::Tone::Dim),
        );
        targets.push(pane_dir.clone());
    }
    for p in &paths {
        let short = crate::project::short_path(p, &home);
        items.push(
            crate::picker::Item::with_lazy_preview(short.clone(), listing(p))
                .in_columns(vec![String::new(), short])
                .with_icon(crate::tmux::icons::FOLDER, crate::picker::Tone::Dim),
        );
        targets.push(p.clone());
    }
    let outcome = crate::picker::run_with_query(items, &prefill, &chrome)?;
    match crate::project::window_target(&outcome, &targets, &prefill, &pane_dir, &home) {
        crate::project::WindowTarget::Cancelled => Ok(()),
        crate::project::WindowTarget::NoSuchDirectory(q) => {
            tmux(&[
                "display-message",
                &format!("new window: no such directory: {q}"),
            ])
            .await;
            Ok(())
        }
        crate::project::WindowTarget::Open(dir) => {
            // Counted as a visit, the same as `z` would, so opening a window
            // somewhere twice floats it up the list next time.
            crate::project::record_visit(&config, &dir).await;
            // Targeted at this pane's session, so the window lands where the
            // person is looking rather than in whichever session the server
            // used last. A session and not the pane: `new-window -t %7` is
            // "can't specify pane here", because for new-window the target is
            // the index to create at. A bare `session:` appends at the next
            // free index, which is what this did before it was targeted.
            match client.as_deref() {
                Some(target) => tmux(&["new-window", "-t", target, "-c", &dir]).await,
                None => tmux(&["new-window", "-c", &dir]).await,
            }
            Ok(())
        }
    }
}

/// `shell-init`: print the prompt-mark hook for a shell.
fn run_shell_init(shell: Option<String>) -> anyhow::Result<()> {
    let shell = shell
        .or_else(|| std::env::var("SHELL").ok())
        .unwrap_or_default();
    match crate::shell::init(&shell) {
        Some(text) => {
            print!("{text}");
            Ok(())
        }
        None => anyhow::bail!("{}", crate::shell::unknown(&shell)),
    }
}

/// Refuse a capture that could not read every line tmux gave it.
///
/// Restoring is a deliberate act, so saving is held to the same bargain: what
/// lands on disk is the whole session or the file that was already there. A
/// dropped line is one pane or one window missing from a layout that otherwise
/// looks complete and reports success, which is the one failure nobody would
/// notice until they opened the project and a pane was gone.
fn whole_or_nothing(c: &crate::saved::Captured, session: &str) -> anyhow::Result<()> {
    if c.skipped.is_empty() {
        return Ok(());
    }
    anyhow::bail!(
        "could not read {} line{} of {session}, so nothing was saved and the layout on disk \
         is unchanged:\n  {}",
        c.skipped.len(),
        if c.skipped.len() == 1 { "" } else { "s" },
        c.skipped.join("\n  ")
    )
}

/// Capture a session's layout on the way out.
async fn save_before_close(session: &str) -> anyhow::Result<()> {
    let path = project_of(session).await;
    if path.is_empty() {
        anyhow::bail!("no directory for session {session}");
    }
    let c = capture_session(session, &path, true).await?;
    whole_or_nothing(&c, session)?;
    crate::saved::store_rendered(&c.layout, &c.guessed)?;
    Ok(())
}

/// The directory a session belongs to.
///
/// `@tmux-companion-project` is set when the project picker creates a session
/// and it survives a rename, so it answers correctly for a session whose panes
/// have since wandered somewhere else. `#{session_path}` is the fallback, which
/// is right for any session tmux made without this tool.
async fn project_of(session: &str) -> String {
    // `={session}:` and not `={session}`. `display-message -t` takes a pane,
    // and a bare session name is not one, so the session form answers with an
    // empty string rather than an error and the caller reads it as "no
    // project" instead of as a mistake.
    let target = format!("={session}:");
    let tagged = tmux_capture(&[
        "display-message",
        "-p",
        "-t",
        &target,
        "#{@tmux-companion-project}",
    ])
    .await
    .trim()
    .to_string();
    if !tagged.is_empty() {
        return tagged;
    }
    tmux_capture(&["display-message", "-p", "-t", &target, "#{session_path}"])
        .await
        .trim()
        .to_string()
}

/// The session this is running in, and the directory it belongs to.
async fn current_project() -> anyhow::Result<(String, String)> {
    let session = tmux_display("#{session_name}").await;
    if session.is_empty() {
        anyhow::bail!(
            "not inside tmux: run this from a pane in the project's session, \
             or name the project directory"
        );
    }
    let path = project_of(&session).await;
    Ok((session, path))
}

/// The project a `show` or `forget` is about: the directory given, or the
/// session this runs in.
async fn project_named_or_current(dir: Option<String>) -> anyhow::Result<String> {
    let Some(dir) = dir else {
        return Ok(current_project().await?.1);
    };
    let home = std::env::var("HOME").unwrap_or_default();
    let cwd = std::env::current_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    crate::project::resolve_typed(&dir, &cwd, &home)
        .ok_or_else(|| anyhow::anyhow!("no such directory: {dir}"))
}

/// `project save`: capture this session and write it for this project.
async fn run_project_save(with_commands: bool) -> anyhow::Result<()> {
    let (session, path) = current_project().await?;
    let c = capture_session(&session, &path, with_commands).await?;
    whole_or_nothing(&c, &session)?;
    let file = crate::saved::store_rendered(&c.layout, &c.guessed)?;
    println!(
        "saved {} window{} for {}\n  {}",
        c.layout.window.len(),
        if c.layout.window.len() == 1 { "" } else { "s" },
        path,
        file.display()
    );
    if !c.guessed.is_empty() {
        println!(
            "  {} pane{} took its command from the running process, so any arguments are gone",
            c.guessed.len(),
            if c.guessed.len() == 1 { "" } else { "s" }
        );
    }
    Ok(())
}

/// `sessions save`: capture every session as a new generation.
///
/// Client side, like `project save`. The daemon has no reason to be involved:
/// this runs tmux commands and writes files, and putting it behind the socket
/// would mean the answer travelled twice for nobody's benefit.
async fn run_sessions_save(
    skip_pane_history: bool,
    extra_exclude: &[String],
    clean: bool,
) -> anyhow::Result<()> {
    let config = config_or_default();
    let taken = crate::sessions::timer::take_snapshot(
        &config.sessions,
        extra_exclude,
        skip_pane_history,
        clean,
    )
    .await?;

    let snap = &taken.captured.snapshot;
    println!(
        "saved {} session{}, {} window{}, {} pane{}\n  {}",
        snap.session.len(),
        plural(snap.session.len()),
        snap.window_count(),
        plural(snap.window_count()),
        snap.pane_count(),
        plural(snap.pane_count()),
        taken.file.display()
    );
    if taken.history_panes > 0 {
        println!(
            "  {} pane screens, {} lines each",
            taken.history_panes, config.sessions.pane_history_lines
        );
    }
    if !taken.captured.excluded.is_empty() {
        println!(
            "  not saved: {} (excluded)",
            taken.captured.excluded.join(", ")
        );
    }
    if !taken.captured.guessed.is_empty() {
        let n = taken.captured.guessed.len();
        println!(
            "  {n} pane{} took {} command from the running process, so any arguments are gone",
            plural(n),
            if n == 1 { "its" } else { "their" }
        );
    }
    Ok(())
}

/// The flags `sessions resurrect` takes, as one struct so the signature stays
/// under what clippy accepts.
pub struct ResurrectOptions {
    /// Which generation, newest when absent.
    pub stamp: Option<String>,
    /// Only these sessions.
    pub only: Vec<String>,
    /// Never these sessions.
    pub exclude: Vec<String>,
    /// Add what is missing rather than refusing a busy server.
    pub merge: bool,
    /// Print the commands and exit.
    pub dry_run: bool,
    /// Do not stop over panes the table did not claim.
    pub yes: bool,
    /// Leave the server detached.
    pub detach: bool,
    /// The caller attaches, or chose not to, so the restore neither attaches
    /// nor says how to: `sessions start` restoring one session.
    pub hand_back: bool,
}

/// `sessions resurrect`: rebuild a server from a snapshot.
///
/// Answers with the exit code rather than a `Result`, because this is the
/// command a boot script runs with nobody watching: `0` restored, `2` bad
/// arguments, `3` refused because sessions are live, `4` nothing to restore,
/// `1` for everything else.
async fn run_sessions_resurrect(opts: ResurrectOptions) -> i32 {
    match crate::sessions::cli::resurrect(opts).await {
        Ok(code) => code,
        Err(e) => {
            eprintln!("tmux-companion: {e}");
            1
        }
    }
}

/// `shutdown`: stop the daemon and leave tmux alone.
async fn run_daemon_shutdown() -> anyhow::Result<()> {
    if !daemon_is_running().await {
        println!("no daemon running");
        return Ok(());
    }
    stop_the_daemon().await;
    println!("daemon stopped. The next client starts one");
    Ok(())
}

/// `restart`: stop the daemon and start a fresh one.
///
/// The reason this exists on its own is `config.toml`: the daemon reads it once
/// at startup and holds it for its whole life, so editing the file changes
/// nothing until this runs. tmux is not touched.
async fn run_daemon_restart() -> anyhow::Result<()> {
    println!("{}", restart_daemon().await?);
    Ok(())
}

/// The body of `restart`, answering with the line it prints.
///
/// `setup` restarts the daemon after writing config.toml and says so in its
/// footer rather than on a terminal the picker is about to draw over.
pub(crate) async fn restart_daemon() -> anyhow::Result<String> {
    let was = daemon_is_running().await;
    if was {
        stop_the_daemon().await;
    }
    // Any client starts one, and `noop` is the cheapest that does no work.
    // The answer is the point: a daemon that refuses its config dies before
    // it answers, and "restarted" over a dead daemon is the message that
    // sends somebody looking everywhere but the config.
    let resp = crate::client::send(crate::proto::Request::raw("noop", serde_json::Value::Null))
        .await
        .map_err(|e| anyhow::anyhow!("the daemon did not come back: {e}"))?;
    if let Some(why) = resp.error {
        anyhow::bail!("the daemon came back but refused: {why}");
    }
    Ok(format!(
        "daemon {}, now {}",
        if was { "restarted" } else { "started" },
        crate::proto::build_id()
    ))
}

/// Whether anything is listening on the socket.
async fn daemon_is_running() -> bool {
    tokio::net::UnixStream::connect(crate::client::sock_path())
        .await
        .is_ok()
}

/// Ask the daemon to go, and wait for the socket to be released.
async fn stop_the_daemon() {
    let _ = crate::client::send_once(&crate::proto::Request::raw(
        "__shutdown",
        serde_json::Value::Null,
    ))
    .await;
    for _ in 0..40 {
        if !daemon_is_running().await {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
}

/// What `sessions shutdown` and `sessions restart` were asked to do.
pub struct Lifecycle {
    /// Bring the server back afterwards.
    pub restart: bool,
    /// Sessions not to save.
    pub exclude: Vec<String>,
    /// Take the daemon down too.
    pub daemon: bool,
    /// Say what would happen, and do none of it.
    pub dry_run: bool,
}

/// `sessions shutdown` and `sessions restart`.
///
/// Answers with an exit code, like `resurrect`, because these are what a
/// logout hook or a boot script runs with nobody watching.
async fn run_lifecycle(opts: Lifecycle) -> i32 {
    match lifecycle(opts).await {
        Ok(code) => code,
        Err(e) => {
            eprintln!("tmux-companion: {e}");
            1
        }
    }
}

/// The body of [`run_lifecycle`].
async fn lifecycle(opts: Lifecycle) -> anyhow::Result<i32> {
    // Killing the server takes this process's own client with it, and the
    // restore that was meant to follow never runs. There is no flag for this:
    // a terminal outside tmux is the only place it works.
    if std::env::var_os("TMUX").is_some() && !opts.dry_run {
        eprintln!(
            "run this from outside tmux: stopping the server would take this pane with it, \
             and anything after it would never run"
        );
        return Ok(2);
    }

    let live: Vec<String> = tmux_capture(&["list-sessions", "-F", "#{session_name}"])
        .await
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect();
    // A refusal, so it goes to stderr like the one above: stdout is for what
    // the command did, and a script reading it should get nothing here.
    if live.is_empty() {
        if opts.restart {
            eprintln!("no tmux server running; use sessions resurrect");
        } else {
            eprintln!("no tmux server running, nothing to stop");
        }
        return Ok(4);
    }

    // Name what is being dropped before anything happens to it. `--exclude`
    // here is not "leave it alone": the server takes every session with it
    // either way, so an excluded one is one that does not come back.
    let dropped: Vec<&String> = live.iter().filter(|s| opts.exclude.contains(s)).collect();
    for name in &dropped {
        println!("not saving: {name} (excluded) -- it will not come back");
    }

    let what = if opts.restart { "restart" } else { "shutdown" };
    if opts.dry_run {
        println!(
            "would save {} session{}",
            live.len() - dropped.len(),
            plural(live.len() - dropped.len())
        );
        println!("would stop the tmux server");
        if opts.daemon {
            println!("would stop the daemon, so it rereads config.toml");
        }
        if opts.restart {
            println!("would start a server and restore what was saved");
        }
        return Ok(0);
    }

    run_sessions_save(false, &opts.exclude, true).await?;
    println!("stopping the tmux server");
    let _ = crate::tmux::command().arg("kill-server").status().await;
    if opts.daemon {
        stop_the_daemon().await;
        println!("daemon stopped");
    }
    if !opts.restart {
        println!("{what} done. `tmux-companion sessions resurrect` brings it back");
        return Ok(0);
    }

    crate::sessions::cli::resurrect(ResurrectOptions {
        stamp: None,
        only: Vec::new(),
        exclude: Vec::new(),
        merge: false,
        dry_run: false,
        yes: false,
        detach: false,
        hand_back: false,
    })
    .await
}

/// `sessions autosave`: run the daemon's snapshot once, or say when it last ran.
///
/// With neither flag it reports, the same as `--status`: the timer lives in
/// the daemon and there is no third thing for the bare command to do, and a
/// sentence saying so was one more thing to read before typing the flag.
async fn run_sessions_autosave(once: bool, status: bool) -> anyhow::Result<()> {
    let config = config_or_default();
    if status || !once {
        let state_dir =
            crate::server::state_dir().ok_or_else(|| anyhow::anyhow!("no state directory"))?;
        match crate::sessions::store::generations_in(&state_dir).first() {
            Some(newest) => println!("last snapshot: {}", newest.stamp),
            None => println!("no snapshots yet"),
        }
        println!(
            "autosave: {}",
            match config.sessions.autosave {
                crate::config::SessionsAutosave::Off => "off".to_string(),
                crate::config::SessionsAutosave::Interval =>
                    format!("every {} seconds", config.sessions.interval_secs),
                crate::config::SessionsAutosave::Cron => format!("cron {}", config.sessions.cron),
            }
        );
        // The `crashed` marker, which a live daemon never sets and a restore
        // removes once it has gone ahead, so this is about the daemon before
        // the current one and only until somebody has restored from it.
        println!(
            "last daemon: {}",
            if crate::sessions::cli::after_a_crash() {
                "did not stop cleanly, and no restore has acted on it yet"
            } else {
                "stopped cleanly, or a restore has already acted on the crash"
            }
        );
        return Ok(());
    }
    run_sessions_save(false, &[], false).await
}

/// `sessions list`: every generation, newest first.
fn run_sessions_list(json: bool) -> anyhow::Result<()> {
    let state_dir = crate::server::state_dir()
        .ok_or_else(|| anyhow::anyhow!("no state directory: neither XDG_STATE_HOME nor HOME"))?;
    let generations = crate::sessions::store::generations_in(&state_dir);
    let last = crate::sessions::store::last_stamp_in(&state_dir);

    if json {
        let rows: Vec<serde_json::Value> = generations
            .iter()
            .map(|g| {
                let snap = crate::sessions::store::load_in(&state_dir, &g.stamp).ok();
                serde_json::json!({
                    "stamp": g.stamp,
                    "newest": Some(g.stamp.clone()) == last,
                    "captured_at": snap.as_ref().map(|s| s.header.captured_at.clone()),
                    "clean": snap.as_ref().map(|s| s.header.clean),
                    "hostname": snap.as_ref().map(|s| s.header.hostname.clone()),
                    "sessions": snap.as_ref().map(|s| s.session.len()),
                    "windows": snap.as_ref().map(|s| s.window_count()),
                    "panes": snap.as_ref().map(|s| s.pane_count()),
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&rows)?);
        return Ok(());
    }

    if generations.is_empty() {
        println!("no snapshots yet. `tmux-companion sessions save` takes one");
        return Ok(());
    }
    // The newest is the one a resurrect takes, so its mark is the accent; a
    // snapshot taken at a clean shutdown is the one that is whole, so it
    // says so in green, and one nobody can read says so in red.
    use crate::picker::Tone;
    let paint = crate::picker::Paint::for_stdout(false);
    for g in &generations {
        let mark = if Some(g.stamp.clone()) == last {
            paint.ink("*", Tone::Accent)
        } else {
            " ".to_string()
        };
        let stamp = paint.ink(&g.stamp, Tone::Strong);
        match crate::sessions::store::load_in(&state_dir, &g.stamp) {
            Ok(snap) => println!(
                "{mark} {stamp}  {} session{}, {} pane{}  {}  {}{}",
                snap.session.len(),
                plural(snap.session.len()),
                snap.pane_count(),
                plural(snap.pane_count()),
                if !snap.header.imported_from.is_empty() {
                    paint.ink("imported", Tone::Plain)
                } else if snap.header.clean {
                    paint.ink("taken at shutdown", Tone::Ok)
                } else {
                    paint.ink("taken while running", Tone::Plain)
                },
                paint.ink(&snap.header.captured_at, Tone::Dim),
                // Where an import came from, since its stamp is when it
                // landed here and says nothing about the machine it left.
                if snap.header.imported_from.is_empty() {
                    String::new()
                } else {
                    format!("  from {}", snap.header.imported_from)
                }
            ),
            Err(e) => println!(
                "{mark} {stamp}  {}",
                paint.ink(&format!("unreadable: {e}"), Tone::Failed)
            ),
        }
    }
    Ok(())
}

/// `sessions show`: what one generation holds.
fn run_sessions_show(stamp: Option<String>, json: bool) -> anyhow::Result<()> {
    let state_dir = crate::server::state_dir()
        .ok_or_else(|| anyhow::anyhow!("no state directory: neither XDG_STATE_HOME nor HOME"))?;
    let snap = match &stamp {
        Some(s) => crate::sessions::store::load_in(&state_dir, s)?,
        None => crate::sessions::store::load_last_in(&state_dir)?,
    };

    if json {
        println!("{}", serde_json::to_string_pretty(&snap)?);
        return Ok(());
    }

    println!(
        "{}  {}  {}",
        snap.header.captured_at,
        if snap.header.clean {
            "taken at shutdown"
        } else {
            "taken while running"
        },
        snap.header.hostname
    );
    println!(
        "  {} on {}, attaching to {}",
        snap.header.companion_version,
        snap.header.tmux_version,
        snap.attach_target().unwrap_or("nothing")
    );
    for session in &snap.session {
        println!("{}  {}", session.name, session.path);
        for window in &session.window {
            println!(
                "  {}:{}{}{}",
                window.index,
                window.name,
                if window.active { " *" } else { "" },
                if window.zoomed { " zoomed" } else { "" }
            );
            for pane in &window.pane {
                let what = if pane.command.is_empty() {
                    "a shell".to_string()
                } else {
                    format!("{} ({:?})", pane.command, pane.confidence)
                };
                println!("    {}. {what}  {}", pane.index, pane.cwd);
            }
        }
    }
    Ok(())
}

/// `s` when there is more than one of something, for a sentence that counts.
pub(crate) fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

/// Ask tmux what a session looks like right now.
async fn capture_session(
    session: &str,
    path: &str,
    with_commands: bool,
) -> anyhow::Result<crate::saved::Captured> {
    let target = format!("={session}");
    let windows = tmux_capture_checked(&[
        "list-windows",
        "-t",
        &target,
        "-F",
        "#{window_index}\t#{window_name}\t#{window_width}\t#{window_height}\t#{window_layout}",
    ])
    .await?;
    let panes = tmux_capture_checked(&[
        "list-panes",
        "-s",
        "-t",
        &target,
        "-F",
        crate::saved::PANE_FORMAT,
    ])
    .await?;

    // A scratch shell is not part of what a project opens with: the pocket
    // window and any pocket that is out are taken off both listings. A
    // listing that cannot be read leaves the window's name to decide.
    let marks = tmux_capture(&[
        "list-panes",
        "-s",
        "-t",
        &target,
        "-F",
        &format!(
            "#{{window_index}}\t#{{pane_index}}\t#{{window_name}}\t#{{{}}}",
            crate::pocket::MARK
        ),
    ])
    .await;
    let (windows, panes) = crate::pocket::without_pocket(&windows, &panes, &marks);

    // A server with this set reports it as the start command of every pane
    // nobody gave a command to, so the capture needs it to tell a wrapped shell
    // from a command somebody typed. Reading it is best effort: an old tmux or
    // a server that answers nothing leaves the capture on the shape of the
    // start command, which is the case it had before this was read at all.
    let default_command = tmux_capture(&["show-options", "-gv", "default-command"])
        .await
        .trim()
        .to_string();

    let home = std::env::var("HOME").unwrap_or_default();
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
    let at = crate::tasks::format_unix(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0),
    );
    let real = std::fs::canonicalize(path)
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| path.to_string());
    // The arguments of what each pane runs, which tmux has no format for. Only
    // read when commands are being recorded; an unreadable table leaves each
    // pane to its process name.
    let processes = if with_commands {
        match tokio::process::Command::new("ps")
            .args(crate::sessions::capture::PS_ARGS)
            .output()
            .await
        {
            Ok(o) => String::from_utf8_lossy(&o.stdout).into_owned(),
            Err(_) => String::new(),
        }
    } else {
        String::new()
    };
    Ok(crate::saved::capture(&crate::saved::Capture {
        windows: &windows,
        panes: &panes,
        project: path,
        project_real: &real,
        shell: &shell,
        default_command: &default_command,
        home: &home,
        at: &at,
        with_commands,
        processes: &processes,
    }))
}

/// `project forget`: drop this project's saved layout.
async fn run_project_forget(dir: Option<String>) -> anyhow::Result<()> {
    let path = project_named_or_current(dir).await?;
    if crate::saved::forget(&path)? {
        println!("forgot the saved layout for {path}");
    } else {
        println!("no saved layout for {path}");
    }
    Ok(())
}

/// A project's saved layout, with the file that is there but not used kept
/// apart from no file at all, so `project show` can say which.
async fn load_saved(path: &str) -> crate::saved::SavedFile {
    let default_command = tmux_capture(&["show-options", "-gv", "default-command"]).await;
    crate::saved::load_checked(path, &default_command)
}

/// `project show`: which layout this project gets, and which file decided.
async fn run_project_show(dir: Option<String>) -> anyhow::Result<()> {
    let path = project_named_or_current(dir).await?;
    let config = config_or_default();
    let home = std::env::var("HOME").unwrap_or_default();
    let file = load_saved(&path).await;
    let ignored = file
        .ignored()
        .map(|(f, w)| (f.to_path_buf(), w.to_string()));
    let repo = crate::repofile::load(&path);
    let (windows, source) = crate::saved::resolve(&config, file.layout(), &repo, &path, &home);
    print!(
        "{}",
        crate::saved::describe(
            &path,
            &windows,
            &source,
            &home,
            ignored.as_ref().map(|(f, w)| (f.as_path(), w.as_str())),
            repo.ignored(),
        )
    );
    Ok(())
}

/// One tmux command, ignoring a failure.
///
/// Each of these is a step in building a session, and a step that fails should
/// cost its own window rather than leaving half a session and an error.
/// The pane this process is running in, when tmux put it in one.
///
/// Every `tmux` subcommand here used to run unanchored, and tmux then resolves
/// "current" as the most recently used session on the server. That is the
/// right answer only while there is one session. With a second one touched
/// more recently, `open` split a pane in a window nobody was looking at and
/// `display-message -p '#{pane_current_path}'` answered for it too, so the
/// editor opened on a path from another project. tmux sets `$TMUX_PANE` in
/// every pane and in every binding it runs, so there is an exact answer
/// available and no reason to guess.
pub(crate) fn pane_target() -> Option<String> {
    match std::env::var("TMUX_PANE") {
        Ok(p) if !p.trim().is_empty() => Some(p),
        _ => None,
    }
}

pub(crate) async fn tmux(args: &[&str]) {
    let _ = crate::tmux::command().args(args).status().await;
}

/// `switch windows`: a press from `run-shell`, the list in a popup, or the
/// order printed; [`crate::switcher`] has the rule.
///
/// Which of the first two it is follows from whether there is a terminal:
/// `run-shell` has none and a popup is one.
async fn run_switch_windows(
    session: Option<String>,
    client: Option<String>,
    reverse: bool,
    print: bool,
    pressed: bool,
) -> anyhow::Result<()> {
    use std::io::IsTerminal;
    if print {
        return crate::switcher::print(session, client).await;
    }
    let config = config_or_default();
    if std::io::stdout().is_terminal() {
        crate::switcher::list(
            session,
            client,
            reverse,
            pressed,
            &config.switcher,
            &config.picker,
        )
        .await
    } else {
        crate::switcher::press(session, client, reverse, &config.switcher).await
    }
}

/// `switch sessions`: the project picker opened on the sessions alone, most
/// recently used first, with the cursor on the one you were in before.
///
/// The session key moves the cursor, with shift it moves back, and enter
/// goes; there is no clock. The directories are in the list from the start
/// and hidden until something is typed, so the first letter is the project
/// picker's search and a typed path opens as it does there.
async fn run_switch_sessions(reverse: bool, print: bool, at: Option<String>) -> anyhow::Result<()> {
    let config = config_or_default();
    let home = std::env::var("HOME").unwrap_or_default();
    let settings = &config.switcher;
    let key = settings.sessions_key;
    let upper = key.to_ascii_uppercase();
    let mut at = at;
    let mut start_index: Option<usize> = None;

    loop {
        // The sessions alone before the first frame: one tmux call, the hooks
        // chained in front, and one question to a daemon already running for
        // each host's rows, which it answers from memory or its cache on disk
        // and which asks it to bring each on-demand link up afterwards. The
        // directories are hidden until a letter is typed, so collecting them
        // (the source's process and every theme file) waits until the frame
        // is up; see `picker::Later`.
        let mut sessions = crate::project::sessions(&crate::switcher::hooks()).await;
        let views = crate::remote::views_for_picker();
        crate::remote::host_paths(&mut sessions, &views);
        // Inside a mirror session a typed path is the host's: its
        // directories are listed and a path opens there.
        let here = here_host(&sessions);
        let now = crate::sessions::idle::now();
        let open = crate::remote::open_mirrors(&sessions);
        let remote_rows = crate::remote::rows(&views, &open, now);

        if print {
            for r in &sessions {
                println!("{}", r.label);
            }
            for line in crate::remote::print_lines(&views, &open, now) {
                println!("{line}");
            }
            return Ok(());
        }

        let remote_rows = crate::remote::for_picker(remote_rows, &open);
        let mut footer = format!(
            "enter go   alt-{key} next   alt-{upper} back   esc stay   type to filter session, **concealed** projects"
        );
        if !views.is_empty() {
            footer.push_str("   ctrl-o new on host   ctrl-x forget   ctrl-n add host");
            footer.push_str(crate::remote::alt_enter_hint(&views));
        }
        let chrome = crate::picker::Chrome {
            title: "[ Sessions/Projects ]".into(),
            icon: crate::tmux::icons::SESSION.into(),
            footer,
            preview_title: "[ Where ]".into(),
            ..Default::default()
        }
        .configured(&config.picker, crate::config::Picker::Project);
        // The shape is the project picker's, the name is this list's own: a
        // `label` under [picker.project] names the project picker, and this
        // one is the sessions first and the projects behind a letter.
        let chrome = crate::picker::Chrome {
            title: "[ Sessions/Projects ]".into(),
            // Sessions by last use, then projects by last jump, typed or not.
            keep_order: true,
            ..chrome
        };

        let mut items = project_items(&sessions, &config, &home);
        // The session you are in says so in words, not only by being first.
        if let Some(first) = items.first_mut() {
            first.columns.push("now".into());
            first.tones.push(crate::picker::Tone::Dim);
        }
        let n_local = sessions.len();
        let n_remote = remote_rows.len();
        items.extend(crate::remote::items(&remote_rows, &views, now));

        // The directories' rows, kept for after the pick: an index past the
        // sessions and the hosts is one of them.
        let directories = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let later = {
            let (config, home, sessions) = (config.clone(), home.clone(), sessions.clone());
            let (here, views) = (here.clone(), views.clone());
            let keep = directories.clone();
            crate::picker::Later::new(move || {
                let Ok(rt) = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                else {
                    return Vec::new();
                };
                if let Some(host) = &here {
                    // The host's directories, through its link; a typed path
                    // opens there whether or not it is listed.
                    let rows = match rt.block_on(crate::remote::host_dirs(host)) {
                        Ok(paths) => crate::remote::dir_rows(host, &paths, &views),
                        Err(_) => Vec::new(),
                    };
                    let items: Vec<crate::picker::Item> = project_items(&rows, &config, &home)
                        .into_iter()
                        .map(crate::picker::Item::searched_only)
                        .collect();
                    if let Ok(mut k) = keep.lock() {
                        *k = rows;
                    }
                    return items;
                }
                let mut found = rt.block_on(crate::project::directories(&config, &home));
                // Projects by when they were last jumped to, as the sessions
                // are by when they were last used. Only zoxide keeps that
                // time; any other source, or a db.zo this can't read, keeps
                // the source's own order.
                let source = crate::dirsource::DirsSource::from_config(&config.project);
                if let Some(last) = source
                    .watched_file(&home)
                    .and_then(|f| std::fs::read(f).ok())
                    .and_then(|db| crate::dirsource::zoxide_last_jumped(&db))
                {
                    found.sort_by_key(|r| {
                        std::cmp::Reverse(last.get(&r.path).copied().unwrap_or(0))
                    });
                }
                // A directory a session already stands for is left out, as
                // `merge` leaves it out of the project picker. Not through
                // `merge` itself: it would drop a second session in the same
                // directory as well, and the sessions are already listed.
                let taken: std::collections::HashSet<String> = sessions
                    .iter()
                    .filter(|r| r.mirror.is_none())
                    .map(|r| r.path.clone())
                    .collect();
                let rows = crate::project::merge(
                    Vec::new(),
                    found
                        .into_iter()
                        .filter(|r| !taken.contains(&r.path))
                        .collect(),
                );
                let items: Vec<crate::picker::Item> = project_items(&rows, &config, &home)
                    .into_iter()
                    .map(crate::picker::Item::searched_only)
                    .collect();
                if let Ok(mut k) = keep.lock() {
                    *k = rows;
                }
                items
            })
        };

        // Where a step can land: every local session, and each remote
        // session whose host can be reached. The start rule, the second
        // row, counts these alone.
        let steps: Vec<usize> = (0..n_local)
            .chain(
                remote_rows
                    .iter()
                    .enumerate()
                    .filter(|(_, r)| {
                        matches!(
                            r.target,
                            crate::remote::Target::Session {
                                reachable: true,
                                ..
                            }
                        )
                    })
                    .map(|(i, _)| n_local + i),
            )
            .collect();
        let ids: Vec<String> = sessions
            .iter()
            .map(|r| format!("local:{}", r.label))
            .chain(remote_rows.iter().map(|r| r.target.id()))
            .collect();
        let start = match (
            at.take().and_then(|id| ids.iter().position(|x| *x == id)),
            start_index.take(),
        ) {
            (Some(i), _) => i,
            (None, Some(i)) => i.min(items.len().saturating_sub(1)),
            (None, None) => match (steps.len(), reverse) {
                (0, _) => 0,
                (1, _) => steps[0],
                (_, false) => steps[1],
                (n, true) => steps[n - 1],
            },
        };
        let on_step = settings.announce.then(|| {
            let mut names: Vec<Option<String>> =
                sessions.iter().map(|r| Some(r.label.clone())).collect();
            names.extend(remote_rows.iter().map(|r| match &r.target {
                crate::remote::Target::Session {
                    host,
                    name,
                    reachable: true,
                } => Some(format!("{name} on {host}")),
                _ => None,
            }));
            let lines: Vec<String> = remote_rows.iter().map(|r| r.line.clone()).collect();
            let steps = steps.clone();
            crate::picker::OnStep::new(move |i| {
                // A directory is reached by searching, and is not a step.
                let text = match (names.get(i), steps.iter().position(|s| *s == i)) {
                    (Some(Some(name)), Some(at)) => {
                        crate::switcher::announcement(name, at + 1, steps.len())
                    }
                    // A group line or an unreachable row, where a reopened
                    // picker can start: its words, which say why.
                    (Some(None), _) => match lines.get(i - n_local) {
                        Some(l) => l.clone(),
                        None => return,
                    },
                    _ => return,
                };
                let _ = crate::tmux::command_sync()
                    .args(["display-message", &crate::picker::literal_for_tmux(&text)])
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn();
            })
        });
        // alt-enter on a remote session does the other thing with it: hand
        // the terminal over where the host mirrors, mirror where it hands
        // over. Anywhere else it takes the query, as in any picker.
        let alt_enter: Vec<usize> = remote_rows
            .iter()
            .enumerate()
            .filter(|(_, r)| {
                matches!(
                    r.target,
                    crate::remote::Target::Session {
                        reachable: true,
                        ..
                    }
                )
            })
            .map(|(i, _)| n_local + i)
            .collect();
        // `name@vm` that no row is: a row saying it makes that session on
        // vm, first, so a loose match below can't take Enter. `ab@vm` was a
        // fuzzy match for `ghsa-xm5q-backstage@vm` and opened that instead.
        let create = {
            let hosts: Vec<String> = views.iter().map(|v| v.name.clone()).collect();
            let taken: std::collections::HashSet<String> = sessions
                .iter()
                .map(|r| r.label.clone())
                .chain(remote_rows.iter().filter_map(|r| match &r.target {
                    crate::remote::Target::Session { host, name, .. } => {
                        Some(crate::link::mirror::session_name(name, host))
                    }
                    _ => None,
                }))
                .collect();
            (!hosts.is_empty()).then(|| {
                crate::picker::Create::new(move |q| {
                    let (name, host) = crate::remote::split_at_host(q, &hosts)?;
                    let made = crate::link::mirror::session_name(name, host);
                    (!taken.contains(&made)).then(|| {
                        format!(
                            "new session {} on {host}",
                            crate::sessions::start::tmux_name(name)
                        )
                    })
                })
            })
        };
        let chrome = crate::picker::Chrome {
            steps: vec![(key, 1), (upper, -1)],
            on_step,
            alt_enter,
            create,
            ..chrome
        };

        let (outcome, pressed) =
            crate::picker::run_from_keys(items, "", &chrome, start, Some(later), &['x', 'n', 'o'])?;
        let mut rows = sessions.clone();
        if let Ok(mut d) = directories.lock() {
            rows.append(&mut d);
        }
        let remote_at = |i: usize| (n_local..n_local + n_remote).contains(&i);

        if let (crate::picker::Outcome::Chosen(i), Some('\r')) = (&outcome, pressed)
            && remote_at(*i)
            && let crate::remote::Target::Session { host, name, .. } =
                &remote_rows[*i - n_local].target
        {
            return crate::remote::open_other(host, name).await;
        }
        if let (crate::picker::Outcome::Chosen(i), Some(k)) = (&outcome, pressed) {
            let i = *i;
            match (k, remote_at(i)) {
                ('x', false) if i < n_local => {
                    crate::remote::say(&format!(
                        "{} is on this machine; only remote rows can be forgotten",
                        sessions[i].label
                    ))
                    .await;
                    start_index = Some(i);
                }
                ('x', true) => match &remote_rows[i - n_local].target {
                    crate::remote::Target::Session { host, name, .. } => {
                        crate::remote::forget_row(host, name).await;
                        start_index = Some(i);
                    }
                    crate::remote::Target::Group(host) => {
                        if let Some((_, r)) = open.iter().find(|(h, _)| h == host) {
                            crate::remote::say(&format!(
                                "{} is open; close it first",
                                crate::link::mirror::session_name(r, host)
                            ))
                            .await;
                            start_index = Some(i);
                        } else {
                            return crate::hosts::ask_to_forget(host).await;
                        }
                    }
                },
                // A host's row: its line, one of its sessions, or a mirror
                // session open here, since the line is left out once a
                // session stands for the host.
                ('n', _) if remote_at(i) || sessions.get(i).is_some_and(|r| r.mirror.is_some()) => {
                    let said = crate::hosts::form().await?;
                    if let Some(said) = said {
                        crate::remote::say(&said).await;
                    }
                    start_index = Some(i);
                }
                ('n', _) => {
                    crate::remote::say(
                        "ctrl-n on a host's row adds a host; tmux-companion hosts add does it from anywhere",
                    )
                    .await;
                    start_index = Some(i);
                }
                // A new session on the row's host: its name, then the
                // directory there, each in a picker in this popup. Its line,
                // one of its sessions, or a mirror session open here.
                ('o', true) => {
                    let host = remote_rows[i - n_local].target.host().to_string();
                    return crate::remote::new_session(&host, None, &views).await;
                }
                ('o', false)
                    if let Some((host, _)) = sessions.get(i).and_then(|r| r.mirror.clone()) =>
                {
                    return crate::remote::new_session(&host, None, &views).await;
                }
                ('o', _) => {
                    crate::remote::say(
                        "ctrl-o on a host's row starts a session there; type name@host for one by name",
                    )
                    .await;
                    start_index = Some(i);
                }
                _ => {
                    crate::remote::say(
                        "ctrl-x forgets a remote row or a host; a directory is not kept to forget",
                    )
                    .await;
                    start_index = Some(i);
                }
            }
            continue;
        }

        if let crate::picker::Outcome::Chosen(i) = outcome
            && remote_at(i)
        {
            return match &remote_rows[i - n_local].target {
                crate::remote::Target::Group(host)
                | crate::remote::Target::Session {
                    host,
                    reachable: false,
                    ..
                } => {
                    crate::remote::retry(host).await;
                    Ok(())
                }
                crate::remote::Target::Session { host, name, .. } => {
                    crate::remote::open(host, name, false, false).await
                }
            };
        }
        // A typed `name@vm` with no row for it, or Enter on the create row,
        // makes the session on the host and opens it, once a picker has
        // asked where on the host it starts.
        if let crate::picker::Outcome::Typed(q) = &outcome {
            let hosts: Vec<String> = views.iter().map(|v| v.name.clone()).collect();
            if let Some((name, host)) = crate::remote::split_at_host(q, &hosts) {
                return crate::remote::new_session(host, Some(name), &views).await;
            }
        }
        let outcome = match outcome {
            crate::picker::Outcome::Chosen(i) if i >= n_local + n_remote => {
                crate::picker::Outcome::Chosen(i - n_remote)
            }
            o => o,
        };
        let cwd = std::env::current_dir()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        return match project_pick(&outcome, &rows, &cwd, &home, here.as_deref()) {
            ProjectPick::Session(name) => focus_session(&name).await,
            ProjectPick::Directory(path) => open_project(&path, &config, &home).await,
            ProjectPick::HostDirectory(host, path) => {
                crate::remote::open_project(&host, &path).await
            }
            ProjectPick::Missing(typed) => {
                crate::picker::say_nothing_to_show(&format!("no such directory: {typed}"), false)
                    .await;
                Ok(())
            }
            ProjectPick::Nothing => Ok(()),
        };
    }
}

/// `autosave`: the timer lives in the daemon, so this is the manual half.
///
/// Neither flag reports, like `--status`. The bare command used to print a
/// sentence saying where the loop lives, which was there to stop anybody
/// starting a second one, but a sentence about flags is not what somebody
/// who typed the command wanted to know, and the answer to "when did it last
/// save" is.
async fn run_autosave(once: bool, status: bool) -> anyhow::Result<()> {
    if status || !once {
        println!("{}", crate::tasks::last_save());
        return Ok(());
    }
    let config = config_or_default();
    let home = std::env::var("HOME").unwrap_or_default();
    crate::tasks::save_now(&config.autosave.script_path(&home)).await
}

/// One `tmux display-message -p`, empty when tmux is not there.
pub(crate) async fn tmux_display(format: &str) -> String {
    tmux_display_at(None, format).await
}

/// `display-message -p`, answering for one pane rather than for the server's
/// idea of the current one.
///
/// `target` wins when it is given; otherwise this falls back to `$TMUX_PANE`,
/// and to tmux's own current pane when there is neither.
pub(crate) async fn tmux_display_at(target: Option<&str>, format: &str) -> String {
    if let Some(pane) = target.map(str::to_string).or_else(pane_target) {
        let answered = display_message(&["-t", &pane, format]).await;
        if !answered.is_empty() {
            return answered;
        }
        // The pane did not answer, so it is not a pane on this server. That is
        // an ordinary thing and not an error: `run-shell` does not set
        // `$TMUX_PANE`, it passes whatever the server inherited when it
        // started, which can be a pane id from a tmux the user has since
        // closed. Targeting it made `project save` report "not inside tmux"
        // from inside tmux, every time it was run from its binding. Asking
        // again without a target is what this did before it asked at all.
    }
    display_message(&[format]).await
}

/// `display-message -p`, or the empty string if tmux would not answer.
async fn display_message(args: &[&str]) -> String {
    let mut argv: Vec<&str> = vec!["display-message", "-p"];
    argv.extend_from_slice(args);
    match crate::tmux::command().args(&argv).output().await {
        Ok(o) => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        Err(_) => String::new(),
    }
}

/// `run`: pick a command from history and run it in a pane beside this one.
async fn run_command(
    print: bool,
    pane: Option<String>,
    exec: Option<String>,
) -> anyhow::Result<()> {
    let config = config_or_default();
    let home = std::env::var("HOME").unwrap_or_default();

    // The pane half: this process *is* the new pane, so it runs the command
    // here rather than asking tmux to.
    if let Some(command) = exec {
        return run_in_this_pane(&command, &config).await;
    }

    let commands = crate::run::history(&config.run, &home).await;

    if print {
        for c in &commands {
            println!("{c}");
        }
        return Ok(());
    }

    let items: Vec<crate::picker::Item> = commands
        .iter()
        .map(|c| {
            crate::picker::Item::new(c.clone())
                .with_icon(crate::tmux::icons::HISTORY, crate::picker::Tone::Dim)
        })
        .collect();

    let chrome = crate::picker::Chrome {
        title: "[ Run command ]".into(),
        icon: crate::tmux::icons::RUN.into(),
        footer: "enter run beside   alt-enter run typed   esc cancel".into(),
        preview_title: String::new(),
        ..Default::default()
    }
    .configured(&config.picker, crate::config::Picker::Run);

    // The typed query is the command when nothing matched, which is the only
    // way to run something that was never in the history.
    let picked = crate::picker::run_with_query(items, "", &chrome)?;
    let command = match picked {
        crate::picker::Outcome::Chosen(i) => commands.get(i).cloned(),
        crate::picker::Outcome::Typed(q) if !q.trim().is_empty() => Some(q),
        _ => None,
    };
    let Some(command) = command else {
        return Ok(());
    };

    let exe = std::env::current_exe()?;
    // No `--pane` means an older binding, or somebody typing this at a shell.
    // The shell case is fine and tmux resolves it; the popup case is not, so
    // the session an attached client is looking at stands in for the pane.
    let pane = match pane {
        Some(p) if !p.is_empty() => Some(p),
        _ => attached_session().await.map(|s| format!("{s}:")),
    };
    let width = crate::run::pane_width(
        window_width(pane.as_deref()).await,
        config.run.width_percent,
    );
    let opening = if config.run.slide_steps > 0 { 1 } else { width };

    // The window as it is now, so the side pane can put it back when it
    // closes; see `restore_args`.
    let saved = tmux_display_at(pane.as_deref(), "#{window_id}\t#{window_layout}").await;
    let args = crate::run::split_args(
        pane.as_deref(),
        opening,
        &format!("{} run --exec {}", exe.display(), shell_quote(&command)),
        Some(&saved),
    );
    tmux(&args.iter().map(String::as_str).collect::<Vec<_>>()).await;
    Ok(())
}

/// Quote a command so tmux hands it back to us whole.
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// Open a target with an application picked from the configured list.
async fn open_with_chosen(
    target: &crate::open::Target,
    config: &crate::config::Config,
    pane: Option<&str>,
) -> anyhow::Result<()> {
    use crate::open::Target;

    let (text, line, column) = match target {
        Target::Url(u) => (u.clone(), 0, 0),
        Target::File { path, line, column } => {
            (path.display().to_string(), *line as usize, *column as usize)
        }
    };

    let items: Vec<crate::picker::Item> = config
        .open
        .applications
        .iter()
        .map(|a| {
            crate::picker::Item::with_preview(
                a.name.clone(),
                crate::open::application_command(&a.command, &text, line, column),
            )
            .in_cells(vec![crate::picker::Cell::strong(a.name.clone())])
        })
        .collect();

    let chrome = crate::picker::Chrome {
        title: "[ Open with ]".into(),
        icon: crate::tmux::icons::RUN.into(),
        footer: "enter open   esc cancel".into(),
        preview_title: "[ What it runs ]".into(),
        ..Default::default()
    }
    .configured(&config.picker, crate::config::Picker::Open);

    let Some(index) = crate::picker::run(items, "", &chrome)? else {
        return Ok(());
    };
    let chosen = &config.open.applications[index];
    let command = crate::open::application_command(&chosen.command, &text, line, column);

    if chosen.pane {
        let args = crate::open::split_window_args(&config.open, command, pane);
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        tmux(&args).await;
        return Ok(());
    }

    // Launched and left alone, which is what a browser wants. Through the
    // shell because the template is where the quoting lives: an application
    // name with a space in it only reaches `-a` intact if somebody wrote the
    // quotes, and an argument vector built here would have to guess.
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "sh".to_string());
    let _ = tokio::process::Command::new(shell)
        .args(["-c", &command])
        .status()
        .await;
    Ok(())
}

/// The session an attached client is looking at, if there is one.
///
/// `list-clients` rather than `display-message`, because the caller is a popup
/// and a popup is not a client: every untargeted question it asks tmux is
/// answered for whichever session the server touched last, which after a
/// project switch is a session nobody is looking at. With more than one client
/// attached this takes the first, which is the same guess tmux itself makes.
async fn attached_session() -> Option<String> {
    attached_client().await.map(|(_, session)| session)
}

/// The first attached client, as its name and the session it is looking at.
///
/// The name is what `switch-client -c` takes, and it is the client to move
/// when the mover is a popup: `panes` needs it for the same reason
/// `attached_session` exists.
pub(crate) async fn attached_client() -> Option<(String, String)> {
    let out = crate::tmux::command()
        .args(["list-clients", "-F", "#{client_name}\t#{client_session}"])
        .output()
        .await
        .ok()?;
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .find_map(|l| {
            let (name, session) = l.split_once('\t')?;
            Some((name.to_string(), session.to_string()))
        })
}

/// The attached client's session as a tmux target, `name:`.
///
/// Everything drawn in a `display-popup` needs this. A popup is not a client,
/// so `#{session_name}`, `#{pane_current_path}` and an untargeted
/// `new-window` are all answered for whichever session the server touched
/// last, and the answer is right up until somebody switches project -- which
/// is the one thing this tool is for.
async fn client_target() -> Option<String> {
    attached_session().await.map(|s| format!("{s}:"))
}

/// The width of the window a pane is in, or of whatever tmux calls current.
///
/// The argument is why this takes one at all: the picker runs inside
/// `display-popup -E`, and a popup is not a client, so tmux resolves an
/// untargeted `#{window_width}` against the session it touched most recently
/// rather than the one on screen.
async fn window_width(pane: Option<&str>) -> u16 {
    tmux_display_at(pane, "#{window_width}")
        .await
        .parse()
        .unwrap_or(180)
}

/// Run the command here, then offer the dialog, repeating on Restart.
async fn run_in_this_pane(command: &str, config: &crate::config::Config) -> anyhow::Result<()> {
    use crate::run::Choice;

    let pane = std::env::var("TMUX_PANE").unwrap_or_default();
    let target = pane.clone();
    let width = crate::run::pane_width(
        window_width(if pane.is_empty() { None } else { Some(&pane) }).await,
        config.run.width_percent,
    );
    slide(&target, 1, width, config).await;

    loop {
        println!("\x1b[2m$ \x1b[0m{command}");
        // Empty means `$SHELL`, which is the default: hardcoding zsh here meant
        // a pane that opened and never ran anything on a machine without it.
        let shell = if config.run.shell.is_empty() {
            std::env::var("SHELL").unwrap_or_else(|_| "sh".to_string())
        } else {
            config.run.shell.clone()
        };
        let status = tokio::process::Command::new(&shell)
            .args(["-ic", command])
            .status()
            .await;
        let code = status.ok().and_then(|s| s.code()).unwrap_or(1);

        match dialog(code, config).await {
            Choice::Close => {
                slide(&target, width, 1, config).await;
                if let Some(args) = std::env::var(crate::run::LAYOUT_ENV)
                    .ok()
                    .and_then(|saved| crate::run::restore_args(&target, &saved))
                {
                    tmux(&args.iter().map(String::as_str).collect::<Vec<_>>()).await;
                }
                return Ok(());
            }
            Choice::Restart => println!(),
            Choice::View => {
                println!(
                    "\x1b[2m-- read-only view: q leaves copy-mode, any key for the dialog --\x1b[0m"
                );
                tmux(&["copy-mode", "-t", &target]).await;
                wait_for_a_key();
            }
        }
    }
}

/// Step a pane's width, so it slides rather than appears.
pub(crate) async fn slide(target: &str, from: u16, to: u16, config: &crate::config::Config) {
    let steps = crate::run::slide_steps(from, to, config.run.slide_steps);
    if steps.is_empty() {
        tmux(&["resize-pane", "-t", target, "-x", &to.to_string()]).await;
        return;
    }
    let gap = std::time::Duration::from_millis(config.run.slide_ms / steps.len().max(1) as u64);
    for w in steps {
        tmux(&["resize-pane", "-t", target, "-x", &w.to_string()]).await;
        tokio::time::sleep(gap).await;
    }
}

/// Ask what to do now the command has exited.
///
/// A popup when one can be opened, and an inline prompt when it cannot: on a
/// tiny or detached client `display-popup` fails, and failing to ask is worse
/// than asking plainly.
async fn dialog(code: i32, config: &crate::config::Config) -> crate::run::Choice {
    match popup_dialog(code).await {
        Some(choice) => choice,
        None => inline_dialog(code, config),
    }
}

/// The dialog as a floating popup over the pane the command ran in.
///
/// `None` when the popup could not be opened at all, which is what happens on
/// a client too small to hold it and on a detached one.
///
/// The answer comes back through a file rather than through the popup's exit
/// status, because `display-popup -E` reports only whether the command
/// succeeded and there are three answers here.
async fn popup_dialog(code: i32) -> Option<crate::run::Choice> {
    let pane = std::env::var("TMUX_PANE").ok()?;
    let geometry = tmux_capture(&[
        "display",
        "-p",
        "-t",
        &pane,
        "#{pane_left} #{pane_top} #{pane_width} #{pane_height}",
    ])
    .await;
    let numbers: Vec<u16> = geometry
        .split_whitespace()
        .filter_map(|n| n.parse().ok())
        .collect();
    let [left, top, width, height] = numbers[..] else {
        return None;
    };
    let (x, y) = crate::run::dialog_at(left, top, width, height);

    let answer = std::env::temp_dir().join(format!("tmux-companion-dialog-{}", std::process::id()));
    let me = std::env::current_exe().ok()?;
    let command = format!(
        "{} run --dialog {code} --out {}",
        shell_quote(&me.display().to_string()),
        shell_quote(&answer.display().to_string())
    );
    let status = crate::tmux::command()
        .args([
            "display-popup",
            "-x",
            &x.to_string(),
            "-y",
            &y.to_string(),
            "-w",
            &crate::run::DIALOG_WIDTH.to_string(),
            "-h",
            &crate::run::DIALOG_HEIGHT.to_string(),
            "-T",
            &crate::run::dialog_title(code),
            "-E",
            &command,
        ])
        .status()
        .await
        .ok()?;
    if !status.success() {
        let _ = std::fs::remove_file(&answer);
        return None;
    }
    let word = std::fs::read_to_string(&answer).unwrap_or_default();
    let _ = std::fs::remove_file(&answer);
    Some(crate::run::choice_of_word(&word))
}

/// `run --dialog`: draw the dialog and write the answer where the caller asked.
fn run_dialog(code: i32, out: Option<String>) -> anyhow::Result<()> {
    let choice = draw_dialog(code)?;
    if let Some(path) = out {
        std::fs::write(path, crate::run::choice_word(choice))?;
    }
    Ok(())
}

/// The dialog itself: a line about what happened and three buttons.
fn draw_dialog(code: i32) -> anyhow::Result<crate::run::Choice> {
    use crate::run::{BUTTONS, Choice, button_label, default_button, step_button};
    use ratatui::{
        crossterm::event::{self, Event, KeyCode, KeyEventKind},
        layout::{Constraint, Direction, Layout},
        text::{Line, Span},
        widgets::Paragraph,
    };

    use crate::picker::Tone;
    let paint = crate::picker::Paint::detect();
    let mut selected = default_button(code);
    let said = if code == 0 {
        Span::styled("Command finished successfully", paint.style(Tone::Ok))
    } else {
        Span::styled(
            format!("Command failed with exit {code}"),
            paint.style(Tone::Failed),
        )
    };

    // The keys sit where every picker's do, from the same setting.
    let look = config_or_default()
        .picker
        .resolved(crate::config::Picker::Run)
        .look;
    let hint = "enter selected   c close   v view   r restart   tab next   esc view";
    let mut terminal = ratatui::init();
    let result = (|| -> anyhow::Result<Choice> {
        loop {
            terminal.draw(|frame| {
                let (area, hint_row, rule_row) =
                    crate::picker::hint_rows(frame.area(), &look, hint);
                if let Some(row) = hint_row {
                    frame.render_widget(
                        Paragraph::new(crate::picker::hint_line(hint, &paint, row.width)),
                        row,
                    );
                }
                if let Some(row) = rule_row {
                    frame.render_widget(
                        Paragraph::new(crate::picker::rule_line(&look, &paint, row.width)),
                        row,
                    );
                }
                let rows = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Length(1),
                        Constraint::Length(1),
                        Constraint::Length(1),
                        Constraint::Min(0),
                    ])
                    .split(area);

                frame.render_widget(
                    Paragraph::new(Line::from(vec![Span::raw("  "), said.clone()])),
                    rows[0],
                );

                let mut buttons = vec![Span::raw("  ")];
                for (i, choice) in BUTTONS.iter().enumerate() {
                    // The band is focus here as it is in every picker.
                    buttons.push(if i == selected {
                        Span::styled(button_label(*choice), paint.on_band(Tone::Strong))
                    } else {
                        Span::styled(button_label(*choice), paint.style(Tone::Dim))
                    });
                    buttons.push(Span::raw("   "));
                }
                // The terminal's cursor on the focused button, which a
                // screen reader follows where it cannot see a band.
                let before: usize = BUTTONS[..selected]
                    .iter()
                    .map(|c| Span::raw(button_label(*c)).width() + 3)
                    .sum();
                let x = rows[2].x + u16::try_from(2 + before).unwrap_or(0);
                frame.set_cursor_position((x.min(rows[2].right().saturating_sub(1)), rows[2].y));
                frame.render_widget(Paragraph::new(Line::from(buttons)), rows[2]);
            })?;

            let Event::Key(key) = event::read()? else {
                continue;
            };
            if key.kind != KeyEventKind::Press {
                continue;
            }
            match key.code {
                KeyCode::Enter => return Ok(BUTTONS[selected]),
                // Bare Esc dismisses to View, which leaves the pane open and
                // read-only: nothing is lost while somebody works out what
                // happened.
                KeyCode::Esc => return Ok(Choice::View),
                KeyCode::Char('c' | 'C' | 'q' | 'Q') => return Ok(Choice::Close),
                KeyCode::Char('v' | 'V') => return Ok(Choice::View),
                KeyCode::Char('r' | 'R') => return Ok(Choice::Restart),
                KeyCode::Right | KeyCode::Tab | KeyCode::Char('l') => {
                    selected = step_button(selected, 1);
                }
                KeyCode::Left | KeyCode::BackTab | KeyCode::Char('h') => {
                    selected = step_button(selected, -1);
                }
                _ => {}
            }
        }
    })();
    ratatui::restore();
    result
}

/// The dialog as a line in the pane, for when a popup cannot be opened.
fn inline_dialog(code: i32, _config: &crate::config::Config) -> crate::run::Choice {
    use crate::run::{Choice, default_choice};
    use std::io::Write;

    let default = default_choice(code);
    let label = if code == 0 {
        "\x1b[32m✔ done\x1b[0m".to_string()
    } else {
        format!("\x1b[31m✘ exit {code}\x1b[0m")
    };
    let hint = match default {
        Choice::Close => "[C]lose/q  [v]iew  [r]estart",
        _ => "[c]lose/q  [v]iew  [R]estart",
    };
    print!("\n  {label}  {hint} ");
    let _ = std::io::stdout().flush();

    match read_choice() {
        Some(c) => c,
        None => default,
    }
}

/// Take the terminal back before reading from it.
///
/// The command ran under an interactive shell, and an interactive shell does
/// job control: it puts the command in its own process group, hands that group
/// the terminal with `tcsetpgrp`, and does not hand it back when the command
/// exits. This process is then in a background process group, where touching
/// the terminal's attributes is an error rather than a wait, so
/// `enable_raw_mode` fails with `EIO`, `read_choice` returns `None`, the
/// default is taken and the pane closes about a second after it opened.
///
/// It only showed up for commands that fork. A shell builtin never gets a
/// process group of its own, so the terminal never moves and the dialog waits
/// the way it is supposed to, which is why `echo` behaved and `seq` did not.
///
/// `SIGTTOU` is ignored across the call because `tcsetpgrp` from a background
/// group raises it, and the default action is to stop this process: the pane
/// would hang instead of closing, which is a worse bug than the one being
/// fixed.
fn reclaim_terminal() {
    use nix::sys::signal::{SaFlags, SigAction, SigHandler, SigSet, Signal, sigaction};
    use nix::unistd::{getpgrp, tcsetpgrp};

    let Ok(tty) = std::fs::File::open("/dev/tty") else {
        return;
    };
    let ignore = SigAction::new(SigHandler::SigIgn, SaFlags::empty(), SigSet::empty());
    // Safety: installing a handler for one signal around one call, and the
    // previous action is put back before returning.
    let previous = unsafe { sigaction(Signal::SIGTTOU, &ignore) };
    let _ = tcsetpgrp(&tty, getpgrp());
    if let Ok(previous) = previous {
        // Safety: restoring exactly what was there a moment ago.
        let _ = unsafe { sigaction(Signal::SIGTTOU, &previous) };
    }
}

/// One keypress, mapped to a choice. `None` means Enter or anything else,
/// which takes the default.
fn read_choice() -> Option<crate::run::Choice> {
    use crate::run::Choice;
    use ratatui::crossterm::{
        event::{self, Event, KeyCode},
        terminal::{disable_raw_mode, enable_raw_mode},
    };

    reclaim_terminal();
    if enable_raw_mode().is_err() {
        return None;
    }
    let choice = loop {
        match event::read() {
            Ok(Event::Key(k)) => match k.code {
                // `q` alongside `c`, because a dialog that is in the way is
                // a thing people quit rather than a thing they close, and the
                // hand reaches for `q` before it has read the buttons.
                KeyCode::Char('c' | 'C' | 'q' | 'Q') => break Some(Choice::Close),
                KeyCode::Char('v' | 'V') => break Some(Choice::View),
                KeyCode::Char('r' | 'R') => break Some(Choice::Restart),
                KeyCode::Enter | KeyCode::Esc => break None,
                _ => continue,
            },
            Ok(_) => continue,
            Err(_) => break None,
        }
    };
    let _ = disable_raw_mode();
    println!();
    choice
}

/// `open`: find a URL or a file in some text and open it.
async fn run_open(
    text: Vec<String>,
    selection: bool,
    base_dir: Option<String>,
    dry_run: bool,
    pane: Option<String>,
    cursor_x: Option<usize>,
    choose: bool,
) -> anyhow::Result<()> {
    use crate::open::Target;

    // The binding runs this through `run-shell`, and `run-shell` is the one
    // place `$TMUX_PANE` lies: it holds the most recently active pane on the
    // server rather than the pane the key was pressed in, and `run-shell -t`
    // does not change that. What does work is `#{pane_id}` in the command
    // string, which tmux expands against the pane of the key press, so the
    // binding passes the pane and this prefers what it was told.
    let pane = pane.filter(|p| !p.trim().is_empty()).or_else(pane_target);

    let text = if selection {
        let out = crate::tmux::command().arg("show-buffer").output().await?;
        String::from_utf8_lossy(&out.stdout).into_owned()
    } else if text.is_empty() {
        use std::io::Read;
        let mut buf = String::new();
        let _ = std::io::stdin().read_to_string(&mut buf);
        buf
    } else {
        text.join(" ")
    };

    let home = std::env::var("HOME").unwrap_or_default();
    let base = match base_dir {
        Some(d) => std::path::PathBuf::from(d),
        None => {
            let p = tmux_display_at(pane.as_deref(), "#{pane_current_path}").await;
            if p.is_empty() {
                std::env::current_dir().unwrap_or_default()
            } else {
                std::path::PathBuf::from(p)
            }
        }
    };

    let Some(target) = crate::open::scan_at(&text, cursor_x, &base, &home, &|p| p.exists()) else {
        // Said rather than raised. This runs from a `run-shell` binding, and
        // tmux turns a non-zero exit into `'tmux-companion open -s' returned
        // 1` in the message area, which names the command rather than the
        // problem. stdout from a run-shell is displayed, so a sentence there
        // is what somebody actually reads.
        if text.trim().is_empty() {
            println!("nothing under the cursor, and nothing selected");
        } else {
            let sample: String = text.trim().chars().take(60).collect();
            println!("no file or URL in that selection: {sample}");
        }
        return Ok(());
    };

    if dry_run {
        match &target {
            Target::Url(u) => println!("url {u}"),
            Target::File { path, line, column } => {
                println!("file {} line {line} column {column}", path.display())
            }
        }
        return Ok(());
    }

    let config = config_or_default();

    // `-i` in the script this replaces, which read "interactive" and meant
    // "let me say which of my browsers or editors this goes to". With no
    // applications configured there is nothing to choose between, so this
    // opens what it would have opened anyway rather than showing an empty
    // picker.
    if choose && !config.open.applications.is_empty() {
        return open_with_chosen(&target, &config, pane.as_deref()).await;
    }

    match target {
        // No shell anywhere in this: the text came off somebody's screen, and
        // an argument vector cannot be talked into being two commands.
        Target::Url(url) => {
            let opener = if cfg!(target_os = "macos") {
                "open"
            } else {
                "xdg-open"
            };
            // A machine with no opener is an ordinary thing -- a server, a
            // container, a minimal install -- and the raw `No such file or
            // directory (os error 2)` names the wrong file, the opener rather
            // than the URL. Say which URL could not be opened, so it can at
            // least be copied out by hand.
            if let Err(e) = tokio::process::Command::new(opener)
                .arg(&url)
                .status()
                .await
            {
                if e.kind() == std::io::ErrorKind::NotFound {
                    // stdout, because a `run-shell` binding shows stdout in
                    // the message area and drops stderr on the floor.
                    println!("no {opener} on this machine, so this was not opened: {url}");
                } else {
                    return Err(e.into());
                }
            }
        }
        Target::File { path, line, column } => {
            let command = crate::open::editor_command(
                &config.open.editor,
                &path.display().to_string(),
                line as usize,
                column as usize,
            );
            let args = crate::open::split_window_args(&config.open, command, pane.as_deref());
            let args: Vec<&str> = args.iter().map(String::as_str).collect();
            tmux(&args).await;
        }
    }
    Ok(())
}

/// `project close`: ask every window to go, rather than killing the session.
pub(crate) async fn run_close_project(
    session: Option<String>,
    discard: bool,
    save: bool,
) -> anyhow::Result<()> {
    use crate::close::{Farewell, farewell, is_editor, parse_panes, quit_command};

    let session = match session {
        Some(s) => s,
        None => tmux_display("#{session_name}").await,
    };
    // A session that is not there is an error, not a session that closed at
    // once: with no name the target was `=`, which matches nothing, so the
    // wait loop below saw "gone" on its first look and exited 0 having done
    // nothing, and the same for a name typed wrong.
    if session.is_empty() {
        anyhow::bail!("no such session: none named, and not inside tmux");
    }
    let target = format!("={session}");
    if !session_exists(&target).await {
        anyhow::bail!("no such session: {session}");
    }

    // Before anything is asked to quit, and not after. Once the editors have
    // gone every pane reports the shell, so a capture taken at the end of this
    // function would record the right geometry and none of the commands.
    //
    // A capture that fails is reported and does not stop the close: somebody
    // pressed this key to close a project, and losing the layout is a smaller
    // failure than a session that refuses to shut.
    if save && let Err(e) = save_before_close(&session).await {
        eprintln!("project close: the layout was not saved: {e}");
    }

    let listing = tmux_capture(&[
        "list-panes",
        "-s",
        "-t",
        &target,
        "-F",
        "#{pane_id} #{pane_current_command}",
    ])
    .await;
    let panes = parse_panes(&listing);

    // Editors first, and only editors, because they are the ones that leave
    // state behind.
    let editor_ids: Vec<String> = crate::close::editors(&panes)
        .into_iter()
        .map(|p| p.id.clone())
        .collect();
    for id in &editor_ids {
        tmux(&["send-keys", "-t", id, "Escape"]).await;
        tmux(&["send-keys", "-t", id, quit_command(discard), "Enter"]).await;
    }

    // A quit that does not happen means the editor is asking something, and
    // the answer is to stop and leave the question on screen.
    for id in &editor_ids {
        for _ in 0..30 {
            if !session_exists(&target).await {
                break;
            }
            if !is_editor(&pane_command(id).await) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
        let still = pane_command(id).await;
        if is_editor(&still) {
            tmux(&["select-pane", "-t", id]).await;
            tmux(&[
                "display-message",
                &format!(
                    "project close: {still} would not quit, so nothing was closed. \
                     Read what it is asking."
                ),
            ])
            .await;
            anyhow::bail!("{still} would not quit");
        }
    }

    // Read while the session is still there; the journal line is written
    // once it is gone, which is the moment the close succeeded.
    let (journal, project_path) = {
        let config = config_or_default();
        (config.journal.enabled, project_of(&session).await)
    };
    for _ in 0..40 {
        if !session_exists(&target).await {
            if journal {
                let home = std::env::var("HOME").unwrap_or_default();
                crate::journal::append(&crate::journal::Event {
                    at: crate::panes::now_secs(),
                    session: session.clone(),
                    path: crate::project::short_path(&project_path, &home),
                    kind: crate::journal::Kind::Closed,
                    detail: String::new(),
                });
            }
            return Ok(());
        }
        let listing = tmux_capture(&[
            "list-panes",
            "-s",
            "-t",
            &target,
            "-F",
            "#{pane_id} #{pane_current_command}",
        ])
        .await;
        let panes = parse_panes(&listing);
        if panes.is_empty() {
            break;
        }
        for pane in &panes {
            match farewell(&pane.command) {
                Farewell::ShellExit => {
                    tmux(&["send-keys", "-t", &pane.id, "C-u"]).await;
                    tmux(&["send-keys", "-t", &pane.id, "exit", "Enter"]).await;
                }
                Farewell::EndOfFile => {
                    tmux(&["send-keys", "-t", &pane.id, "C-d"]).await;
                }
                Farewell::Skip => {}
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }

    if session_exists(&target).await {
        tmux(&[
            "display-message",
            &format!("project close: {session} is still open; something did not take Ctrl-D."),
        ])
        .await;
        anyhow::bail!("{session} is still open");
    }
    Ok(())
}

/// Whether a session is still there.
pub(crate) async fn session_exists(target: &str) -> bool {
    // stdout and stderr both go nowhere. This is asked in a loop while a
    // session is shutting down, so the answer "no" is the expected one, and
    // letting tmux print "can't find session" to the terminal would make every
    // successful close look like it went wrong.
    crate::tmux::command()
        .args(["has-session", "-t", target])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await
        .map(|s| s.success())
        .unwrap_or(false)
}

/// What a pane is running now.
async fn pane_command(id: &str) -> String {
    tmux_capture(&["display-message", "-p", "-t", id, "#{pane_current_command}"])
        .await
        .trim()
        .to_string()
}

/// One tmux command, giving back its stdout.
/// [`tmux_capture`], but a tmux that failed is an error rather than an empty
/// answer.
///
/// The lenient version is right for a status segment or a picker, where tmux
/// having nothing to say and tmux not answering look the same and both mean
/// "draw nothing". It is wrong for a capture: `list-windows` against a session
/// that has just been renamed exits non-zero with an empty stdout, which parses
/// as a session with no windows, which used to be written over a good layout as
/// though it were a true picture of the session.
async fn tmux_capture_checked(args: &[&str]) -> anyhow::Result<String> {
    let out = crate::tmux::command()
        .args(args)
        .output()
        .await
        .map_err(|e| anyhow::anyhow!("could not run tmux {}: {e}", args.join(" ")))?;
    if !out.status.success() {
        let why = String::from_utf8_lossy(&out.stderr);
        let why = why.trim();
        anyhow::bail!(
            "tmux {} failed{}{}",
            args.join(" "),
            if why.is_empty() { "" } else { ": " },
            why
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// [`tmux_capture`] for code that runs outside the runtime: a picker's lazy
/// preview is built inside its draw loop, which is synchronous.
pub(crate) fn tmux_capture_sync(args: &[&str]) -> String {
    match crate::tmux::command_sync().args(args).output() {
        Ok(o) => String::from_utf8_lossy(&o.stdout).into_owned(),
        Err(_) => String::new(),
    }
}

pub(crate) async fn tmux_capture(args: &[&str]) -> String {
    let out = crate::tmux::command().args(args).output().await;
    match out {
        Ok(o) => String::from_utf8_lossy(&o.stdout).into_owned(),
        Err(_) => String::new(),
    }
}

/// `probe keys` and `probe cells`.
fn run_probe(what: ProbeAction) -> anyhow::Result<()> {
    use ratatui::crossterm::{
        event::{self, Event, KeyCode},
        terminal::{disable_raw_mode, enable_raw_mode},
    };

    match what {
        ProbeAction::Keys { count } => {
            println!("press keys to see what they send; q or ctrl-c to stop");
            enable_raw_mode()?;
            let mut seen = 0usize;
            loop {
                let Ok(Event::Key(key)) = event::read() else {
                    continue;
                };
                if key.code == KeyCode::Char('q') {
                    break;
                }
                // crossterm has already parsed the bytes, so this reports what
                // it decided rather than the raw sequence. That is the honest
                // thing to print: it is what any crossterm program will act on,
                // including this one.
                println!("{:?} modifiers {:?}\r", key.code, key.modifiers);
                seen += 1;
                if count.is_some_and(|c| seen >= c) {
                    break;
                }
            }
            disable_raw_mode()?;
            Ok(())
        }
        ProbeAction::Cells { strings } => {
            let strings: Vec<String> = if strings.is_empty() {
                crate::probe::DEFAULT_PROBES
                    .iter()
                    .map(|s| s.to_string())
                    .collect()
            } else {
                strings
            };
            for s in strings {
                match measure_cells(&s) {
                    Some(cells) => println!("{cells:>3} cells  {s}"),
                    None => println!("  ?  cells  {s}  (no reply from the terminal)"),
                }
            }
            Ok(())
        }
    }
}

/// Print a string and ask the terminal where the cursor ended up.
///
/// The reply is read from `/dev/tty` rather than stdin. That is the bug this
/// probe already fixed once: with stdin a pipe, reading the reply from it hangs
/// forever.
fn measure_cells(s: &str) -> Option<u32> {
    use ratatui::crossterm::terminal::{disable_raw_mode, enable_raw_mode};
    use std::io::{Read, Write};

    let mut tty = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")
        .ok()?;

    enable_raw_mode().ok()?;
    let _ = write!(tty, "\r{s}\x1b[6n");
    let _ = tty.flush();

    let mut buf = [0u8; 32];
    let n = tty.read(&mut buf).ok()?;
    let _ = write!(tty, "\r\x1b[2K");
    let _ = tty.flush();
    let _ = disable_raw_mode();

    crate::probe::cells_advanced(&buf[..n], 1)
}

/// `clipboard`: one binary picks the copy command, so the config does not.
///
/// This was two `if-shell` branches on `uname` in tmux.conf, which is one more
/// thing the Linux branch had to special-case.
async fn run_clipboard(stdin: bool) -> anyhow::Result<()> {
    let text = if stdin {
        use std::io::Read;
        let mut buf = String::new();
        let _ = std::io::stdin().read_to_string(&mut buf);
        buf
    } else {
        tmux_capture(&["show-buffer"]).await
    };
    copy_to_clipboard(&text).await
}

/// Pipe text into the clipboard command the config picks for this platform.
///
/// `setup` copies a snippet with this before it offers to write one.
pub(crate) async fn copy_to_clipboard(text: &str) -> anyhow::Result<()> {
    let config = config_or_default();
    let (program, args) = config.clipboard.command();

    let mut child = tokio::process::Command::new(&program)
        .args(args)
        .stdin(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| anyhow::anyhow!("{program}: {e}"))?;
    if let Some(mut input) = child.stdin.take() {
        use tokio::io::AsyncWriteExt;
        input.write_all(text.as_bytes()).await?;
    }
    let status = child.wait().await?;
    if !status.success() {
        anyhow::bail!("{program} exited {status}");
    }
    Ok(())
}

/// `zoom`: resize the pane, or the window when the pane is the window.
///
/// This was a `run-shell` wrapping an `if-shell`, which is tmux shelling out to
/// ask tmux how many panes there are.
async fn run_zen(pane: Option<String>) -> anyhow::Result<()> {
    // Same reason `open` takes one: the binding runs this through `run-shell`,
    // where `$TMUX_PANE` is the most recently active pane on the server rather
    // than the pane the key was pressed in. Unanchored, `resize-pane -Z`
    // zoomed a pane in whichever window the server had touched last, and the
    // window the key was pressed in did not move at all.
    let pane = pane.filter(|p| !p.trim().is_empty()).or_else(pane_target);
    let panes: u32 = tmux_display_at(pane.as_deref(), "#{window_panes}")
        .await
        .parse()
        .unwrap_or(1);
    if panes > 1 {
        match pane.as_deref() {
            Some(p) => tmux(&["resize-pane", "-Z", "-t", p]).await,
            None => tmux(&["resize-pane", "-Z"]).await,
        }
    } else {
        // One pane, so there is nothing to zoom: a lone pane already fills the
        // window, and tmux's own `prefix z` does nothing at all here -- the key
        // is dead exactly when you have the least screen. The only clutter left
        // to take is the status bar, so that is what the key takes. Both halves
        // mean the same thing, which is why this is `zen` and not `zoom`.
        tmux(&["set", "-g", "status"]).await;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_project_picker_opens_a_typed_directory_it_never_listed() {
        use crate::picker::Outcome;
        use crate::project::{Kind, Row};
        let home = tempfile::tempdir().unwrap();
        let unlisted = home.path().join("w/never-visited");
        std::fs::create_dir_all(&unlisted).unwrap();
        let home = home.path().canonicalize().unwrap().display().to_string();
        let row = |kind, label: &str, path: &str| Row {
            kind,
            label: label.into(),
            path: path.into(),
            colour: None,
            idle: None,
            mirror: None,
        };
        let rows = vec![
            row(Kind::Session, "api", "/w/api"),
            row(Kind::Directory, "web", "/w/web"),
        ];
        let pick = |o: Outcome| project_pick(&o, &rows, "/", &home, None);
        assert_eq!(pick(Outcome::Chosen(0)), ProjectPick::Session("api".into()));
        assert_eq!(
            pick(Outcome::Chosen(1)),
            ProjectPick::Directory("/w/web".into())
        );
        assert_eq!(
            pick(Outcome::Typed("~/w/never-visited".into())),
            ProjectPick::Directory(format!("{home}/w/never-visited")),
        );
        assert_eq!(
            pick(Outcome::Typed(" ~/w/nowhere ".into())),
            ProjectPick::Missing("~/w/nowhere".into()),
        );
        assert_eq!(pick(Outcome::Typed("  ".into())), ProjectPick::Nothing);
        assert_eq!(pick(Outcome::Cancelled), ProjectPick::Nothing);
    }

    #[test]
    fn inside_a_mirror_session_a_directory_is_the_hosts() {
        use crate::picker::Outcome;
        use crate::project::{Kind, Row};
        let rows = vec![Row {
            kind: Kind::Directory,
            label: "api@vm".into(),
            path: "/srv/api".into(),
            colour: None,
            idle: None,
            mirror: Some(("vm".into(), "api".into())),
        }];
        let pick = |o: Outcome, here| project_pick(&o, &rows, "/", "/home/me", here);
        assert_eq!(
            pick(Outcome::Chosen(0), None),
            ProjectPick::HostDirectory("vm".into(), "/srv/api".into()),
            "a host's row is the host's wherever the picker opened"
        );
        assert_eq!(
            pick(Outcome::Typed(" ~/src/new ".into()), Some("vm")),
            ProjectPick::HostDirectory("vm".into(), "~/src/new".into()),
            "a typed path goes to the host as typed, for the host to read"
        );
        assert_eq!(
            pick(Outcome::Typed(" ".into()), Some("vm")),
            ProjectPick::Nothing
        );
    }

    use clap::CommandFactory;

    #[test]
    fn the_help_opens_with_something_written_for_whoever_is_reading_it() {
        // clap takes a doc comment's first line as `about` and the rest as
        // `long_about`, so the note above `Cli` -- which is addressed to
        // whoever is editing this file -- was what `--help` opened with. The
        // first thing somebody sees after installing this read like somebody
        // else's memo.
        let help = Cli::command().render_long_help().to_string();
        assert!(
            !help.contains("The parsed command line"),
            "the struct's own doc comment is in --help:\n{help}"
        );
        assert!(
            !help.contains("crate version"),
            "the note about the build stamp is in --help:\n{help}"
        );
        assert!(
            help.starts_with("A status line, a set of pickers and a project manager for tmux"),
            "{help}"
        );
    }

    #[test]
    fn the_help_and_the_manual_describe_the_same_tool() {
        // Word for word what `.Nd` says in docs/tmux-companion.1, so the two
        // places somebody meets a one-line description agree. The Homebrew
        // formula's `desc` is a third wording on purpose: `brew audit` rejects
        // one that starts with an article.
        let about = Cli::command().get_about().map(|s| s.to_string());
        assert_eq!(
            about.as_deref(),
            Some("A status line, a set of pickers and a project manager for tmux")
        );
    }

    #[test]
    fn the_short_help_is_short() {
        // `-h` is the one people press by accident, and a page of prose there
        // is worse than a line.
        let short = Cli::command().render_help().to_string();
        assert!(
            !short.contains("One daemon answers for all of it"),
            "{short}"
        );
    }
}
