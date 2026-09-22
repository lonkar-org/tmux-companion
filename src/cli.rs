//! The command-line surface: the `Cmd` enum clap parses into, and the dispatch
//! that turns one of its variants into a request to the server.

use std::path::PathBuf;

use crate::proto::{ClientsArgs, GstArgs, Request, ShJobsArgs, StatusRightArgs};
use clap::{Parser, Subcommand};

/// The parsed command line.
#[derive(Parser)]
#[command(name = "tmux-companion", about = "Singleton tmux status server")]
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

/// What `theme` can do.
#[derive(Subcommand, Debug)]
#[command(rename_all = "kebab-case")]
pub enum ThemeAction {
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
        #[arg(long, default_value = "~/.config/tmux/themes")]
        themes: String,
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
        } => {
            let args = crate::proto::KeysArgs {
                query: if all { String::new() } else { query },
                refresh,
            };
            crate::client::send_and_print(Request::build("keys", &args)).await?;
        }
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
    let ThemeAction::Gen {
        apply,
        shades,
        themes,
        background,
    } = action;

    let dir = expand_tilde(&themes);
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
                    std::fs::write(&target, crate::theme::shade_file(theme, label, index, bg))?;
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
fn expand_tilde(path: &str) -> std::path::PathBuf {
    match path.strip_prefix("~/") {
        Some(rest) => match std::env::var_os("HOME") {
            Some(home) => std::path::PathBuf::from(home).join(rest),
            None => std::path::PathBuf::from(path),
        },
        None => std::path::PathBuf::from(path),
    }
}
