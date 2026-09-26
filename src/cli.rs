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

    /// Searchable key bindings
    Keys {
        /// Show every binding, including the ones tmux ships
        #[arg(long)]
        all: bool,
        /// Only bindings whose note or chord contains this
        #[arg(long, default_value = "custom: ")]
        query: String,
        /// Rebuild from tmux rather than using what the daemon holds
        #[arg(long)]
        refresh: bool,
        /// Print the rows and exit, instead of opening the picker
        #[arg(long)]
        print: bool,
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
    NewWindow,

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

    /// Move to the next window in this session, wrapping at the end
    Toggle {
        /// The session to act on, which the binding passes so the key acts on
        /// the pane it was pressed in
        session: Option<String>,
        /// Ignored; kept so bindings that pass `#{window_name}` still parse
        #[arg(hide = true)]
        window: Option<String>,
        /// Flip to the window this session was on before, tmux's own
        /// last-window, instead of the next one by index
        #[arg(long)]
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

    /// A cheat sheet of the bindings you wrote, in four boxes
    Cheatsheet {
        /// Print and exit instead of waiting for a keypress
        #[arg(long, alias = "plain")]
        print: bool,
    },

    /// Theme tools
    Theme {
        /// What to do
        #[command(subcommand)]
        action: ThemeAction,
    },

    /// Inspect the configuration file
    Config {
        /// What to do with it
        #[command(subcommand)]
        action: ConfigAction,
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
                    branch_types: config.git.branch_types.clone(),
                    bar_bg: config.bar.background.clone(),
                };
                print_segment(crate::local::gst(&opts).await?, no_tmux);
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
            all,
            query,
            refresh,
            print,
        } => run_keys(all, query, refresh, print).await?,
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
        Cmd::NewWindow => run_new_window().await?,
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
        Cmd::Toggle {
            session,
            window,
            last,
        } => run_toggle(session, window, last).await?,
        Cmd::Autosave { once, status } => run_autosave(once, status).await?,
        Cmd::Start { dir, last, hook } => run_start(dir, last, hook).await?,
        Cmd::Project { action, dir, print } => match action {
            Some(ProjectAction::Save { no_commands }) => run_project_save(!no_commands).await?,
            Some(ProjectAction::Forget { dir }) => run_project_forget(dir).await?,
            Some(ProjectAction::Show { dir }) => run_project_show(dir).await?,
            Some(ProjectAction::Close {
                session,
                discard,
                no_save,
            }) => run_close_project(session, discard, !no_save).await?,
            None => run_project(dir, print).await?,
        },
        Cmd::Sessions { action } => match action {
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
            SessionsAction::List { json } => run_sessions_list(json)?,
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
        Cmd::Cheatsheet { print } => run_cheatsheet(print).await?,
        Cmd::Doctor => crate::doctor::run().await?,
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
                    for note in config::deprecations(&config) {
                        println!("  {note}");
                    }
                }
                Err(e) => {
                    eprintln!("{e}");
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
async fn run_keys(all: bool, query: String, refresh: bool, print: bool) -> anyhow::Result<()> {
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
    if !all && nothing_noted(&rows) {
        return Ok(());
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

    let items: Vec<crate::picker::Item> = rows
        .iter()
        .map(|r| {
            crate::picker::Item::with_preview(
                format!("{} {}", r.shown, r.note),
                format!("{}\n\n{}", r.shown, r.command),
            )
            // Two columns rather than one padded string: the picker measures
            // them across every row, so the notes line up whatever the widest
            // chord turns out to be.
            .in_columns(vec![r.shown.clone(), r.note.clone()])
        })
        .collect();

    let config = config_or_default();
    let chrome = crate::picker::Chrome {
        title: "[ Keys ]".into(),
        footer: "enter runs it   ctrl-a shows tmux's own   esc cancels".into(),
        preview_title: "[ What it runs ]".into(),
        ..Default::default()
    }
    .configured(&config.picker, crate::config::Picker::Keys);

    let Some(index) = crate::picker::run(items, &opening, &chrome)? else {
        return Ok(());
    };
    let Some(row) = rows.get(index) else {
        return Ok(());
    };

    // Recorded before it runs, because the command may replace this process's
    // terminal and never come back to us.
    // A broken config should not swallow a keypress somebody already made, so
    // the defaults stand in here rather than the pick being dropped.
    let config = config_or_default();
    if config.usage.enabled {
        crate::keys::record_use(&usage_path(&config), &row.table, &row.key);
    }

    tokio::process::Command::new("tmux")
        .args(["run-shell", "-C", &row.command])
        .status()
        .await?;
    Ok(())
}

/// Say so when no binding carries the note the pickers key on, and return
/// whether that was the case.
///
/// `keys` opens on `custom: ` and `cheatsheet` shows only rows with that
/// prefix, so a tmux.conf whose bindings have no `-N "custom: ..."` note gets
/// an empty picker or four empty boxes, and both look like the tool is broken
/// rather than like the config is missing a word. One hint on stderr and exit
/// 0: nothing failed, there is just nothing to show yet.
fn nothing_noted(rows: &[crate::keys::KeyRow]) -> bool {
    if !crate::keys::filter(rows, "custom: ").is_empty() {
        return false;
    }
    eprintln!(
        "no bindings carry a -N \"custom: ...\" note; keys and cheatsheet list only those. \
         See docs/tmux.conf.starter.example"
    );
    true
}

/// Where the usage log lives.
fn usage_path(config: &crate::config::Config) -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    config.usage.log_path(&home, crate::server::state_dir())
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

    if nothing_noted(&rows) {
        return Ok(());
    }

    let config = config_or_default();
    let usage = std::fs::read_to_string(usage_path(&config))
        .map(|t| crate::keys::usage_counts(&t))
        .unwrap_or_default();

    let (cols, lines) = terminal_size();
    print!(
        "{}",
        crate::cheatsheet::render(&crate::cheatsheet::boxes(&rows, &usage), cols, lines)
    );

    if !print {
        use std::io::Write;
        print!("\n  any key to close ");
        let _ = std::io::stdout().flush();
        wait_for_a_key();
    }
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
        return run_project(dir, false).await;
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
    run_project(None, false).await
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

async fn run_project(dir: Option<String>, print: bool) -> anyhow::Result<()> {
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
        return open_project(&path, &config, &home).await;
    }

    let rows = crate::project::collect(&config, &home).await;

    if print {
        for row in &rows {
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
        }
        return Ok(());
    }

    let mut items: Vec<crate::picker::Item> = Vec::with_capacity(rows.len());
    for r in &rows {
        let mark = match r.kind {
            Kind::Session => "session",
            Kind::Directory => "dir    ",
        };
        let where_it_is = crate::project::short_path(&r.path, &home);
        // A detached session quiet for a day or more says so in a fourth
        // column, so the stale ones show without leaving the list. The column
        // is only pushed when there is something to say: an empty last cell
        // would still pad the path out to the widest row.
        let idle = crate::project::idle_column(r);
        let mut columns = vec![mark.trim_end().to_string(), r.label.clone(), where_it_is];
        let mut label = format!("{mark} {} {}", r.label, columns[2]);
        if let Some(idle) = idle {
            // In the label too, so typing `idle` filters down to them.
            label.push(' ');
            label.push_str(&idle);
            columns.push(idle);
        }
        items.push(
            crate::picker::Item::with_preview(
                label,
                project_preview(r, &config.project.preview_window).await,
            )
            .in_columns(columns)
            // The project's own theme colour as a block in front of the row.
            // Painting the whole line in it, which is what this did before,
            // makes a dark project colour unreadable on a dark popup.
            .with_swatch(r.colour.clone()),
        );
    }

    let chrome = crate::picker::Chrome {
        title: "[ Project ]".into(),
        footer: "up = last session   type a path for a new one   esc cancels".into(),
        preview_title: "[ Where ]".into(),
        ..Default::default()
    }
    .configured(&config.picker, crate::config::Picker::Project);

    let Some(index) = crate::picker::run(items, "", &chrome)? else {
        return Ok(());
    };
    let Some(row) = rows.get(index) else {
        return Ok(());
    };

    match row.kind {
        // Switched to by its own name: deriving one from the path is wrong for
        // any session whose path is not a project directory.
        Kind::Session => focus_session(&row.label).await,
        Kind::Directory => open_project(&row.path, &config, &home).await,
    }
}

/// Switch this client to a session, or attach when run from outside tmux.
async fn focus_session(name: &str) -> anyhow::Result<()> {
    let inside = std::env::var_os("TMUX").is_some();
    let verb = if inside {
        "switch-client"
    } else {
        "attach-session"
    };
    tokio::process::Command::new("tmux")
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

    // Through `session_exists`, which sends tmux's stderr nowhere: asked
    // bare, a first open printed `can't find session: x` before going on to
    // create exactly that session, so every success opened with a complaint.
    if !session_exists(&format!("={name}")).await {
        crate::project::record_visit(config, path).await;

        let (windows, _) =
            crate::saved::resolve(config, load_saved(path).await.layout(), path, home);

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
    }

    focus_session(&name).await
}

/// The server's `pane-base-index`, defaulting to 0 the way tmux does.
///
/// Read rather than assumed: a config that sets it to 1 makes every
/// `window.0` target miss, and tmux answers a missed target by doing nothing
/// rather than by saying so, so the session would come up with its commands
/// quietly absent.
async fn pane_base_index() -> usize {
    tokio::process::Command::new("tmux")
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
async fn project_preview(row: &crate::project::Row, preferred: &str) -> String {
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
            tmux_capture(&["list-windows", "-t", &target, "-F", "#{window_name}"])
                .await
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
        let screen = tmux_capture(&["capture-pane", "-p", "-e", "-t", &pane]).await;
        let tail = crate::project::tail_of_screen(&screen, 40);
        if !tail.is_empty() {
            return tail;
        }
    }
    crate::project::listing_of(std::path::Path::new(&row.path), 40)
}

/// `new-window`: pick a directory, open a window there.
///
/// The tmux default for prefix+c opens a window in the pane's directory and
/// gives you no say in it. This keeps that as the zero-keystroke case and adds
/// the rest: the frecency list, and any path at all typed in full.
async fn run_new_window() -> anyhow::Result<()> {
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

    let config = config_or_default();
    let paths: Vec<String> = crate::project::source_dirs(&config, &home).await;

    let prefill = crate::project::short_path(&pane_dir, &home);

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
    let listing = |path: &str| crate::project::listing_of(std::path::Path::new(path), 200);
    if !paths.iter().any(|p| p == &pane_dir) {
        items.push(
            crate::picker::Item::with_preview(prefill.clone(), listing(&pane_dir))
                .in_columns(vec!["here".to_string(), prefill.clone()]),
        );
        targets.push(pane_dir.clone());
    }
    for p in &paths {
        let short = crate::project::short_path(p, &home);
        items.push(
            crate::picker::Item::with_preview(short.clone(), listing(p))
                .in_columns(vec![String::new(), short]),
        );
        targets.push(p.clone());
    }
    let chrome = crate::picker::Chrome {
        title: "[ New window at ]".into(),
        footer:
            "enter opens a window   ctrl-u clears it   type a path that is not listed   esc cancels"
                .into(),
        preview_title: "[ Directory ]".into(),
        ..Default::default()
    }
    .configured(&config.picker, crate::config::Picker::Window);

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
    println!(
        "daemon {}, now {}",
        if was { "restarted" } else { "started" },
        crate::proto::build_id()
    );
    Ok(())
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
    let _ = tokio::process::Command::new("tmux")
        .arg("kill-server")
        .status()
        .await;
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
    for g in &generations {
        let mark = if Some(g.stamp.clone()) == last {
            "*"
        } else {
            " "
        };
        match crate::sessions::store::load_in(&state_dir, &g.stamp) {
            Ok(snap) => println!(
                "{mark} {}  {} session{}, {} pane{}  {}  {}",
                g.stamp,
                snap.session.len(),
                plural(snap.session.len()),
                snap.pane_count(),
                plural(snap.pane_count()),
                if snap.header.clean {
                    "taken at shutdown"
                } else {
                    "taken while running"
                },
                snap.header.captured_at
            ),
            Err(e) => println!("{mark} {}  unreadable: {e}", g.stamp),
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
        "#{window_index}\t#{pane_index}\t#{pane_current_path}\t#{pane_current_command}\t#{pane_start_command}",
    ])
    .await?;

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
    let (windows, source) = crate::saved::resolve(&config, file.layout(), &path, &home);
    print!(
        "{}",
        crate::saved::describe(
            &path,
            &windows,
            &source,
            &home,
            ignored.as_ref().map(|(f, w)| (f.as_path(), w.as_str()))
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
fn pane_target() -> Option<String> {
    match std::env::var("TMUX_PANE") {
        Ok(p) if !p.trim().is_empty() => Some(p),
        _ => None,
    }
}

pub(crate) async fn tmux(args: &[&str]) {
    let _ = tokio::process::Command::new("tmux")
        .args(args)
        .status()
        .await;
}

/// `toggle`: move to the next window in this session.
///
/// The window argument is accepted and not used. The session's own active
/// window is the one the key was pressed in, and it is an index where the
/// binding could only pass a name, which two windows can share.
///
/// `--last` is tmux's own `last-window`: the flip between the two most recent
/// windows, which is what a toggle means once a session has four windows and
/// cycling through all of them stops being a toggle at all.
async fn run_toggle(
    session: Option<String>,
    _window: Option<String>,
    last: bool,
) -> anyhow::Result<()> {
    let session = match session {
        Some(s) => s,
        None => tmux_display("#{session_name}").await,
    };
    if last {
        tmux(&["last-window", "-t", &format!("={session}")]).await;
        return Ok(());
    }
    let out = tokio::process::Command::new("tmux")
        .args([
            "list-windows",
            "-t",
            &format!("={session}"),
            "-F",
            "#{window_index} #{window_active}",
        ])
        .output()
        .await?;
    let list = String::from_utf8_lossy(&out.stdout);
    if let Some(target) = crate::tasks::toggle_target(&list) {
        tmux(&["select-window", "-t", &format!("={session}:{target}")]).await;
    }
    Ok(())
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
async fn tmux_display(format: &str) -> String {
    tmux_display_at(None, format).await
}

/// `display-message -p`, answering for one pane rather than for the server's
/// idea of the current one.
///
/// `target` wins when it is given; otherwise this falls back to `$TMUX_PANE`,
/// and to tmux's own current pane when there is neither.
async fn tmux_display_at(target: Option<&str>, format: &str) -> String {
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
    match tokio::process::Command::new("tmux")
        .args(&argv)
        .output()
        .await
    {
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
        .map(|c| crate::picker::Item::new(c.clone()))
        .collect();

    let chrome = crate::picker::Chrome {
        title: "[ Run command ]".into(),
        footer: "enter runs it in a side pane   esc cancels".into(),
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

    let args = crate::run::split_args(
        pane.as_deref(),
        opening,
        &format!("{} run --exec {}", exe.display(), shell_quote(&command)),
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
            .in_columns(vec![a.name.clone()])
        })
        .collect();

    let chrome = crate::picker::Chrome {
        title: "[ Open with ]".into(),
        footer: "enter opens it   esc cancels".into(),
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
    let out = tokio::process::Command::new("tmux")
        .args(["list-clients", "-F", "#{client_session}"])
        .output()
        .await
        .ok()?;
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .map(str::to_string)
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
async fn slide(target: &str, from: u16, to: u16, config: &crate::config::Config) {
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
    let status = tokio::process::Command::new("tmux")
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
        layout::{Alignment, Constraint, Direction, Layout},
        style::{Color, Modifier, Style},
        text::{Line, Span},
        widgets::Paragraph,
    };

    let mut selected = default_button(code);
    let said = if code == 0 {
        Span::styled(
            "Command finished successfully",
            Style::default().fg(Color::Indexed(114)),
        )
    } else {
        Span::styled(
            format!("Command failed with exit {code}"),
            Style::default().fg(Color::Indexed(174)),
        )
    };

    let mut terminal = ratatui::init();
    let result = (|| -> anyhow::Result<Choice> {
        loop {
            terminal.draw(|frame| {
                let rows = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Length(1),
                        Constraint::Length(1),
                        Constraint::Length(1),
                        Constraint::Min(0),
                        Constraint::Length(1),
                    ])
                    .split(frame.area());

                frame.render_widget(
                    Paragraph::new(Line::from(vec![Span::raw("  "), said.clone()])),
                    rows[0],
                );

                let mut buttons = vec![Span::raw("  ")];
                for (i, choice) in BUTTONS.iter().enumerate() {
                    buttons.push(if i == selected {
                        Span::styled(
                            button_label(*choice),
                            Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD),
                        )
                    } else {
                        Span::styled(
                            button_label(*choice),
                            Style::default().fg(Color::Indexed(246)),
                        )
                    });
                    buttons.push(Span::raw("   "));
                }
                frame.render_widget(Paragraph::new(Line::from(buttons)), rows[2]);

                frame.render_widget(
                    Paragraph::new(Line::from(Span::styled(
                        "  Enter=default  c/v/r/q  \u{2190}/\u{2192}/Tab  Esc=view",
                        Style::default().fg(Color::Indexed(240)),
                    )))
                    .alignment(Alignment::Left),
                    rows[4],
                );
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
        let out = tokio::process::Command::new("tmux")
            .arg("show-buffer")
            .output()
            .await?;
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
async fn run_close_project(
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

    for _ in 0..40 {
        if !session_exists(&target).await {
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
async fn session_exists(target: &str) -> bool {
    // stdout and stderr both go nowhere. This is asked in a loop while a
    // session is shutting down, so the answer "no" is the expected one, and
    // letting tmux print "can't find session" to the terminal would make every
    // successful close look like it went wrong.
    tokio::process::Command::new("tmux")
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
    let out = tokio::process::Command::new("tmux")
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

pub(crate) async fn tmux_capture(args: &[&str]) -> String {
    let out = tokio::process::Command::new("tmux")
        .args(args)
        .output()
        .await;
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

    let config = config_or_default();
    let (program, args) = config.clipboard.command();

    let mut child = tokio::process::Command::new(program)
        .args(args)
        .stdin(std::process::Stdio::piped())
        .spawn()?;
    if let Some(mut input) = child.stdin.take() {
        use tokio::io::AsyncWriteExt;
        input.write_all(text.as_bytes()).await?;
    }
    child.wait().await?;
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
