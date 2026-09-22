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
#[derive(Parser)]
#[command(
    name = "tmux-companion",
    about = "Singleton tmux status server",
    version = crate::proto::BUILD_ID
)]
pub struct Cli {
    /// The subcommand to run, which decides whether this process is the server
    /// or a client.
    #[command(subcommand)]
    pub command: Cmd,
}

/// Every subcommand. `server` is the daemon; the rest are clients.
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
        /// Color style: fill (solid background), outline, outline-bright
        #[arg(short = 's', long, default_value = "outline-bright")]
        style: String,
        /// Omit the trailing end-cap glyph (for use at the start of status-right)
        #[arg(long, action = clap::ArgAction::SetTrue)]
        no_cap: bool,
        /// Middle-ellipsize the branch name when longer than this (default 20)
        #[arg(long)]
        branch_max_len: Option<usize>,
        /// Show the git glyph before the branch name (off by default)
        #[arg(long, action = clap::ArgAction::SetTrue)]
        branch_icon: bool,
        /// Seconds a cached status stays fresh (0 disables the cache)
        #[arg(long, default_value = "5")]
        ttl: f64,
    },

    /// Whole right-hand status side in one call: git status, bandwidth and
    /// battery, computed concurrently and returned with the tmux literals that
    /// used to sit between them in the config.
    StatusRight {
        /// Path to the current pane's directory (defaults to current directory)
        path: Option<PathBuf>,
        /// Color style: fill (solid background), outline, outline-bright
        #[arg(short = 's', long, default_value = "outline-bright")]
        style: String,
        /// Middle-ellipsize the branch name when longer than this (default 20)
        #[arg(long)]
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
    Net,

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
    },

    /// Close a project session by letting every window exit
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

    /// Open a new window, here or at any directory
    ///
    /// The query starts on the pane's own directory, so pressing the key and
    /// then enter is "another window here" and nothing has to be typed for the
    /// common case. A directory zoxide has never seen can be typed in full.
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

    /// Zoom a pane, or the window when there is only one
    Zoom,

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
        /// Internal: run this command in this pane and show the exit dialog
        #[arg(long, hide = true)]
        exec: Option<String>,
    },

    /// Move to the next window in this session's layout
    Toggle {
        /// The session to act on, which the binding passes so the key acts on
        /// the pane it was pressed in
        session: Option<String>,
        /// The current window name, passed for the same reason
        window: Option<String>,
    },

    /// The session-list autosave the daemon runs
    Autosave {
        /// Save now and exit
        #[arg(long)]
        once: bool,
        /// Print when the last save happened
        #[arg(long)]
        status: bool,
    },

    /// Switch to a project, or start one
    Project {
        /// Save, forget or explain this project's layout
        #[command(subcommand)]
        action: Option<ProjectAction>,
        /// Go straight to this directory instead of opening the picker
        dir: Option<String>,
        /// Print the rows and exit
        #[arg(long)]
        print: bool,
    },

    /// A cheat sheet of the bindings you wrote, in four boxes
    Cheatsheet {
        /// Print and exit instead of waiting for a keypress
        #[arg(long)]
        plain: bool,
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
        /// Remember the pick for this session instead of applying it
        #[arg(short = 'r')]
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
        /// The session to resolve a theme for
        session: String,
        /// Apply to this target rather than whatever is current
        #[arg(short = 't')]
        target: Option<String>,
        /// Where the theme files are
        #[arg(long)]
        themes: Option<String>,
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

    /// Compute each theme's readable text colour and a visible border
    Gen {
        /// Write the files. Without this, report what would change and touch
        /// nothing
        #[arg(long)]
        apply: bool,
        /// Also mint a lighter and a darker sibling of each cube colour
        #[arg(long)]
        shades: bool,
        /// Where the theme files are
        #[arg(long)]
        themes: Option<String>,
        /// The terminal background to measure borders against, as #rrggbb.
        /// Read from ghostty when not given
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
        } => {
            let args = GstArgs {
                path,
                pane_pid,
                force,
                style: parse_style(&style),
                no_cap,
                branch_max_len,
                branch_icon,
                ttl_secs: ttl,
            };
            crate::client::send_and_print(Request::build("gst", &args)).await?;
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
                style: parse_style(&style),
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
        Cmd::Net => {
            crate::client::send_and_print(Request::build("net", &())).await?;
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
        } => run_open(text, selection, base_dir, dry_run).await?,
        Cmd::CloseProject {
            session,
            discard,
            no_save,
        } => run_close_project(session, discard, !no_save).await?,
        Cmd::NewWindow => run_new_window().await?,
        Cmd::ShellInit { shell } => run_shell_init(shell)?,
        Cmd::Clipboard { stdin } => run_clipboard(stdin).await?,
        Cmd::Zoom => run_zoom().await?,
        Cmd::Probe { what } => run_probe(what)?,
        Cmd::Run { print, exec } => run_command(print, exec).await?,
        Cmd::Toggle { session, window } => run_toggle(session, window).await?,
        Cmd::Autosave { once, status } => run_autosave(once, status).await?,
        Cmd::Project { action, dir, print } => match action {
            Some(ProjectAction::Save { no_commands }) => run_project_save(!no_commands).await?,
            Some(ProjectAction::Forget) => run_project_forget().await?,
            Some(ProjectAction::Show) => run_project_show().await?,
            None => run_project(dir, print).await?,
        },
        Cmd::Cheatsheet { plain } => run_cheatsheet(plain).await?,
        Cmd::Doctor => crate::doctor::run().await?,
        Cmd::Theme { action } => run_theme(action)?,
    }

    Ok(())
}

/// Map the `--style` string clap collected onto the enum the wire carries.
///
/// An unrecognised value falls back to the default, which is what
/// `Style::parse` did on the server before the style crossed the wire as a
/// string. The difference now is that the fallback happens once, in the
/// process that saw the flag.
fn parse_style(s: &str) -> crate::tmux::format::Style {
    crate::tmux::format::Style::parse(s).unwrap_or_default()
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
                Ok((_, source)) => println!("{source}: ok"),
                Err(e) => {
                    eprintln!("{e}");
                    std::process::exit(1);
                }
            }
        }
        ConfigAction::Dump => print!("{}", config::dump_defaults()),
    }
    Ok(())
}

/// `theme gen`.
fn run_theme(action: ThemeAction) -> anyhow::Result<()> {
    let (apply, shades, themes, background) = match action {
        ThemeAction::Gen {
            apply,
            shades,
            themes,
            background,
        } => (apply, shades, themes, background),
        ThemeAction::Pick {
            target,
            register,
            themes,
            print,
        } => return theme_pick(target, register, &themes_dir_or(themes), print),
        ThemeAction::Apply {
            session,
            target,
            themes,
        } => return theme_apply(&session, target, &themes_dir_or(themes)),
        ThemeAction::Init { themes } => return theme_init(&themes_dir_or(themes)),
    };

    let dir = themes_dir_or(themes);
    let (bg, source) = match background.as_deref().map(crate::theme::parse_hex) {
        Some(Some(c)) => (c, "--background".to_string()),
        Some(None) => anyhow::bail!("--background wants #rrggbb"),
        None => crate::theme::terminal_background(),
    };

    let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(&dir)
        .map_err(|e| anyhow::anyhow!("{}: {e}", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "tmux"))
        .filter(|p| {
            !p.file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with('_'))
        })
        .collect();
    files.sort();

    let mut themes_parsed = Vec::new();
    for path in &files {
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        if let Some(t) = crate::theme::parse_theme(path, &text) {
            themes_parsed.push(t);
        }
    }

    println!("{} themes", themes_parsed.len());
    println!("background {:?} from {}", bg, source);

    let mut changed = 0usize;
    let mut needs_light = Vec::new();
    let mut lifted = Vec::new();
    for theme in &themes_parsed {
        let (fg, ratio) = crate::theme::readable_on(theme.index);
        let (border, bratio) = crate::theme::border_for(theme.index, bg);
        let stem = theme
            .path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        if fg == crate::theme::TEXT_LIGHT {
            needs_light.push((stem.clone(), theme.index, ratio));
        }
        if border != theme.index {
            lifted.push((stem, theme.index, border, bratio));
        }
        if let Some(text) = crate::theme::with_computed_colours(theme, bg) {
            changed += 1;
            if apply {
                std::fs::write(&theme.path, text)?;
            }
        }
    }

    // Sorted by stem rather than left in directory order: `blue` reads before
    // `blue-dark` in a report and after it in a directory listing, and the
    // report is the thing a person reads.
    needs_light.sort_by(|a, b| a.0.cmp(&b.0));
    lifted.sort_by(|a, b| a.0.cmp(&b.0));

    println!(
        "\n-- text colour: {} of {} themes need {}, and were painting dark text on a dark block",
        needs_light.len(),
        themes_parsed.len(),
        crate::theme::TEXT_LIGHT
    );
    for (stem, index, ratio) in &needs_light {
        println!("   {stem:<22} colour{index:<4} light contrast {ratio:.1}");
    }

    println!(
        "\n-- borders: {} of {} themes need a lighter active pane border to clear {:.1}:1 on {:?}",
        lifted.len(),
        themes_parsed.len(),
        crate::theme::BORDER_MIN,
        bg
    );
    for (stem, index, border, ratio) in &lifted {
        println!("   {stem:<22} colour{index:<4} -> colour{border:<4} {ratio:.1}");
    }

    println!(
        "\n{} @theme-color-on-main and @theme-color-border into {changed} files",
        if apply { "wrote" } else { "would write" }
    );

    if shades {
        let mut taken: std::collections::HashSet<u8> =
            themes_parsed.iter().map(|t| t.index).collect();
        let mut made = Vec::new();
        let mut by_name = themes_parsed.clone();
        by_name.sort_by(|a, b| a.name.cmp(&b.name));
        for theme in &by_name {
            for (label, index) in crate::theme::shades(theme.index) {
                if !taken.insert(index) {
                    continue;
                }
                let stem = format!(
                    "{}-{label}",
                    theme
                        .path
                        .file_stem()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_default()
                );
                let target = dir.join(format!("{stem}.tmux"));
                if apply && !target.exists() {
                    std::fs::write(
                        &target,
                        crate::theme::shade_file(
                            theme,
                            label,
                            index,
                            bg,
                            &dir.display().to_string(),
                        ),
                    )?;
                }
                made.push((stem, index));
            }
        }
        println!(
            "\n-- shades: {} {}",
            if apply { "wrote" } else { "would write" },
            made.len()
        );
        for (stem, index) in &made {
            println!("   {stem:<22} colour{index}");
        }
    }

    Ok(())
}

/// `~` to the home directory, because a default path in `--help` reads better
/// with a tilde in it than with somebody's username.
/// The themes directory a `--themes` flag asked for, or the one this machine
/// actually uses.
///
/// Resolved rather than defaulted in clap, because the answer depends on
/// whether this machine keeps its tmux config under XDG or at `~/.tmux.conf`,
/// and a default string printed in `--help` would be a lie on half of them.
fn themes_dir_or(flag: Option<String>) -> std::path::PathBuf {
    match flag {
        Some(p) => expand_tilde(&p),
        None => crate::theme::default_themes_dir(),
    }
}

fn expand_tilde(path: &str) -> std::path::PathBuf {
    match path.strip_prefix("~/") {
        Some(rest) => match std::env::var_os("HOME") {
            Some(home) => std::path::PathBuf::from(home).join(rest),
            None => std::path::PathBuf::from(path),
        },
        None => std::path::PathBuf::from(path),
    }
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
                format!("{:<28} {}", r.shown, r.note),
                format!("{}\n\n{}", r.shown, r.command),
            )
        })
        .collect();

    let chrome = crate::picker::Chrome {
        title: "[ Keys ]".into(),
        footer: "enter runs it   ctrl-a shows tmux's own   esc cancels".into(),
        preview_title: "[ What it runs ]".into(),
    };

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
    let config = crate::config::load().map(|(c, _)| c).unwrap_or_default();
    if config.usage.enabled {
        crate::keys::record_use(&usage_path(&config), &row.table, &row.key);
    }

    tokio::process::Command::new("tmux")
        .args(["run-shell", "-C", &row.command])
        .status()
        .await?;
    Ok(())
}

/// Where the usage log lives.
fn usage_path(config: &crate::config::Config) -> std::path::PathBuf {
    config.usage.path.clone().unwrap_or_else(|| {
        crate::server::state_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join("keys-usage.tsv")
    })
}

/// `cheatsheet`: the same rows the picker uses, laid out in four boxes.
async fn run_cheatsheet(plain: bool) -> anyhow::Result<()> {
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

    let config = crate::config::load().map(|(c, _)| c).unwrap_or_default();
    let usage = std::fs::read_to_string(usage_path(&config))
        .map(|t| crate::keys::usage_counts(&t))
        .unwrap_or_default();

    let (cols, lines) = terminal_size();
    print!(
        "{}",
        crate::cheatsheet::render(&crate::cheatsheet::boxes(&rows, &usage), cols, lines)
    );

    if !plain {
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
async fn run_project(dir: Option<String>, print: bool) -> anyhow::Result<()> {
    use crate::project::Kind;

    let config = crate::config::load().map(|(c, _)| c).unwrap_or_default();
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
        items.push(
            crate::picker::Item::with_preview(
                format!(
                    "{mark}  {:<24} {}",
                    r.label,
                    crate::project::short_path(&r.path, &home)
                ),
                project_preview(r, &config.project.preview_window).await,
            )
            // The project's own theme colour, so the list reads the way the
            // status bar does. A project with no colour in the map stays the
            // default rather than being given one.
            .in_colour(r.colour.clone()),
        );
    }

    let chrome = crate::picker::Chrome {
        title: "[ Project ]".into(),
        footer: "up = last session   type a path for a new one   esc cancels".into(),
        preview_title: "[ Where ]".into(),
    };

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

    let exists = tokio::process::Command::new("tmux")
        .args(["has-session", "-t", &format!("={name}")])
        .status()
        .await
        .map(|s| s.success())
        .unwrap_or(false);

    if !exists {
        crate::project::record_visit(path).await;

        let (windows, _) = crate::saved::resolve(config, crate::saved::load(path), path, home);

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
    Forget,
    /// Which layout this project gets, and which file decided
    Show,
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
        let pane = if named {
            format!("={}:{preferred}", row.label)
        } else {
            target.clone()
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
    let pane_dir = {
        let d = tmux_display("#{pane_current_path}").await;
        if std::path::Path::new(&d).is_dir() {
            d
        } else {
            std::env::current_dir()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|_| home.clone())
        }
    };

    let config = crate::config::load().map(|(c, _)| c).unwrap_or_default();
    let paths: Vec<String> = if config.project.zoxide {
        crate::project::zoxide_dirs().await
    } else {
        Vec::new()
    };

    let items: Vec<crate::picker::Item> = paths
        .iter()
        .map(|p| crate::picker::Item::with_preview(crate::project::short_path(p, &home), p.clone()))
        .collect();

    let prefill = crate::project::short_path(&pane_dir, &home);
    let chrome = crate::picker::Chrome {
        title: "[ New window at ]".into(),
        footer: "enter opens a window   ctrl-u clears it   type a path zoxide has not seen   esc cancels".into(),
        preview_title: "[ Directory ]".into(),
    };

    let outcome = crate::picker::run_with_query(items, &prefill, &chrome)?;
    match crate::project::window_target(&outcome, &paths, &prefill, &pane_dir, &home) {
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
            crate::project::record_visit(&dir).await;
            tmux(&["new-window", "-c", &dir]).await;
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

/// Capture a session's layout on the way out.
async fn save_before_close(session: &str) -> anyhow::Result<()> {
    let path = project_of(session).await;
    if path.is_empty() {
        anyhow::bail!("no directory for session {session}");
    }
    let (saved, guessed) = capture_session(session, &path, true).await?;
    crate::saved::store_rendered(&saved, &guessed)?;
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
        anyhow::bail!("not inside tmux");
    }
    let path = project_of(&session).await;
    Ok((session, path))
}

/// `project save`: capture this session and write it for this project.
async fn run_project_save(with_commands: bool) -> anyhow::Result<()> {
    let (session, path) = current_project().await?;
    let (saved, guessed) = capture_session(&session, &path, with_commands).await?;
    let file = crate::saved::store_rendered(&saved, &guessed)?;
    println!(
        "saved {} window{} for {}\n  {}",
        saved.window.len(),
        if saved.window.len() == 1 { "" } else { "s" },
        path,
        file.display()
    );
    if !guessed.is_empty() {
        println!(
            "  {} pane{} took its command from the running process, so any arguments are gone",
            guessed.len(),
            if guessed.len() == 1 { "" } else { "s" }
        );
    }
    Ok(())
}

/// Ask tmux what a session looks like right now.
async fn capture_session(
    session: &str,
    path: &str,
    with_commands: bool,
) -> anyhow::Result<(crate::saved::SavedLayout, Vec<(usize, usize)>)> {
    let target = format!("={session}");
    let windows = tmux_capture(&[
        "list-windows",
        "-t",
        &target,
        "-F",
        "#{window_index}\t#{window_name}\t#{window_width}\t#{window_height}\t#{window_layout}",
    ])
    .await;
    let panes = tmux_capture(&[
        "list-panes",
        "-s",
        "-t",
        &target,
        "-F",
        "#{window_index}\t#{pane_index}\t#{pane_current_path}\t#{pane_current_command}\t#{pane_start_command}",
    ])
    .await;

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
        home: &home,
        at: &at,
        with_commands,
    }))
}

/// `project forget`: drop this project's saved layout.
async fn run_project_forget() -> anyhow::Result<()> {
    let (_, path) = current_project().await?;
    if crate::saved::forget(&path)? {
        println!("forgot the saved layout for {path}");
    } else {
        println!("no saved layout for {path}");
    }
    Ok(())
}

/// `project show`: which layout this project gets, and which file decided.
async fn run_project_show() -> anyhow::Result<()> {
    let (_, path) = current_project().await?;
    let config = crate::config::load().map(|(c, _)| c).unwrap_or_default();
    let home = std::env::var("HOME").unwrap_or_default();
    let (windows, source) = crate::saved::resolve(&config, crate::saved::load(&path), &path, &home);
    print!(
        "{}",
        crate::saved::describe(&path, &windows, &source, &home)
    );
    Ok(())
}

/// One tmux command, ignoring a failure.
///
/// Each of these is a step in building a session, and a step that fails should
/// cost its own window rather than leaving half a session and an error.
async fn tmux(args: &[&str]) {
    let _ = tokio::process::Command::new("tmux")
        .args(args)
        .status()
        .await;
}

/// `toggle`: move to the next window in this session's layout.
async fn run_toggle(session: Option<String>, window: Option<String>) -> anyhow::Result<()> {
    let session = match session {
        Some(s) => s,
        None => tmux_display("#{session_name}").await,
    };
    let current = match window {
        Some(w) => w,
        None => tmux_display("#{window_name}").await,
    };

    let config = crate::config::load().map(|(c, _)| c).unwrap_or_default();
    let home = std::env::var("HOME").unwrap_or_default();
    let path = tmux_display("#{session_path}").await;
    let (layout, _) = crate::saved::resolve(&config, crate::saved::load(&path), &path, &home);
    let windows: Vec<String> = layout.iter().map(|w| w.name.clone()).collect();

    match crate::tasks::toggle_target(&current, &windows) {
        Some(target) => {
            tmux(&["select-window", "-t", &format!("={session}:{target}")]).await;
        }
        // Not a layout session. Keep the old two-window habit working rather
        // than printing "can't find window" at somebody.
        None => tmux(&["last-window"]).await,
    }
    Ok(())
}

/// `autosave`: the timer lives in the daemon, so this is the manual half.
async fn run_autosave(once: bool, status: bool) -> anyhow::Result<()> {
    if status {
        println!("{}", crate::tasks::last_save());
        return Ok(());
    }
    if once {
        let config = crate::config::load().map(|(c, _)| c).unwrap_or_default();
        let home = std::env::var("HOME").unwrap_or_default();
        return crate::tasks::save_now(&config.autosave.script_path(&home)).await;
    }
    // Neither flag: say where the loop actually lives rather than starting a
    // second one, which is what the zsh version needed a lock file to prevent.
    println!("the daemon runs the autosave loop; --once saves now, --status says when it last did");
    Ok(())
}

/// One `tmux display-message -p`, empty when tmux is not there.
async fn tmux_display(format: &str) -> String {
    let out = tokio::process::Command::new("tmux")
        .args(["display-message", "-p", format])
        .output()
        .await;
    match out {
        Ok(o) => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        Err(_) => String::new(),
    }
}

/// `theme pick`: choose one, then apply it or remember it.
fn theme_pick(
    target: Option<String>,
    register: Option<String>,
    dir: &std::path::Path,
    print: bool,
) -> anyhow::Result<()> {
    let rows = crate::theme::rows(dir);
    if rows.is_empty() {
        anyhow::bail!("no themes in {}", dir.display());
    }

    if print {
        for row in &rows {
            println!(
                "{}\t{}\t{}",
                row.path.file_name().unwrap_or_default().to_string_lossy(),
                row.name,
                row.colour
            );
        }
        return Ok(());
    }

    let items: Vec<crate::picker::Item> = rows
        .iter()
        .map(|r| crate::picker::Item::with_preview(r.label(), theme_preview(r)))
        .collect();

    let chrome = crate::picker::Chrome {
        title: match &register {
            Some(s) => format!("[ Theme for {s} ]"),
            None => "[ Theme ]".to_string(),
        },
        footer: "enter applies it   esc cancels".into(),
        preview_title: "[ Colours ]".into(),
    };

    let Some(index) = crate::picker::run(items, "", &chrome)? else {
        return Ok(());
    };
    let row = &rows[index];

    if let Some(session) = register {
        // Remembered and not applied: the session does not exist yet, so
        // applying here would paint whichever session happens to be current.
        let stem = row
            .path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let map = dir.join("_project-map.tsv");
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(map)?;
        writeln!(f, "{session}\t{stem}")?;
        println!("{}", row.path.display());
        return Ok(());
    }

    source_theme(&row.path, target.as_deref());
    Ok(())
}

/// `theme init`: write the starter themes and the machinery that applies them.
///
/// Refuses to overwrite. Somebody running this twice, or running it beside
/// themes they already wrote, should get the files they are missing and keep
/// everything they have; the alternative is a command that can quietly undo an
/// afternoon's work.
fn theme_init(dir: &std::path::Path) -> anyhow::Result<()> {
    use crate::theme::{BASE_THEMES, apply_file, base_theme_file, readable_on, reset_file};

    std::fs::create_dir_all(dir)?;

    let mut written = Vec::new();
    let mut kept = Vec::new();
    let mut write = |name: String, body: String| -> anyhow::Result<()> {
        let path = dir.join(&name);
        if path.exists() {
            kept.push(name);
            return Ok(());
        }
        std::fs::write(&path, body)?;
        written.push(name);
        Ok(())
    };

    write("_reset.tmux".to_string(), reset_file())?;
    write("_apply.tmux".to_string(), apply_file())?;
    for (stem, label, index) in BASE_THEMES {
        write(
            format!("{stem}.tmux"),
            base_theme_file(stem, label, index, &dir.display().to_string()),
        )?;
    }

    println!("{}", dir.display());
    for name in &written {
        let index = BASE_THEMES
            .iter()
            .find(|(s, _, _)| format!("{s}.tmux") == *name)
            .map(|(_, _, i)| *i);
        match index {
            Some(i) => {
                let (fg, ratio) = readable_on(i);
                println!("  wrote  {name:<16} colour{i} with {fg} at {ratio:.1}:1");
            }
            None => println!("  wrote  {name}"),
        }
    }
    for name in &kept {
        println!("  kept   {name:<16} already there, left alone");
    }
    if written.is_empty() {
        println!("\nNothing to do: every file was already there.");
        return Ok(());
    }
    println!(
        "\nNext: tmux-companion theme gen --apply --shades\n\
         That mints a lighter and a darker sibling of each colour and measures\n\
         every border against this terminal's background."
    );
    Ok(())
}

/// `theme apply`: the session-created hook's half, with no picker.
fn theme_apply(session: &str, target: Option<String>, dir: &std::path::Path) -> anyhow::Result<()> {
    let map = std::fs::read_to_string(dir.join("_project-map.tsv"))
        .map(|t| crate::project::parse_project_map(&t))
        .unwrap_or_default();
    let path = crate::theme::theme_for_session(session, &map, dir);
    source_theme(&path, target.as_deref());
    Ok(())
}

/// A few lines showing what a theme is made of.
fn theme_preview(row: &crate::theme::ThemeRow) -> String {
    let text = std::fs::read_to_string(&row.path).unwrap_or_default();
    let settings = crate::theme::parse_settings(&text);
    let mut keys: Vec<&String> = settings.keys().collect();
    keys.sort();
    keys.iter()
        .map(|k| {
            let v = &settings[*k];
            format!("{} {:<26} {v}", crate::theme::swatch(v), k)
        })
        .collect::<Vec<String>>()
        .join("\n")
}

/// `tmux source-file`, honouring a target.
///
/// The target matters: `_apply.tmux` sets window options, and a window option
/// lands on one window, so without a target the theme paints whichever window
/// happened to be current and every other window in the session keeps the
/// global default. That is why copy-mode selection could be readable in one
/// window and not the next.
fn source_theme(path: &std::path::Path, target: Option<&str>) {
    let path = path.display().to_string();
    let mut args: Vec<&str> = vec!["source-file"];
    if let Some(t) = target {
        args.push("-t");
        args.push(t);
    }
    args.push(&path);
    let _ = std::process::Command::new("tmux").args(args).status();
}

/// `run`: pick a command from history and run it in a pane beside this one.
async fn run_command(print: bool, exec: Option<String>) -> anyhow::Result<()> {
    let config = crate::config::load().map(|(c, _)| c).unwrap_or_default();
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
    };

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
    let width = crate::run::pane_width(window_width().await, config.run.width_percent);
    let opening = if config.run.slide_steps > 0 { 1 } else { width };

    tmux(&[
        "split-window",
        "-fh",
        "-l",
        &opening.to_string(),
        &format!("{} run --exec {}", exe.display(), shell_quote(&command)),
    ])
    .await;
    Ok(())
}

/// Quote a command so tmux hands it back to us whole.
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// The width of the window this pane is in.
async fn window_width() -> u16 {
    tmux_display("#{window_width}").await.parse().unwrap_or(180)
}

/// Run the command here, then offer the dialog, repeating on Restart.
async fn run_in_this_pane(command: &str, config: &crate::config::Config) -> anyhow::Result<()> {
    use crate::run::Choice;

    let pane = std::env::var("TMUX_PANE").unwrap_or_default();
    let target = pane.clone();
    let width = crate::run::pane_width(window_width().await, config.run.width_percent);
    slide(&target, 1, width, config).await;

    loop {
        println!("\x1b[2m$ \x1b[0m{command}");
        let status = tokio::process::Command::new(&config.run.shell)
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
async fn dialog(code: i32, _config: &crate::config::Config) -> crate::run::Choice {
    use crate::run::{Choice, default_choice};
    use std::io::Write;

    let default = default_choice(code);
    let label = if code == 0 {
        "\x1b[32m✔ done\x1b[0m".to_string()
    } else {
        format!("\x1b[31m✘ exit {code}\x1b[0m")
    };
    let hint = match default {
        Choice::Close => "[C]lose  [v]iew  [r]estart",
        _ => "[c]lose  [v]iew  [R]estart",
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
                KeyCode::Char('c' | 'C') => break Some(Choice::Close),
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
) -> anyhow::Result<()> {
    use crate::open::Target;

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
            let p = tmux_display("#{pane_current_path}").await;
            if p.is_empty() {
                std::env::current_dir().unwrap_or_default()
            } else {
                std::path::PathBuf::from(p)
            }
        }
    };

    let Some(target) = crate::open::scan(&text, &base, &home, &|p| p.exists()) else {
        anyhow::bail!("nothing to open in that text");
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

    match target {
        // No shell anywhere in this: the text came off somebody's screen, and
        // an argument vector cannot be talked into being two commands.
        Target::Url(url) => {
            let opener = if cfg!(target_os = "macos") {
                "open"
            } else {
                "xdg-open"
            };
            tokio::process::Command::new(opener)
                .arg(url)
                .status()
                .await?;
        }
        Target::File { path, line, column } => {
            let at = match (line, column) {
                (0, _) => path.display().to_string(),
                (l, 0) => format!("+{l} {}", path.display()),
                (l, c) => format!("+call cursor({l},{c}) {}", path.display()),
            };
            tmux(&["split-window", "-h", &format!("nvim {at}")]).await;
        }
    }
    Ok(())
}

/// `close-project`: ask every window to go, rather than killing the session.
async fn run_close_project(
    session: Option<String>,
    discard: bool,
    save: bool,
) -> anyhow::Result<()> {
    use crate::close::{Farewell, farewell, parse_panes, quit_command};

    let session = match session {
        Some(s) => s,
        None => tmux_display("#{session_name}").await,
    };
    let target = format!("={session}");

    // Before anything is asked to quit, and not after. Once the editors have
    // gone every pane reports the shell, so a capture taken at the end of this
    // function would record the right geometry and none of the commands.
    //
    // A capture that fails is reported and does not stop the close: somebody
    // pressed this key to close a project, and losing the layout is a smaller
    // failure than a session that refuses to shut.
    if save {
        if let Err(e) = save_before_close(&session).await {
            eprintln!("close-project: the layout was not saved: {e}");
        }
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
            if pane_command(id).await != "nvim" {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
        if pane_command(id).await == "nvim" {
            tmux(&["select-pane", "-t", id]).await;
            tmux(&[
                "display-message",
                "close-project: nvim would not quit, so nothing was closed. Read what it is asking.",
            ])
            .await;
            anyhow::bail!("nvim would not quit");
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
            &format!("close-project: {session} is still open; something did not take Ctrl-D."),
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
async fn tmux_capture(args: &[&str]) -> String {
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

    let config = crate::config::load().map(|(c, _)| c).unwrap_or_default();
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
async fn run_zoom() -> anyhow::Result<()> {
    let panes: u32 = tmux_display("#{window_panes}").await.parse().unwrap_or(1);
    if panes > 1 {
        tmux(&["resize-pane", "-Z"]).await;
    } else {
        // One pane, so zooming it does nothing anybody can see. Toggling the
        // status bar is what somebody pressing zoom in that situation wants.
        tmux(&["set", "-g", "status"]).await;
    }
    Ok(())
}
