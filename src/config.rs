//! The configuration file: where it lives, how it is parsed, and what happens
//! when it cannot be.
//!
//! Every field has a default, so a machine with no config file behaves exactly
//! as the binary did before this module existed. That is not a claim, it is a
//! test: `Config::default()` has to render byte for byte what the pinned tests
//! expect.
//!
//! TOML rather than YAML, for reasons written down in `docs/comrades-port.md`.
//! The short version: `serde_yaml` was archived by its author in March 2024 and
//! both forks have sat still since, YAML 1.1 turns `no` and `off` into booleans
//! which is a problem for a file full of one-word glyph values, and anybody
//! installing a Rust program already has a `Cargo.toml` open.

use std::{collections::HashMap, path::PathBuf};

use serde::{Deserialize, Serialize};

/// Everything the daemon and its clients can be told to do differently.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(deny_unknown_fields, default)]
pub struct Config {
    /// Process-wide settings: logging, and where state is kept.
    pub general: General,
    /// Directory aliases and per-directory icons for the window segment.
    pub dirs: Dirs,
    /// The git segment.
    pub git: Git,
    /// The bandwidth segment.
    pub network: Network,
    /// The battery segment.
    pub battery: Battery,
    /// Which glyphs the bar draws with.
    pub glyphs: Glyphs,
    /// What the status bar puts where.
    pub status: Status,
    /// Jobs stopped or running under a pane.
    pub sh_jobs: ShJobs,
    /// The record of which bindings get used.
    pub usage: Usage,
    /// What a new project session starts with.
    pub layout: Vec<Layout>,
    /// Sourcing tmux's config when it changes.
    #[serde(default)]
    pub autoreload: Autoreload,
    /// Naming windows after what is running in them.
    #[serde(default)]
    pub window_names: WindowNames,
    /// Announcing a long command that finished out of sight.
    #[serde(default)]
    pub notify: Notify,
    /// Where projects come from and how they are named.
    pub project: Project,
    /// Saving the session list on a timer.
    pub autosave: Autosave,
    /// Running a command from history in a side pane.
    pub run: Run,
    /// Copying to the system clipboard.
    pub clipboard: Clipboard,
    /// Which theme a session gets before anybody picks one.
    #[serde(default)]
    pub theme: Theme,
    /// What the status bar itself looks like, which the segments draw against.
    #[serde(default)]
    pub bar: Bar,
}

/// The bar the segments are drawn on.
///
/// Every segment here ends in a powerline cap, and a cap is two colours: the
/// segment's, and whatever is behind it. That second one used to be the
/// compiled-in constant `colour233`, which is the bar in one person's tmux.conf
/// and nobody else's, so a bar set to anything else got wedges and outline
/// backgrounds in a colour that appears nowhere on the screen.
///
/// Set these to whatever `status-style` says, and `tmux-companion doctor`
/// compares the two and says so when they have drifted apart.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(deny_unknown_fields, default)]
pub struct Bar {
    /// The status bar's own background: `status-style bg=...`.
    ///
    /// Anything tmux takes, so `colour233`, `#121212`, `black` and `default`
    /// all work. `default` leaves the terminal's background showing through,
    /// which is what a transparent bar wants.
    pub background: String,
    /// The background behind the current window in the window list, which only
    /// the `window` segment draws.
    pub current_window_background: String,
}

impl Default for Bar {
    fn default() -> Self {
        Self {
            background: crate::tmux::format::BG_BAR.to_string(),
            current_window_background: crate::segments::window::BG_CURRENT_DEFAULT.to_string(),
        }
    }
}

/// The theme a session is given when it is created.
///
/// The `session-created` hook runs `theme apply` for every new session, which
/// means this decides the colour of a session nobody has picked a theme for.
/// It used to be four hardcoded names (`blue`, `magenta`, `orange`, `grey`)
/// carried over from the shell scripts this replaced, and none of them is a
/// theme `theme init` writes, so a fresh install painted nothing and printed
/// "No such file or directory" once per session instead.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(deny_unknown_fields, default)]
pub struct Theme {
    /// The theme every unclaimed session gets. Empty leaves them unpainted.
    pub default: String,
    /// A theme per session-name prefix, where the prefix is everything before
    /// the first `/`: `w/api` and `w/web` both match `w`. A session the map
    /// in `_project-map.tsv` already claims wins over this.
    pub namespace: std::collections::HashMap<String, String>,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            // One of the six `theme init` writes. Picked because it is the
            // quietest of them, being a session colour rather than a choice.
            default: "ink".to_string(),
            namespace: std::collections::HashMap::new(),
        }
    }
}

/// How to copy to the system clipboard.
///
/// This was two `if-shell` branches on `uname` in tmux.conf. One binary picking
/// the right command is one less thing the Linux branch has to special-case.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
#[serde(deny_unknown_fields, default)]
pub struct Clipboard {
    /// The command to pipe into. Empty picks one for the platform.
    pub copy: String,
}

impl Clipboard {
    /// The program and its arguments.
    pub fn command(&self) -> (String, Vec<String>) {
        if !self.copy.trim().is_empty() {
            let mut words = self.copy.split_whitespace().map(str::to_string);
            let program = words.next().unwrap_or_default();
            return (program, words.collect());
        }
        if cfg!(target_os = "macos") {
            return ("pbcopy".to_string(), Vec::new());
        }
        // Wayland first, since a session with both usually wants it.
        if std::process::Command::new("wl-copy")
            .arg("--version")
            .output()
            .is_ok()
        {
            return ("wl-copy".to_string(), Vec::new());
        }
        (
            "xclip".to_string(),
            vec!["-selection".to_string(), "clipboard".to_string()],
        )
    }
}

/// Running a command from history in a side pane.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(deny_unknown_fields, default)]
pub struct Run {
    /// Where the commands come from.
    pub history: HistorySource,
    /// The history file, when it is not where the shell usually puts it.
    pub history_file: Option<PathBuf>,
    /// How wide the pane is, as a percentage of the window.
    pub width_percent: u16,
    /// How many steps the pane takes to slide out. Zero opens it at once.
    pub slide_steps: u16,
    /// How long the slide takes, in milliseconds.
    pub slide_ms: u64,
    /// The shell the command runs under.
    pub shell: String,
}

impl Default for Run {
    fn default() -> Self {
        Self {
            history: HistorySource::Zsh,
            history_file: None,
            width_percent: 33,
            slide_steps: 5,
            slide_ms: 150,
            shell: "zsh".to_string(),
        }
    }
}

/// Which shell's history to read.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum HistorySource {
    /// zsh, plain or extended format.
    #[default]
    Zsh,
    /// bash.
    Bash,
    /// fish.
    Fish,
    /// atuin, asked through its own command.
    ///
    /// Worth having because anybody using atuin has no `.zsh_history` worth
    /// reading: atuin keeps the history in its own database.
    Atuin,
}

/// Saving the session list on a timer, so a reboot does not cost the layout.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(deny_unknown_fields, default)]
pub struct Autosave {
    /// Whether the daemon saves at all.
    pub enabled: bool,
    /// Seconds between saves.
    pub interval_secs: u64,
    /// The script that does the saving.
    ///
    /// tmux-resurrect's, by default. Only the saving half: restoring stays on
    /// a keybinding, because an automatic restore would resurrect a stale
    /// layout over a session somebody has already started working in.
    pub script: Option<PathBuf>,
}

impl Default for Autosave {
    fn default() -> Self {
        Self {
            enabled: true,
            interval_secs: 900,
            script: None,
        }
    }
}

impl Autosave {
    /// The save script, resolved against `home` when the config leaves it out.
    pub fn script_path(&self, home: &str) -> PathBuf {
        self.script.clone().unwrap_or_else(|| {
            PathBuf::from(home).join(".config/tmux/plugins/tmux-resurrect/scripts/save.sh")
        })
    }
}

/// Telling somebody a long command finished in a pane they were not watching.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(deny_unknown_fields, default)]
pub struct Notify {
    /// Whether to announce anything.
    pub enabled: bool,
    /// Seconds between scans. One tmux call each.
    pub interval_secs: u64,
    /// How long something has to run before finishing is news.
    pub threshold_secs: u64,
    /// Only announce what finished out of sight.
    ///
    /// On by default, because a command that finishes in front of you needs no
    /// announcement and firing for those is the noise that teaches people to
    /// ignore the ones that matter.
    pub only_when_unwatched: bool,
    /// What to run. Empty means tmux's own `display-message`, which needs
    /// nothing installed and behaves the same everywhere.
    ///
    /// `{command}`, `{duration}`, `{pane}` and `{message}` are substituted in
    /// every argument, so a desktop notifier can be given a title and a body.
    pub command: Vec<String>,
    /// Commands never worth announcing.
    ///
    /// An editor, a pager or an agent runs for hours, and finishing one is not
    /// news. Without this list every `:q` fires a notification about a two-hour
    /// nvim session.
    pub ignore: Vec<String>,
}

impl Default for Notify {
    fn default() -> Self {
        Self {
            enabled: false,
            interval_secs: 2,
            threshold_secs: 30,
            only_when_unwatched: true,
            command: Vec::new(),
            ignore: [
                "nvim", "vim", "vi", "emacs", "nano", "less", "more", "man", "top", "htop", "btop",
                "watch", "ssh", "tmux", "claude", "codex", "gemini", "lazygit", "tig", "fzf",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect(),
        }
    }
}

/// Naming windows after what is running in them, from the job table.
///
/// Off by default: renaming somebody's windows is visible, and a window called
/// `2.1.278` because an agent renamed itself is annoying in a way that a window
/// renamed by a tool they did not configure is worse.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(deny_unknown_fields, default)]
pub struct WindowNames {
    /// Whether to rename anything.
    pub enabled: bool,
    /// Seconds between passes. One tmux call each.
    pub interval_secs: u64,
}

impl Default for WindowNames {
    fn default() -> Self {
        Self {
            enabled: false,
            interval_secs: 5,
        }
    }
}

/// Sourcing tmux's config when it changes on disk.
///
/// Off by default, because reloading somebody's tmux config without being asked
/// is a thing that happens to their running sessions.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(deny_unknown_fields, default)]
pub struct Autoreload {
    /// Whether to watch at all.
    pub enabled: bool,
    /// Seconds between checks. This is a `stat` per file, so it is cheap.
    pub interval_secs: u64,
    /// The files to watch. Empty means the tmux config the daemon already
    /// watches for `keys`.
    pub files: Vec<PathBuf>,
}

impl Default for Autoreload {
    fn default() -> Self {
        Self {
            enabled: false,
            interval_secs: 2,
            files: Vec::new(),
        }
    }
}

/// Where the project picker gets its rows.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(deny_unknown_fields, default)]
pub struct Project {
    /// Whether to list the directories zoxide knows.
    ///
    /// zoxide is an assumption rather than a requirement: with this off the
    /// picker lists live sessions and whatever gets typed, which is a smaller
    /// tool and still a working one.
    pub zoxide: bool,
    /// The layout a new session starts with, by name.
    pub layout: String,
    /// Path-prefix overrides, first match wins.
    ///
    /// Named with a trailing underscore because `override` is a reserved word
    /// in Rust; the config file spells it without one.
    #[serde(rename = "override")]
    pub override_: Vec<LayoutOverride>,
    /// Which window the picker previews for a live session.
    ///
    /// The window worth seeing is the one you would have switched to in order
    /// to answer "what is happening over there", and for this machine that is
    /// the agent. A session without a window by this name previews whichever
    /// window it is currently on, so every session shows something rather than
    /// only the ones that happen to match.
    ///
    /// Empty means never look for a named window, and always preview the
    /// current one.
    pub preview_window: String,
}

impl Default for Project {
    fn default() -> Self {
        Self {
            zoxide: true,
            layout: "default".to_string(),
            override_: Vec::new(),
            preview_window: "ai".to_string(),
        }
    }
}

/// A layout chosen by where the project is.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct LayoutOverride {
    /// A path prefix, with `~` meaning the home directory.
    #[serde(rename = "match")]
    pub match_: String,
    /// The layout to use for a project under it.
    pub use_layout: String,
}

/// The windows a new project session starts with.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Layout {
    /// What this layout is called, which is what `project` and the overrides
    /// refer to.
    pub name: String,
    /// The windows, in order. The first one is selected when the session opens.
    #[serde(default)]
    pub window: Vec<LayoutWindow>,
}

/// One window in a layout.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct LayoutWindow {
    /// The window name, which is also what `toggle` cycles by.
    pub name: String,
    /// What to run in it. Empty leaves a shell.
    #[serde(default)]
    pub command: String,
    /// Whether to hold the name against the running program.
    ///
    /// On by default because an editor window otherwise follows whatever is
    /// running and an agent window renames itself to its own version string,
    /// which is how windows end up called `2.1.278`.
    #[serde(default = "yes")]
    pub hold_name: bool,
    /// How the panes are arranged.
    ///
    /// Either one of tmux's own five preset names, or a raw tmux layout string
    /// of the kind `tmux list-windows -F '#{window_layout}'` prints. Nothing
    /// here is parsed: the string is handed to `select-layout`, which is what
    /// lets somebody arrange a window by hand and paste the result without
    /// this file having to grow a layout language.
    #[serde(default)]
    pub layout: Option<String>,
    /// `main-pane-width` for `main-vertical`, `main-pane-height` for
    /// `main-horizontal`, ignored by the other presets.
    ///
    /// A percentage needs tmux 3.4; before that it has to be a cell count.
    #[serde(default)]
    pub main_size: Option<String>,
    /// The panes, in creation order.
    ///
    /// Empty means one pane running `command`, which is every layout written
    /// before this field existed.
    #[serde(default)]
    pub pane: Vec<LayoutPane>,
}

/// One pane in a layout window.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct LayoutPane {
    /// What to run in it. Empty leaves a shell.
    #[serde(default)]
    pub command: String,
    /// Where it starts, with `~` meaning the home directory. Empty means the
    /// project directory, same as the window.
    #[serde(default)]
    pub cwd: Option<String>,
    /// Whether this is the pane selected when the window opens.
    ///
    /// The first pane wins when nothing sets it, and the first pane that sets
    /// it wins when several do, because a layout with two focused panes is a
    /// typo rather than a question worth erroring over.
    #[serde(default)]
    pub focus: bool,
}

/// serde needs a function for a default of `true`.
fn yes() -> bool {
    true
}

/// The five layout names tmux understands, which are the only strings
/// `select-layout` will take that are not a serialised layout.
pub const TMUX_PRESETS: [&str; 5] = [
    "even-horizontal",
    "even-vertical",
    "main-horizontal",
    "main-vertical",
    "tiled",
];

impl LayoutWindow {
    /// How many panes this window opens with, never zero.
    pub fn pane_count(&self) -> usize {
        self.pane.len().max(1)
    }

    /// The window option that `main_size` sets, if the layout is one that
    /// reads it.
    ///
    /// `main-vertical` puts the main pane on the left so its size is a width;
    /// `main-horizontal` stacks it on top so the size is a height. Every other
    /// preset ignores both options, so this answers `None` rather than setting
    /// something that does nothing.
    pub fn main_size_option(&self) -> Option<&'static str> {
        match self.layout.as_deref()? {
            "main-vertical" => Some("main-pane-width"),
            "main-horizontal" => Some("main-pane-height"),
            _ => None,
        }
    }

    /// Which pane index is selected once the window is built, counting from
    /// zero within this window.
    pub fn focused_pane(&self) -> usize {
        self.pane.iter().position(|p| p.focus).unwrap_or(0)
    }
}

impl Config {
    /// The layout to start a project at `path` with.
    ///
    /// An override wins over `[project] layout`, and a name nothing defines
    /// gives an empty layout, which is a plain shell rather than an error: a
    /// typo in a layout name should cost the windows, not the session.
    pub fn layout_for(&self, path: &str, home: &str) -> Option<&Layout> {
        let wanted = self
            .project
            .override_
            .iter()
            .find(|o| {
                let prefix = match o.match_.strip_prefix("~/") {
                    Some(rest) => format!("{home}/{rest}"),
                    None => o.match_.clone(),
                };
                let prefix = prefix.trim_end_matches('*').trim_end_matches('/');
                !prefix.is_empty() && path.starts_with(prefix)
            })
            .map(|o| o.use_layout.as_str())
            .unwrap_or(&self.project.layout);
        self.layout.iter().find(|l| l.name == wanted)
    }
}

/// The record of which bindings get used, which orders the cheat sheet.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(deny_unknown_fields, default)]
pub struct Usage {
    /// Whether to record anything at all.
    ///
    /// It records what somebody presses, which is their business and not the
    /// tool's, so turning it off is one line and nothing else changes.
    pub enabled: bool,
    /// Where the log lives. Empty means `$XDG_STATE_HOME/tmux-companion/`.
    pub path: Option<PathBuf>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            general: General::default(),
            dirs: Dirs::default(),
            git: Git::default(),
            network: Network::default(),
            battery: Battery::default(),
            glyphs: Glyphs::default(),
            status: Status::default(),
            sh_jobs: ShJobs::default(),
            usage: Usage::default(),
            // Two windows, an editor and an agent, which is what
            // project-session.zsh hardcoded. Somebody who wants one window, or
            // five, or neither of these tools, changes this table.
            layout: vec![Layout {
                name: "default".to_string(),
                window: vec![
                    LayoutWindow {
                        name: "edit".to_string(),
                        command: "nvim".to_string(),
                        hold_name: true,
                        layout: None,
                        main_size: None,
                        pane: Vec::new(),
                    },
                    LayoutWindow {
                        name: "ai".to_string(),
                        command: "claude".to_string(),
                        hold_name: true,
                        layout: None,
                        main_size: None,
                        pane: Vec::new(),
                    },
                ],
            }],
            project: Project::default(),
            autoreload: Autoreload::default(),
            window_names: WindowNames::default(),
            notify: Notify::default(),
            autosave: Autosave::default(),
            run: Run::default(),
            clipboard: Clipboard::default(),
            theme: Theme::default(),
            bar: Bar::default(),
        }
    }
}

impl Default for Usage {
    fn default() -> Self {
        Self {
            enabled: true,
            path: None,
        }
    }
}

/// Which stopped or background jobs the `sh-jobs` segment draws, and how.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(deny_unknown_fields, default)]
pub struct ShJobs {
    /// Which job states count.
    pub states: JobStates,
    /// How many icons a busy pane may put on the bar.
    pub max: usize,
    /// The table, in priority order: the first entry whose pattern matches a
    /// job name wins.
    pub job: Vec<JobEntry>,
}

impl ShJobs {
    /// What `vim-bg` drew for a suspended `nvim`, kept byte for byte so the
    /// rename is only a rename.
    pub const VIM_OUTPUT: &'static str =
        "#[fg=#0262a8,bg=colour235,none] n#[fg=#539035]\u{f0577}im#[fg=colour235,bg=colour233]";
}

impl Default for ShJobs {
    fn default() -> Self {
        Self {
            states: JobStates::Stopped,
            max: 3,
            job: vec![JobEntry {
                match_: "nvim".to_string(),
                icon: Self::VIM_OUTPUT.to_string(),
                color: String::new(),
                window_name: None,
            }],
        }
    }
}

/// Which job states the segment counts.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum JobStates {
    /// Only jobs stopped with ctrl-z, which is what `vim-bg` meant.
    #[default]
    Stopped,
    /// Stopped jobs and ones running in the background.
    Any,
}

impl JobStates {
    /// Whether a job in this state counts.
    pub fn matches(self, stopped: bool) -> bool {
        match self {
            JobStates::Stopped => stopped,
            JobStates::Any => true,
        }
    }
}

/// One row of the job table.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct JobEntry {
    /// A regular expression matched against the process name.
    #[serde(rename = "match")]
    pub match_: String,
    /// What to draw for it. `{NAME}` expands to a glyph, as in a separator.
    pub icon: String,
    /// What to call a window whose active pane is running this.
    ///
    /// Unset means this entry says nothing about window names, which is what
    /// every entry written before this field existed says. The icon is tmux
    /// markup and a window name is not, so it cannot stand in for one.
    #[serde(default)]
    pub window_name: Option<String>,
    /// A tmux colour for the icon. Empty leaves it uncoloured, which is what
    /// an icon carrying its own markup wants.
    #[serde(default)]
    pub color: String,
}

impl JobEntry {
    /// Whether this entry claims a job of this name.
    ///
    /// An unparseable pattern matches nothing rather than panicking: it comes
    /// from a config file, and one bad row should cost its own icon and not
    /// the daemon.
    pub fn matches(&self, name: &str) -> bool {
        match regex::Regex::new(&self.match_) {
            Ok(re) => re.is_match(name),
            Err(_) => false,
        }
    }

    /// The markup this entry draws.
    pub fn render(&self) -> String {
        let icon = expand_glyphs(&self.icon);
        if self.color.is_empty() {
            icon
        } else {
            format!("#[fg={}]{}", self.color, icon)
        }
    }
}

/// What the status bar puts where.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
#[serde(deny_unknown_fields, default)]
pub struct Status {
    /// The right-hand side, which is the one served in a single call.
    pub right: StatusRight,
}

/// The right-hand side of the status bar.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(deny_unknown_fields, default)]
pub struct StatusRight {
    /// The segments, in the order they are drawn.
    pub segments: Vec<RightSegment>,
    /// Whether the side ends with a space.
    ///
    /// tmux draws the right side flush to the edge, and without this the last
    /// glyph sits against the terminal border.
    pub trailing_space: bool,
}

impl Default for StatusRight {
    fn default() -> Self {
        Self {
            segments: vec![
                RightSegment {
                    name: SegmentName::Git,
                    separator_before: String::new(),
                },
                RightSegment {
                    name: SegmentName::Net,
                    separator_before: String::new(),
                },
                RightSegment {
                    name: SegmentName::Battery,
                    // The literal that sat between the `net` and `battery`
                    // `#()` calls in the old configuration, reproduced byte for
                    // byte. The `{ARROW_RIGHT}` is load-bearing: dropping it
                    // silently removes the powerline wedge in front of the
                    // battery, which cost one character out of 252 and was
                    // caught only by a diff against the real tmux.conf.
                    separator_before: "#[reverse,fg=color237]{ARROW_RIGHT}#[bg=color237,none]"
                        .to_string(),
                },
            ],
            trailing_space: true,
        }
    }
}

/// One segment on a side of the status bar.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RightSegment {
    /// Which segment to draw.
    pub name: SegmentName,
    /// tmux markup drawn before this segment, when the segment renders
    /// anything at all.
    ///
    /// `{NAME}` expands to the glyph of that name in `src/tmux/icons.rs`, so
    /// the file stays readable in an editor with no patched font. An empty
    /// string means nothing between this segment and the one before it, which
    /// is a real preference and was not expressible before.
    #[serde(default)]
    pub separator_before: String,
}

/// A segment the right-hand side can draw.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum SegmentName {
    /// Git status.
    Git,
    /// Bandwidth.
    Net,
    /// Battery.
    Battery,
}

/// Expand `{NAME}` placeholders to the glyphs they name.
///
/// An unknown name is left as it was written rather than dropped: a separator
/// that renders `{ARROW_RIGH}` is a typo somebody can see, and one that
/// silently renders nothing is a typo they cannot.
pub fn expand_glyphs(template: &str) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        rest = &rest[open..];
        match rest.find('}') {
            Some(close) => {
                let name = &rest[1..close];
                match crate::tmux::icons::by_name(name) {
                    Some(glyph) => out.push_str(glyph),
                    None => out.push_str(&rest[..=close]),
                }
                rest = &rest[close + 1..];
            }
            None => break,
        }
    }
    out.push_str(rest);
    out
}

/// Which glyphs the bar draws with.
///
/// The default preset assumes a Nerd Fonts v3 patch, which most people do not
/// have, and a bar of boxes tells a new reader nothing about whether their
/// install worked.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
#[serde(deny_unknown_fields, default)]
pub struct Glyphs {
    /// Which set to start from.
    pub preset: Preset,
    /// Replacements for individual glyphs, by the constant name in
    /// `src/tmux/icons.rs`, applied on top of the preset.
    ///
    /// One missing icon is a reason to fix that icon, not to drop to a whole
    /// preset below.
    pub icons: HashMap<String, String>,
}

/// A named set of glyph replacements.
///
/// A preset is a table of names to strings in its own file, so adding one is a
/// data change with no Rust attached.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Preset {
    /// The Nerd Fonts v3 codepoints in `src/tmux/icons.rs`, unchanged.
    #[default]
    NerdFontV3,
    /// 7-bit, for a terminal whose font nobody controls.
    Ascii,
}

impl Preset {
    /// The preset's replacements, empty for the default set.
    pub fn table(self) -> HashMap<String, String> {
        let text = match self {
            Preset::NerdFontV3 => return HashMap::new(),
            Preset::Ascii => include_str!("presets/ascii.toml"),
        };
        toml::from_str(text).expect("a shipped preset parses")
    }
}

/// Process-wide settings.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
#[serde(deny_unknown_fields, default)]
pub struct General {
    /// A file the daemon appends diagnostics to. Empty means no log.
    ///
    /// A picker inside `display-popup -E` sends its stderr wherever the popup
    /// went, which is nowhere, so a path here is the only way to see what a
    /// misbehaving command said.
    pub log: Option<PathBuf>,
}

/// Directory aliases and per-directory icons for the window segment.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
#[serde(deny_unknown_fields, default)]
pub struct Dirs {
    /// Path to label, replacing the abbreviated path in the window segment.
    ///
    /// This is what `~/.yrl/lib/dir-aliases` used to hold, which was a path on
    /// one laptop compiled into a binary other people are invited to install.
    /// The old file is still read when this table is empty, so nothing breaks
    /// the day somebody upgrades.
    pub aliases: HashMap<PathBuf, String>,
}

/// One thing the git segment can draw.
///
/// The names are what a config file writes, so they describe what a reader
/// sees rather than what the code calls it: `conflicts` rather than
/// `unmerged`, `untracked` rather than `new`.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[serde(rename_all = "kebab-case")]
pub enum GitPart {
    /// The in-flight and failed-remote block before the branch name.
    Sync,
    /// The branch name itself.
    Branch,
    /// The clean, dirty, new-branch or gone-upstream marker.
    State,
    /// Commits this branch has that its upstream does not.
    Ahead,
    /// Commits the upstream has that this branch does not.
    Behind,
    /// Files with merge conflicts.
    Conflicts,
    /// Untracked files, counted with unstaged additions as git reports them.
    Untracked,
    /// Deleted files in the work tree.
    Deleted,
    /// Renamed files in the work tree.
    Renamed,
    /// Copied files in the work tree.
    Copied,
    /// Modified files in the work tree.
    Modified,
    /// Everything in the index, as one group.
    Staged,
    /// The stash count.
    Stash,
}

impl GitPart {
    /// Every part, in the order the segment drew them before the list existed.
    pub fn all() -> Vec<Self> {
        use GitPart::*;
        vec![
            Sync, Branch, State, Ahead, Behind, Conflicts, Untracked, Deleted, Renamed, Copied,
            Modified, Staged, Stash,
        ]
    }

    /// Which group this part is rendered inside.
    ///
    /// Order in the config list is honoured between groups. Inside one, the
    /// order is fixed, because a group is a single colour run and reordering
    /// its counters would move escape sequences rather than glyphs.
    pub fn group(self) -> Option<GitGroup> {
        use GitPart::*;
        match self {
            Sync | Branch | State => None,
            Ahead | Behind | Conflicts => Some(GitGroup::BranchInfo),
            Untracked | Deleted | Renamed | Copied | Modified => Some(GitGroup::Unstaged),
            Staged => Some(GitGroup::Staged),
            Stash => Some(GitGroup::Stash),
        }
    }
}

/// The groups the counters are drawn in, after the branch name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GitGroup {
    /// Ahead, behind and conflicts.
    BranchInfo,
    /// Work-tree counts.
    Unstaged,
    /// Index counts.
    Staged,
    /// The stash count.
    Stash,
}

/// The git segment.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(deny_unknown_fields, default)]
pub struct Git {
    /// How long a parsed status stays fresh, in seconds. Zero disables the
    /// cache.
    pub ttl_secs: f64,
    /// How long "this path is inside a work tree" is trusted, in seconds.
    ///
    /// A directory's repo-ness effectively never changes, but caching it
    /// forever would leave a fresh `git init` invisible until the daemon
    /// restarts.
    pub repo_check_ttl_secs: f64,
    /// Middle-ellipsize a branch name longer than this many characters.
    pub branch_max_len: usize,
    /// How many characters of the branch name's tail survive the ellipsis.
    pub branch_tail_len: usize,
    /// What the segment draws, and in what order.
    ///
    /// A part left out of this list is not rendered. Somebody working in a
    /// tree with four hundred untracked build artifacts does not want a count
    /// of them, and somebody who never pushes does not want ahead and behind.
    pub parts: Vec<GitPart>,
    /// Fetching in the background so ahead and behind mean something.
    #[serde(default)]
    pub autofetch: Autofetch,
}

/// Fetching the repositories the bar has drawn, on a timer in the daemon.
///
/// The ahead and behind counts are wrong until somebody fetches, and a status
/// bar reporting a stale number confidently is worse than one reporting
/// nothing. Off by default all the same: this is the only thing in the tool
/// that touches the network, and a daemon that quietly starts talking to a
/// remote is not a surprise anybody should get from a status bar.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(deny_unknown_fields, default)]
pub struct Autofetch {
    /// Whether to fetch at all.
    pub enabled: bool,
    /// Seconds between passes over the repositories the bar has drawn.
    pub interval_secs: u64,
    /// How long one repository gets before it is given up on.
    ///
    /// A fetch that hangs is the failure that matters: it holds the pass open
    /// and every repository behind it goes unfetched. The non-interactive
    /// environment stops the usual cause, a credential or passphrase prompt on
    /// a terminal that is not there, and this catches the rest.
    pub timeout_secs: u64,
    /// How long a repository stays on the list after the bar last drew it.
    ///
    /// Without this the daemon would fetch every repository visited since it
    /// started, forever, which on a long-lived daemon is a slowly growing bill
    /// paid to remotes nobody is looking at.
    pub remember_secs: u64,
}

impl Default for Autofetch {
    fn default() -> Self {
        Self {
            enabled: false,
            interval_secs: 600,
            timeout_secs: 20,
            remember_secs: 3600,
        }
    }
}

impl Default for Git {
    fn default() -> Self {
        Self {
            ttl_secs: 5.0,
            repo_check_ttl_secs: 300.0,
            branch_max_len: 20,
            branch_tail_len: 10,
            parts: GitPart::all(),
            autofetch: Autofetch::default(),
        }
    }
}

/// The bandwidth segment.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(deny_unknown_fields, default)]
pub struct Network {
    /// Below this many bytes per second, the segment draws nothing.
    ///
    /// The default is 20 KiB/s: a bar that reacts to every background poll is
    /// noise rather than information.
    pub threshold_bps: u64,
    /// The block the download rate is drawn on.
    pub download_colour: String,
    /// The block the upload rate is drawn on.
    pub upload_colour: String,
    /// The unit after the number: `KiB/s` in `20KiB/s`.
    ///
    /// Drawn dimmer than the number, so the figure reads first. The default is
    /// a dark grey, which is a dark-bar choice: on a light bar set this to
    /// something that is not nearly invisible.
    pub unit_colour: String,
}

impl Default for Network {
    fn default() -> Self {
        Self {
            threshold_bps: 20_480,
            download_colour: "#5cae36".to_string(),
            upload_colour: "#0262a8".to_string(),
            unit_colour: "colour237".to_string(),
        }
    }
}

/// The battery segment.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(deny_unknown_fields, default)]
pub struct Battery {
    /// How long a battery reading stays fresh, in seconds.
    pub ttl_secs: f64,
}

impl Default for Battery {
    fn default() -> Self {
        Self { ttl_secs: 30.0 }
    }
}

/// The glyph substitutions to apply to a rendered segment.
///
/// Rendering uses the constants in `src/tmux/icons.rs` throughout, and the
/// preset is applied once to the finished string rather than threaded through
/// 263 call sites. That keeps one vocabulary in the code, makes the default
/// preset free — `apply` returns the input untouched — and means a preset can
/// only replace glyphs the default set contains, which is the limitation worth
/// knowing about.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GlyphMap {
    /// From default glyph to replacement, longest first so a two-character
    /// glyph is not half-matched by a one-character one.
    pairs: Vec<(String, String)>,
}

impl GlyphMap {
    /// Build the map for a config: the preset, then the per-icon overrides.
    pub fn new(glyphs: &Glyphs) -> Self {
        let mut table = glyphs.preset.table();
        for (name, value) in &glyphs.icons {
            table.insert(name.clone(), value.clone());
        }

        let mut pairs: Vec<(String, String)> = table
            .into_iter()
            .filter_map(|(name, replacement)| {
                crate::tmux::icons::by_name(&name).map(|glyph| (glyph.to_string(), replacement))
            })
            .filter(|(from, to)| from != to && !from.is_empty())
            .collect();
        pairs.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then(a.0.cmp(&b.0)));
        Self { pairs }
    }

    /// Whether this map changes anything at all.
    pub fn is_identity(&self) -> bool {
        self.pairs.is_empty()
    }

    /// Apply the substitutions to one rendered segment.
    ///
    /// One pass, matching the longest glyph at each position, so replacements
    /// never feed into each other: an `ascii` preset mapping `STAGED` to `*`
    /// cannot then have that `*` rewritten by a later pair.
    pub fn apply<'a>(&self, s: &'a str) -> std::borrow::Cow<'a, str> {
        if self.is_identity() {
            return std::borrow::Cow::Borrowed(s);
        }
        let mut out = String::with_capacity(s.len());
        let mut rest = s;
        'outer: while !rest.is_empty() {
            for (from, to) in &self.pairs {
                if let Some(stripped) = rest.strip_prefix(from.as_str()) {
                    out.push_str(to);
                    rest = stripped;
                    continue 'outer;
                }
            }
            let ch = rest.chars().next().expect("non-empty");
            out.push(ch);
            rest = &rest[ch.len_utf8()..];
        }
        std::borrow::Cow::Owned(out)
    }
}

// ── Loading ──────────────────────────────────────────────────────────────────

/// Where a config file was found, or that there wasn't one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// Read from this path.
    File(PathBuf),
    /// No file anywhere in the search order; built-in defaults in use.
    Defaults,
}

impl std::fmt::Display for Source {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Source::File(p) => write!(f, "{}", p.display()),
            Source::Defaults => f.write_str("(built-in defaults, no config file found)"),
        }
    }
}

/// A config that could not be parsed, with enough detail to fix it.
#[derive(Debug)]
pub struct ConfigError {
    /// The file the error came from.
    pub path: PathBuf,
    /// What `toml` said, line and column included.
    pub message: String,
    /// The known key closest to the one that was not recognised, when the
    /// error looks like a typo.
    pub did_you_mean: Option<String>,
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.path.display(), self.message)?;
        if let Some(s) = &self.did_you_mean {
            write!(f, "\n  did you mean `{s}`?")?;
        }
        Ok(())
    }
}

impl std::error::Error for ConfigError {}

/// The field names serde says it expected, pulled out of its own message.
///
/// Better than a list of every key in the struct: `ttl_secs` exists under
/// `git`, `battery` and nothing else agrees, so a global list suggests the
/// wrong table. Serde already knows which table it was reading.
fn expected_fields(message: &str) -> Vec<String> {
    let Some(start) = message.find("expected one of ") else {
        return Vec::new();
    };
    message[start..]
        .split('`')
        .skip(1)
        .step_by(2)
        .map(|s| s.to_string())
        .collect()
}

/// Levenshtein distance, for suggesting the key somebody meant.
fn edit_distance(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            cur[j + 1] = (prev[j] + cost).min(prev[j + 1] + 1).min(cur[j] + 1);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

/// The expected field closest to `unknown`, when one is close enough to be a
/// typo rather than a different word entirely.
fn closest_key(unknown: &str, known: &[String]) -> Option<String> {
    known
        .iter()
        .map(|k| (edit_distance(unknown, k), k))
        .filter(|(d, _)| *d > 0 && *d <= 3)
        .min_by_key(|(d, _)| *d)
        .map(|(_, k)| k.clone())
}

/// Pull the offending key out of a serde error like ``unknown field `foo` ``.
fn unknown_field(message: &str) -> Option<String> {
    let start = message.find("unknown field `")? + "unknown field `".len();
    let rest = &message[start..];
    let end = rest.find('`')?;
    Some(rest[..end].to_string())
}

/// Parse a config from TOML text, naming the file in any error.
pub fn parse(text: &str, path: &std::path::Path) -> Result<Config, ConfigError> {
    match toml::from_str::<Config>(text) {
        Ok(c) => Ok(c),
        Err(e) => {
            let message = e.to_string();
            let did_you_mean = unknown_field(&message)
                .as_deref()
                .and_then(|k| closest_key(k, &expected_fields(&message)));
            Err(ConfigError {
                path: path.to_path_buf(),
                message,
                did_you_mean,
            })
        }
    }
}

/// The candidate config paths, in the order they are tried.
///
/// `--config` is handled by the caller, since it never has to exist to be
/// meant.
pub fn search_paths() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(p) = std::env::var_os("TMUX_COMPANION_CONFIG") {
        out.push(PathBuf::from(p));
    }
    let xdg = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")));
    if let Some(dir) = xdg {
        out.push(dir.join("tmux-companion").join("config.toml"));
    }
    if let Some(home) = std::env::var_os("HOME") {
        out.push(PathBuf::from(home).join("tmux-companion.toml"));
    }
    out
}

/// Find and parse the config, or return the defaults when there is no file.
///
/// An unreadable file is skipped as if it were absent; a file that exists and
/// does not parse is an error, because silently falling back to the defaults is
/// how somebody spends an evening wondering why a setting does nothing.
pub fn load() -> Result<(Config, Source), ConfigError> {
    for path in search_paths() {
        match std::fs::read_to_string(&path) {
            Ok(text) => return parse(&text, &path).map(|c| (c, Source::File(path))),
            Err(_) => continue,
        }
    }
    Ok((Config::default(), Source::Defaults))
}

/// Load from an explicit path, which must exist.
pub fn load_from(path: &std::path::Path) -> Result<(Config, Source), ConfigError> {
    let text = std::fs::read_to_string(path).map_err(|e| ConfigError {
        path: path.to_path_buf(),
        message: e.to_string(),
        did_you_mean: None,
    })?;
    parse(&text, path).map(|c| (c, Source::File(path.to_path_buf())))
}

/// Serialise the current defaults as a TOML document.
///
/// This is what `config dump` prints and what `docs/config.example.toml` is
/// generated from, so the example cannot drift away from the struct.
pub fn dump_defaults() -> String {
    toml::to_string_pretty(&Config::default()).expect("defaults serialise")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_file_is_the_defaults() {
        let c = parse("", std::path::Path::new("test.toml")).unwrap();
        assert_eq!(c, Config::default());
    }

    #[test]
    fn defaults_match_the_constants_the_code_used_before_the_config_existed() {
        // The promise this module makes: a machine with no config file behaves
        // exactly as the binary did before it was written.
        let c = Config::default();
        assert_eq!(c.git.ttl_secs, 5.0);
        assert_eq!(c.git.repo_check_ttl_secs, 300.0);
        assert_eq!(c.git.branch_max_len, 20);
        assert_eq!(c.git.branch_tail_len, 10);
        assert_eq!(c.network.threshold_bps, 20_480);
        assert_eq!(c.battery.ttl_secs, 30.0);
        assert!(c.dirs.aliases.is_empty());
        assert!(c.general.log.is_none());
    }

    #[test]
    fn a_partial_file_leaves_everything_else_alone() {
        let c = parse("[git]\nttl_secs = 0.0\n", std::path::Path::new("t.toml")).unwrap();
        assert_eq!(c.git.ttl_secs, 0.0);
        assert_eq!(c.git.branch_max_len, 20);
        assert_eq!(c.network, Network::default());
    }

    #[test]
    fn an_unknown_key_is_an_error_with_the_key_in_it() {
        let e = parse("[git]\nttl_sec = 5\n", std::path::Path::new("t.toml")).unwrap_err();
        assert!(e.message.contains("ttl_sec"), "{}", e.message);
    }

    #[test]
    fn an_unknown_key_suggests_the_one_that_was_meant() {
        // `ttl_secs` exists under `git` and under `battery`, so this also
        // checks the suggestion comes from the table being read rather than
        // from a global list of every key in the struct.
        let e = parse("[git]\nttl_sec = 5\n", std::path::Path::new("t.toml")).unwrap_err();
        assert_eq!(e.did_you_mean.as_deref(), Some("ttl_secs"));
    }

    #[test]
    fn an_unknown_table_is_an_error_too() {
        let e = parse("[gti]\nttl_secs = 5\n", std::path::Path::new("t.toml")).unwrap_err();
        assert!(e.message.contains("gti"), "{}", e.message);
        assert_eq!(e.did_you_mean.as_deref(), Some("git"));
    }

    #[test]
    fn a_nonsense_key_gets_no_suggestion() {
        // A suggestion that is not close to anything is worse than none: it
        // sends the reader off to change a key that was already right.
        let e = parse("[git]\nqqqqqqqqqqqq = 5\n", std::path::Path::new("t.toml")).unwrap_err();
        assert_eq!(e.did_you_mean, None);
    }

    #[test]
    fn the_error_message_names_the_file_and_the_line() {
        let e = parse(
            "[git]\nttl_secs = \"soon\"\n",
            std::path::Path::new("/tmp/x.toml"),
        )
        .unwrap_err();
        let s = e.to_string();
        assert!(s.contains("/tmp/x.toml"), "{s}");
        assert!(s.contains("line 2") || s.contains("2:"), "{s}");
    }

    #[test]
    fn the_dump_round_trips_back_to_the_defaults() {
        // What makes docs/config.example.toml safe to generate: reading the
        // dump back has to give exactly what produced it.
        let text = dump_defaults();
        let parsed = parse(&text, std::path::Path::new("dump.toml")).unwrap();
        assert_eq!(parsed, Config::default());
    }

    #[test]
    fn the_example_file_documents_every_setting() {
        // `toml` cannot emit comments, so docs/config.example.toml is written
        // by hand and this is what stops it drifting: every key the dump
        // produces has to appear in the example, or a setting exists that
        // nobody reading the example would ever find.
        let example = include_str!("../docs/config.example.toml");
        for line in dump_defaults().lines() {
            let Some((key, _)) = line.split_once(" = ") else {
                continue;
            };
            let key = key.trim();
            assert!(
                example.contains(key),
                "`{key}` is missing from docs/config.example.toml"
            );
        }
    }

    #[test]
    fn the_example_file_parses_and_is_the_defaults() {
        // It documents the defaults, so reading it has to produce them. A
        // stale example that no longer parses is worse than none, because
        // somebody copies it.
        let example = include_str!("../docs/config.example.toml");
        let parsed = parse(example, std::path::Path::new("config.example.toml")).unwrap();
        assert_eq!(parsed, Config::default());
    }

    // ── glyphs ───────────────────────────────────────────────────────────────

    #[test]
    fn the_default_preset_changes_nothing() {
        // The promise byte-identity rests on: with no config, the substitution
        // pass is not just a no-op, it does not even copy the string.
        let map = GlyphMap::new(&Glyphs::default());
        assert!(map.is_identity());
        let rendered = format!("#[fg=colour233]{}main", crate::tmux::icons::BRANCH);
        assert!(matches!(
            map.apply(&rendered),
            std::borrow::Cow::Borrowed(_)
        ));
        assert_eq!(map.apply(&rendered), rendered);
    }

    #[test]
    fn the_ascii_preset_replaces_glyphs() {
        let map = GlyphMap::new(&Glyphs {
            preset: Preset::Ascii,
            icons: HashMap::new(),
        });
        assert!(!map.is_identity());
        let rendered = format!(
            "{}main {}2",
            crate::tmux::icons::BRANCH,
            crate::tmux::icons::AHEAD
        );
        let out = map.apply(&rendered).into_owned();
        assert!(!out.contains(crate::tmux::icons::AHEAD), "{out}");
        assert!(out.contains("^"), "{out}");
    }

    #[test]
    fn an_override_beats_the_preset() {
        let map = GlyphMap::new(&Glyphs {
            preset: Preset::Ascii,
            icons: HashMap::from([("AHEAD".to_string(), "UP".to_string())]),
        });
        let out = map.apply(crate::tmux::icons::AHEAD).into_owned();
        assert_eq!(out, "UP");
    }

    #[test]
    fn an_override_works_without_a_preset() {
        let map = GlyphMap::new(&Glyphs {
            preset: Preset::NerdFontV3,
            icons: HashMap::from([("STAGED".to_string(), "*".to_string())]),
        });
        assert_eq!(map.apply(crate::tmux::icons::STAGED).into_owned(), "*");
        // and nothing else moved
        assert_eq!(
            map.apply(crate::tmux::icons::AHEAD).into_owned(),
            crate::tmux::icons::AHEAD
        );
    }

    #[test]
    fn a_replacement_is_not_itself_replaced() {
        // The reason `apply` is one pass with a longest-match rather than a
        // sequence of `str::replace` calls: `STAGED` becoming `*` must not
        // then be rewritten by whatever else maps to or from `*`.
        let map = GlyphMap::new(&Glyphs {
            preset: Preset::NerdFontV3,
            icons: HashMap::from([
                ("STAGED".to_string(), "*".to_string()),
                ("MODIFIED".to_string(), "~".to_string()),
            ]),
        });
        let rendered = format!(
            "{}{}",
            crate::tmux::icons::STAGED,
            crate::tmux::icons::MODIFIED
        );
        assert_eq!(map.apply(&rendered).into_owned(), "*~");
    }

    #[test]
    fn an_unknown_icon_name_is_ignored_rather_than_fatal() {
        // A glyph that existed in an older build and was renamed should not
        // stop the daemon: the bar loses one substitution, not its whole self.
        let map = GlyphMap::new(&Glyphs {
            preset: Preset::NerdFontV3,
            icons: HashMap::from([("NOT_A_GLYPH".to_string(), "!".to_string())]),
        });
        assert!(map.is_identity());
    }

    #[test]
    fn an_unknown_preset_name_is_a_config_error() {
        let e = parse(
            "[glyphs]\npreset = \"powerline\"\n",
            std::path::Path::new("t.toml"),
        )
        .unwrap_err();
        assert!(e.message.contains("powerline"), "{}", e.message);
    }

    #[test]
    fn the_shipped_ascii_preset_names_only_real_glyphs() {
        // A preset is data, so nothing stops a typo in it except this.
        let table = Preset::Ascii.table();
        assert!(!table.is_empty());
        for name in table.keys() {
            assert!(
                crate::tmux::icons::by_name(name).is_some(),
                "ascii.toml names `{name}`, which is not a glyph"
            );
        }
    }

    #[test]
    fn a_glyph_placeholder_expands_to_its_glyph() {
        assert_eq!(
            expand_glyphs("a{ARROW_RIGHT}b"),
            format!("a{}b", crate::tmux::icons::ARROW_RIGHT)
        );
    }

    #[test]
    fn an_unknown_placeholder_is_left_visible_rather_than_dropped() {
        // A separator that renders `{ARROW_RIGH}` is a typo somebody can see.
        // One that silently renders nothing is a typo they cannot.
        assert_eq!(expand_glyphs("a{ARROW_RIGH}b"), "a{ARROW_RIGH}b");
    }

    #[test]
    fn an_unclosed_brace_is_left_alone() {
        assert_eq!(expand_glyphs("a{ARROW_RIGHT"), "a{ARROW_RIGHT");
        assert_eq!(expand_glyphs("{"), "{");
    }

    #[test]
    fn text_with_no_placeholder_is_untouched() {
        assert_eq!(expand_glyphs("#[fg=colour233]"), "#[fg=colour233]");
        assert_eq!(expand_glyphs(""), "");
    }

    // ── layouts ──────────────────────────────────────────────────────────────

    #[test]
    fn the_default_layout_is_the_two_windows_the_script_hardcoded() {
        let c = Config::default();
        let l = c
            .layout_for("/anywhere", "/home/me")
            .expect("a default layout");
        let names: Vec<&str> = l.window.iter().map(|w| w.name.as_str()).collect();
        assert_eq!(names, vec!["edit", "ai"]);
        assert_eq!(l.window[0].command, "nvim");
        assert!(l.window[0].hold_name, "an editor window must keep its name");
    }

    #[test]
    fn the_previewed_window_defaults_to_the_agent_and_can_be_turned_off() {
        assert_eq!(Project::default().preview_window, "ai");
        let c = parse(
            "[project]\npreview_window = \"\"\n",
            std::path::Path::new("t.toml"),
        )
        .unwrap();
        assert_eq!(
            c.project.preview_window, "",
            "empty means the current window"
        );
        let c = parse(
            "[project]\npreview_window = \"logs\"\n",
            std::path::Path::new("t.toml"),
        )
        .unwrap();
        assert_eq!(c.project.preview_window, "logs");
    }

    #[test]
    fn an_override_wins_over_the_default_layout() {
        let text = r#"
[project]
layout = "default"

[[project.override]]
match = "~/work/*"
use_layout = "work"

[[layout]]
name = "default"

[[layout]]
name = "work"
"#;
        let c = parse(text, std::path::Path::new("t.toml")).unwrap();
        assert_eq!(
            c.layout_for("/home/me/work/thing", "/home/me")
                .map(|l| l.name.as_str()),
            Some("work")
        );
        assert_eq!(
            c.layout_for("/home/me/other", "/home/me")
                .map(|l| l.name.as_str()),
            Some("default")
        );
    }

    #[test]
    fn a_layout_name_nothing_defines_gives_no_windows_rather_than_an_error() {
        // A typo in a layout name should cost the windows, not the session.
        let text = "[project]\nlayout = \"nope\"\n";
        let c = parse(text, std::path::Path::new("t.toml")).unwrap();
        assert!(c.layout_for("/anywhere", "/home/me").is_none());
    }

    #[test]
    fn an_empty_layout_is_a_plain_shell() {
        let text = "[[layout]]\nname = \"bare\"\n\n[project]\nlayout = \"bare\"\n";
        let c = parse(text, std::path::Path::new("t.toml")).unwrap();
        let l = c.layout_for("/anywhere", "/home/me").expect("bare exists");
        assert!(l.window.is_empty());
    }

    #[test]
    fn the_override_key_is_spelled_without_the_underscore_in_the_file() {
        // `override` is reserved in Rust and is not in TOML.
        let text = "[[project.override]]\nmatch = \"/a\"\nuse_layout = \"x\"\n";
        let c = parse(text, std::path::Path::new("t.toml")).unwrap();
        assert_eq!(c.project.override_.len(), 1);
    }

    #[test]
    fn a_configured_clipboard_command_is_used_whole() {
        let c = Clipboard {
            copy: "xclip -selection clipboard".to_string(),
        };
        let (program, args) = c.command();
        assert_eq!(program, "xclip");
        assert_eq!(args, vec!["-selection", "clipboard"]);
    }

    #[test]
    fn an_unset_clipboard_picks_something_for_the_platform() {
        let (program, _) = Clipboard::default().command();
        assert!(!program.is_empty());
        if cfg!(target_os = "macos") {
            assert_eq!(program, "pbcopy");
        }
    }

    #[test]
    fn expected_fields_come_out_of_serdes_own_message() {
        let fields = expected_fields(
            "unknown field `ttl_sec`, expected one of `ttl_secs`, `branch_max_len`",
        );
        assert_eq!(fields, vec!["ttl_secs", "branch_max_len"]);
        assert!(expected_fields("invalid type: string").is_empty());
    }

    #[test]
    fn edit_distance_is_symmetric_and_zero_on_equal_strings() {
        assert_eq!(edit_distance("ttl_secs", "ttl_secs"), 0);
        assert_eq!(edit_distance("ttl_sec", "ttl_secs"), 1);
        assert_eq!(
            edit_distance("threshold", "threshold_bps"),
            edit_distance("threshold_bps", "threshold")
        );
    }

    #[test]
    fn unknown_field_extracts_the_key_from_a_serde_message() {
        assert_eq!(
            unknown_field("unknown field `ttl_sec`, expected one of `ttl_secs`"),
            Some("ttl_sec".to_string())
        );
        assert_eq!(unknown_field("invalid type: string"), None);
    }

    #[test]
    fn search_order_puts_the_env_var_first_and_the_home_dotfile_last() {
        // Not asserting on this machine's actual HOME: the order is the
        // contract, the values are the environment's business.
        let paths = search_paths();
        assert!(!paths.is_empty());
        let as_str: Vec<String> = paths.iter().map(|p| p.display().to_string()).collect();
        if let Some(i) = as_str
            .iter()
            .position(|p| p.contains(".config/tmux-companion"))
        {
            let j = as_str
                .iter()
                .position(|p| p.ends_with("tmux-companion.toml") && !p.contains(".config"));
            if let Some(j) = j {
                assert!(i < j, "XDG path must be tried before the home dotfile");
            }
        }
    }
}
