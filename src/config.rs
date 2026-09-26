//! The configuration file: where it lives, how it is parsed, and what happens
//! when it cannot be.
//!
//! Every field has a default, so a machine with no config file behaves exactly
//! as the binary did before this module existed. That is not a claim, it is a
//! test: `Config::default()` has to render byte for byte what the pinned tests
//! expect.
//!
//! TOML rather than YAML, for reasons written down in `docs/dev/comrades-port.md`.
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
    /// Which programs are coding agents, and when one counts as waiting.
    #[serde(default)]
    pub agents: Agents,
    /// Where projects come from and how they are named.
    pub project: Project,
    /// Saving the session list on a timer.
    pub autosave: Autosave,
    /// Snapshots of the whole server, in generations.
    #[serde(default)]
    pub sessions: Sessions,
    /// What a restore is allowed to run, and how.
    #[serde(default)]
    pub restore: Restore,
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
    /// How every picker is laid out.
    #[serde(default)]
    pub picker: PickerLayout,
    /// What happens when a file is opened out of copy mode.
    #[serde(default)]
    pub open: Open,
}

/// Where the editor goes when `open` finds a file.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(deny_unknown_fields, default)]
pub struct Open {
    /// The pane the editor opens in, beside or below the one you are in.
    pub split: Split,
    /// How big that pane is, as a percentage of the window. Zero lets tmux
    /// halve it, which is what it did before this was a setting.
    pub size_percent: u16,
    /// The editor, and how it is told to jump to a line and column.
    ///
    /// `{path}`, `{line}` and `{column}` are replaced. The default is what
    /// vim and neovim take; emacs and helix want something else, which is
    /// why this is a template rather than a program name.
    pub editor: String,

    /// The applications `open --choose` offers.
    ///
    /// Empty means no chooser: `--choose` then opens what it would have opened
    /// anyway rather than showing a picker with nothing in it.
    ///
    /// This is the `-i` of the script it replaces, which read "interactive"
    /// and meant "let me say which of my browsers or editors this goes to".
    /// A list rather than a compiled-in set of browsers, because the right
    /// answer is whatever somebody has installed.
    #[serde(default, rename = "application")]
    pub applications: Vec<Application>,
}

/// One entry in `open --choose`.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields, default)]
pub struct Application {
    /// What the picker calls it.
    pub name: String,
    /// The command, with `{url}`, `{path}`, `{line}` and `{column}` replaced.
    ///
    /// Run through the shell, like `editor` above, because a template with
    /// quoting in it is the only way an application name with a space in it
    /// reaches a `-a` flag intact.
    pub command: String,
    /// Whether this opens in a tmux pane beside the one you are in, the way
    /// the editor does, rather than being launched and left alone.
    ///
    /// An editor wants a pane and a browser does not, and getting it the wrong
    /// way round means either a browser that holds a pane open forever or an
    /// editor with nowhere to draw.
    pub pane: bool,
}

/// Which way `open` splits the window for an editor.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Split {
    /// Beside the pane you are in.
    #[default]
    Right,
    /// Under the pane you are in.
    Bottom,
}

impl Split {
    /// The flag tmux's `split-window` wants.
    pub fn flag(self) -> &'static str {
        match self {
            Split::Right => "-h",
            Split::Bottom => "-v",
        }
    }
}

impl Default for Open {
    fn default() -> Self {
        Self {
            split: Split::Right,
            size_percent: 0,
            editor: "nvim '+call cursor({line},{column})' {path}".to_string(),
            applications: Vec::new(),
        }
    }
}

/// How the pickers are laid out.
///
/// One setting for all of them, because five pickers that each drift into
/// their own shape are five things to learn rather than one.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
#[serde(deny_unknown_fields, default)]
pub struct PickerLayout {
    /// What every picker gets unless its own table says otherwise.
    ///
    /// The same shape as one picker's table, and for the same reason: a field
    /// nobody wrote has to be telling apart from one somebody wrote to the
    /// value that happens to be the default. Comparing against the default was
    /// what this did before, and it meant writing `preview_label_position =
    /// "bottom-center"` here quietly stopped every picker using its own
    /// answer for it.
    ///
    /// Flattened, so these are written straight under `[picker]` rather than
    /// under a `[picker.look]` nobody would guess the name of.
    #[serde(flatten)]
    pub global: PickerOverride,

    /// `[picker.keys]`, for the key search.
    ///
    /// One picker's answer where it differs from the rest. The shape is shared
    /// on purpose -- six pickers that each drift into their own is six things
    /// to learn rather than one -- but the preview is genuinely not the same
    /// question for each, so `[picker]` is the answer and `[picker.keys]` is
    /// the exception.
    #[serde(default)]
    pub keys: PickerOverride,
    /// `[picker.project]`, for the project and session list.
    #[serde(default)]
    pub project: PickerOverride,
    /// `[picker.window]`, for the directory list a new window opens at.
    #[serde(default)]
    pub window: PickerOverride,
    /// `[picker.theme]`, for the colour themes.
    #[serde(default)]
    pub theme: PickerOverride,
    /// `[picker.run]`, for the shell history.
    #[serde(default)]
    pub run: PickerOverride,
    /// `[picker.open]`, for the application chooser.
    #[serde(default)]
    pub open: PickerOverride,
    /// `[picker.panes]`, for the list of every pane on the server.
    #[serde(default)]
    pub panes: PickerOverride,
}

/// One picker's departures from `[picker]`.
///
/// Every field is optional and an absent one means "whatever the shared answer
/// is". Written out rather than derived, because a macro here would save
/// thirty lines and cost the config the error message that names the key you
/// got wrong.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields, default)]
pub struct PickerOverride {
    /// What this picker calls itself on its border, e.g. `[ Keys ]`.
    ///
    /// The default is the call site's, because only it knows whether this is
    /// the key search or the theme list. It is settable because a label is
    /// four words somebody reads every day and the tool's four words are not
    /// necessarily theirs.
    pub label: Option<String>,
    /// The line at the top saying what the keys do.
    ///
    /// Same reasoning, and more of it: the shipped line names the keys this
    /// tool binds, and anybody who has been driving fzf has a line of their
    /// own that names the same keys in their own words.
    pub hint: Option<String>,
    /// What this picker calls its preview pane.
    pub preview_label: Option<String>,
    /// Where the preview pane goes.
    pub preview: Option<crate::picker::Preview>,
    /// The preview's share of the popup.
    pub preview_percent: Option<u16>,
    /// The line the box is drawn with.
    pub border: Option<crate::picker::BorderKind>,
    /// Where the picker's own label sits.
    pub label_position: Option<crate::picker::LabelPosition>,
    /// Cells of border left showing beyond that label.
    pub label_offset: Option<u16>,
    /// Which end the line explaining the keys sits at.
    pub hint_position: Option<crate::picker::Edge>,
    /// Which end the query sits at.
    pub prompt_position: Option<crate::picker::Edge>,
    /// Which end the first row sits at.
    pub list_from: Option<crate::picker::Edge>,
    /// Whether the matched/total counter is drawn.
    pub counter: Option<bool>,
    /// Rules between the hint, the list and the query.
    pub rules: Option<bool>,
    /// What is drawn in front of the row the cursor is on.
    pub marker: Option<String>,
    /// The order a row's columns are drawn in.
    pub column_order: Option<Vec<usize>>,
    /// The fewest columns a list may keep before a side preview moves under it.
    pub min_list_width: Option<u16>,
    /// How much of a border the preview pane gets.
    pub preview_border: Option<crate::picker::PreviewBorder>,
    /// Where the preview's label sits on that line.
    pub preview_label_position: Option<crate::picker::LabelPosition>,
    /// Cells of that line left showing beyond the preview's label.
    pub preview_label_offset: Option<u16>,
}

/// Which picker is asking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Picker {
    /// The key search.
    Keys,
    /// The project and session list.
    Project,
    /// The directory list a new window opens at.
    Window,
    /// The colour themes.
    Theme,
    /// The shell history.
    Run,
    /// The application chooser `open --choose` shows.
    Open,
    /// Every pane on the server, or only the agents among them.
    Panes,
}

/// What one picker looks like before anybody configures it.
///
/// The preview is not one question with one answer. What goes in it differs by
/// picker -- a card, three lines of tmux command, a screenshot of another
/// session, a directory listing, nothing at all -- and so does how much room
/// that needs. These are those answers, and they are here rather than in the
/// example file so a machine with no config still gets a picker shaped like
/// what it holds.
fn builtin(
    which: Picker,
) -> (
    crate::picker::Preview,
    u16,
    crate::picker::PreviewBorder,
    crate::picker::LabelPosition,
) {
    use crate::picker::{LabelPosition as L, Preview as P, PreviewBorder as B};
    match which {
        // Three lines: the chord and what it runs. Wide and short.
        Picker::Keys => (P::Bottom, 30, B::Edge, L::TopCenter),
        // Whatever the other session is doing. Most of the popup, because the
        // question it answers is "what is happening over there".
        Picker::Project => (P::Right, 80, B::Edge, L::BottomCenter),
        // A directory listing, which needs less room than a screen does.
        Picker::Window => (P::Right, 40, B::Edge, L::BottomCenter),
        // A card showing what the theme paints, framed, beside a narrow column
        // of names. The list is the thing being searched and the card is the
        // thing being read, so the column stays narrow and tall.
        Picker::Theme => (P::Right, 70, B::Full, L::Hidden),
        // The command is the row. There is nothing to put beside it.
        Picker::Run => (P::None, 0, B::None, L::Hidden),
        // The command the chosen application would run.
        Picker::Open => (P::Bottom, 30, B::Edge, L::TopCenter),
        // The last few lines of that pane's screen, which is the question
        // "which of these is the one I want" answered without switching.
        // Beside the list rather than under it: the rows are wide and the
        // list is what is being searched.
        Picker::Panes => (P::Right, 50, B::Edge, L::BottomCenter),
    }
}

impl PickerLayout {
    /// One picker's exceptions, as written.
    pub fn overrides(&self, which: Picker) -> &PickerOverride {
        match which {
            Picker::Keys => &self.keys,
            Picker::Project => &self.project,
            Picker::Window => &self.window,
            Picker::Theme => &self.theme,
            Picker::Run => &self.run,
            Picker::Open => &self.open,
            Picker::Panes => &self.panes,
        }
    }

    /// What this picker actually looks like, once every layer has had its say.
    ///
    /// Four of them, narrowest last: the tool's own defaults, then the shape
    /// this particular picker is built for, then whatever `[picker]` says for
    /// all of them, then this picker's own table. A field nobody wrote at
    /// either level keeps the built-in, which is what makes a machine with no
    /// config get pickers shaped like what they hold rather than six of the
    /// same shape.
    pub fn resolved(&self, which: Picker) -> Resolved {
        let (preview, percent, preview_border, preview_label) = builtin(which);
        let mut out = Resolved {
            preview,
            preview_percent: percent,
            look: crate::picker::Look {
                preview_border,
                preview_label_position: preview_label,
                ..crate::picker::Look::default()
            },
        };
        for layer in [&self.global, self.overrides(which)] {
            layer.apply_to(&mut out);
        }
        out
    }
}

/// One picker's settings with nothing left unanswered.
#[derive(Debug, Clone, PartialEq)]
pub struct Resolved {
    /// Where the preview pane goes.
    pub preview: crate::picker::Preview,
    /// The preview's share of the popup.
    pub preview_percent: u16,
    /// The border, the labels, the rules and where each one sits.
    pub look: crate::picker::Look,
}

impl PickerOverride {
    /// Write whatever this layer says over what is there.
    fn apply_to(&self, out: &mut Resolved) {
        let look = &mut out.look;
        if let Some(v) = self.preview {
            out.preview = v;
        }
        if let Some(v) = self.preview_percent {
            out.preview_percent = v;
        }
        if let Some(v) = self.border {
            look.border = v;
        }
        if let Some(v) = self.label_position {
            look.label_position = v;
        }
        if let Some(v) = self.label_offset {
            look.label_offset = v;
        }
        if let Some(v) = self.hint_position {
            look.hint_position = v;
        }
        if let Some(v) = self.prompt_position {
            look.prompt_position = v;
        }
        if let Some(v) = self.list_from {
            look.list_from = v;
        }
        if let Some(v) = self.counter {
            look.counter = v;
        }
        if let Some(v) = self.rules {
            look.rules = v;
        }
        if let Some(v) = self.marker.clone() {
            look.marker = v;
        }
        if let Some(v) = self.column_order.clone() {
            look.column_order = v;
        }
        if let Some(v) = self.min_list_width {
            look.min_list_width = v;
        }
        if let Some(v) = self.preview_border {
            look.preview_border = v;
        }
        if let Some(v) = self.preview_label_position {
            look.preview_label_position = v;
        }
        if let Some(v) = self.preview_label_offset {
            look.preview_label_offset = v;
        }
    }
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
    /// The shell the command runs under. Empty means `$SHELL`.
    ///
    /// It used to default to `zsh`, which on a machine without zsh -- most
    /// Linux boxes -- meant the pane opened and the command never ran.
    pub shell: String,
}

impl Default for Run {
    fn default() -> Self {
        Self {
            history: HistorySource::Auto,
            history_file: None,
            width_percent: 33,
            slide_steps: 5,
            slide_ms: 150,
            shell: String::new(),
        }
    }
}

impl HistorySource {
    /// What `Auto` resolves to, from the name `$SHELL` ends with.
    ///
    /// Unknown shells read as zsh, because the zsh parser passes a plain
    /// one-command-per-line history through untouched and that is what an
    /// unknown shell most likely writes.
    pub fn resolve(self, shell_env: &str) -> Self {
        if self != Self::Auto {
            return self;
        }
        let name = shell_env.rsplit('/').next().unwrap_or(shell_env);
        match name {
            "bash" => Self::Bash,
            "fish" => Self::Fish,
            _ => Self::Zsh,
        }
    }
}

/// Which shell's history to read.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum HistorySource {
    /// Whichever shell `$SHELL` names, falling back to zsh.
    ///
    /// The default, and it used to be `Zsh`. A bash user then got a `run`
    /// picker that read `~/.zsh_history`, found no such file, and showed an
    /// empty list with no error -- the one failure shape that looks like the
    /// feature having nothing to offer.
    #[default]
    Auto,
    /// zsh, plain or extended format.
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
    ///
    /// Deprecated in favour of `[sessions] autosave`, which keeps generations
    /// of its own and knows what each pane was running, where this shells out
    /// to a plugin's save script and keeps one file.
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
            // Off since `[sessions]` arrived, which does the same job and more.
            // A config that still asks for this keeps it, and `config check`
            // says what to move to.
            enabled: false,
            interval_secs: 900,
            script: None,
        }
    }
}

impl Autosave {
    /// The save script, resolved against `home` when the config leaves it out,
    /// and with a leading `~` expanded when it does not.
    pub fn script_path(&self, home: &str) -> PathBuf {
        match &self.script {
            Some(p) => expand_home(p, home),
            None => PathBuf::from(home).join(".config/tmux/plugins/tmux-resurrect/scripts/save.sh"),
        }
    }
}

/// A path from the config with a leading `~` or `~/` replaced by `home`.
///
/// Every path a person writes into `config.toml` goes through this before it
/// is opened. TOML does not expand a tilde and neither does `File::open`, and
/// the one path that skipped this step failed every fifteen minutes for a day
/// with "is missing" while `ls` found the file, because the daemon's stderr
/// went nowhere. A path without a tilde comes back as it was.
pub fn expand_home(path: &std::path::Path, home: &str) -> PathBuf {
    let Some(text) = path.to_str() else {
        return path.to_path_buf();
    };
    if text == "~" {
        return PathBuf::from(home);
    }
    match text.strip_prefix("~/") {
        Some(rest) => PathBuf::from(home).join(rest),
        None => path.to_path_buf(),
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

/// Which programs are coding agents, and when one of them is waiting on you.
///
/// One list, read by everything that asks "is this pane an agent": the
/// `panes --agents` filter, the `agents` segment on the bar, and the restore
/// headline that counts them. It used to be a `match` compiled into the
/// headline, which meant a new agent on the machine was invisible to all three
/// until somebody edited the source.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(deny_unknown_fields, default)]
pub struct Agents {
    /// Programs that are coding agents, matched against `pane_current_command`.
    pub programs: Vec<String>,
    /// Seconds without output after which an agent counts as waiting for you.
    ///
    /// tmux has no per-pane activity time, so this is measured on the window
    /// the agent is in: a shell you are typing into beside the agent keeps it
    /// reading as busy. Ten seconds is long enough that a model thinking
    /// between two tool calls is not called idle, and short enough that a
    /// question left on the screen is noticed before you wonder why nothing is
    /// happening.
    pub waiting_secs: u64,
    /// Seconds between two reads of the pane list for the bar's `agents`
    /// segment and the inbox. One tmux call each, shared by every attached
    /// client.
    pub interval_secs: u64,
    /// Whether the daemon keeps the inbox: the agents that have stopped, with
    /// the last lines of each one's screen captured at the moment it stopped.
    ///
    /// On by default, because the capture is what makes `inbox` answer "what
    /// did it ask" for a window nobody has looked at. The cost is one
    /// `list-panes` per `interval_secs` and one `capture-pane` per stop.
    pub inbox: bool,
    /// Seconds an agent may wait before the daemon says so out loud. Zero,
    /// the default, never nudges: the bar already counts them.
    pub nudge_after_secs: u64,
    /// The command that nudges. Empty is tmux's own `display-message`;
    /// `{program}`, `{at}`, `{waited}` and `{question}` are substituted
    /// anywhere they appear.
    pub nudge_command: Vec<String>,
}

impl Default for Agents {
    fn default() -> Self {
        Self {
            programs: [
                "claude",
                "codex",
                "gemini",
                "cursor-agent",
                "aider",
                "opencode",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect(),
            waiting_secs: 10,
            interval_secs: 2,
            inbox: true,
            nudge_after_secs: 0,
            nudge_command: Vec::new(),
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
    /// Deprecated, and replaced by `dirs_source = "none"`.
    ///
    /// Kept one release so a config written before the directory list had more
    /// than one source keeps behaving as it did: `false` still means the
    /// picker lists live sessions and whatever gets typed, whatever else is
    /// set.
    pub zoxide: bool,
    /// Which directory jumper the picker lists, by name.
    ///
    /// One of [`crate::dirsource::NAMES`]. zoxide is the default rather than
    /// the requirement, and `"none"` leaves the picker with live sessions and
    /// whatever gets typed, which is a smaller tool and still a working one.
    pub dirs_source: String,
    /// A command that prints one absolute path per line, overriding
    /// `dirs_source`.
    ///
    /// The escape hatch for every jumper that is not built in: anything that
    /// can be executed and prints paths works without this crate knowing its
    /// name. Empty means use `dirs_source`.
    pub dirs_command: Vec<String>,
    /// The command that records a visit, with the directory appended.
    ///
    /// Empty means whatever the source does by default, which is `zoxide add`
    /// for zoxide and nothing at all for the rest.
    pub visit_command: Vec<String>,
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
            dirs_source: "zoxide".to_string(),
            dirs_command: Vec::new(),
            visit_command: Vec::new(),
            layout: "default".to_string(),
            override_: Vec::new(),
            preview_window: String::new(),
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

impl Usage {
    /// Where the usage log lives: the configured path with `~` expanded, or
    /// `keys-usage.tsv` under `state_dir`.
    pub fn log_path(&self, home: &str, state_dir: Option<PathBuf>) -> PathBuf {
        match &self.path {
            Some(p) => expand_home(p, home),
            None => state_dir
                .unwrap_or_else(|| PathBuf::from("."))
                .join("keys-usage.tsv"),
        }
    }
}

// Written out rather than derived so the two fields whose default changed
// keep the reason beside them.
#[allow(clippy::derivable_impls)]
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
            // No windows, which is a plain shell: the layout this used to
            // ship, an editor beside an agent, was what one laptop runs, and
            // a machine without either got two windows of "command not
            // found" on its first `start`. config.example.toml keeps that
            // pair as the example to copy.
            layout: Vec::new(),
            project: Project::default(),
            autoreload: Autoreload::default(),
            window_names: WindowNames::default(),
            notify: Notify::default(),
            agents: Agents::default(),
            autosave: Autosave::default(),
            sessions: Sessions::default(),
            restore: Restore::default(),
            run: Run::default(),
            clipboard: Clipboard::default(),
            theme: Theme::default(),
            bar: Bar::default(),
            picker: PickerLayout::default(),
            open: Open::default(),
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

/// How often the daemon takes a snapshot of the whole server.
///
/// An earlier draft had a fourth mode that wrote on every change the daemon
/// noticed. It is gone: somebody who wants a loss window of seconds can set
/// `interval_secs = 10` and read what that costs, which is a better deal than
/// a mode name hiding the same arithmetic behind a word.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum SessionsAutosave {
    /// Nothing on a timer. A snapshot is whatever somebody asked for.
    #[default]
    Off,
    /// Every [`Sessions::interval_secs`].
    Interval,
    /// On the schedule in [`Sessions::cron`].
    Cron,
}

/// The floor under [`Sessions::interval_secs`].
///
/// Ten seconds. Below that the writes start overlapping the capture on a busy
/// machine, and there is nothing sensible for the daemon to do about that
/// except refuse the setting at the point somebody wrote it.
pub const MIN_SESSIONS_INTERVAL_SECS: u64 = 10;

/// Snapshots of the whole tmux server, kept in generations.
///
/// This is the saving half of what a session-restore plugin does, on the
/// daemon's own timer. Restoring stays a command somebody runs, because an
/// automatic restore drops a stale layout over a session already being worked
/// in, which is worse than losing a layout to a reboot.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(deny_unknown_fields, default)]
pub struct Sessions {
    /// Whether the daemon writes snapshots by itself, and on what cadence.
    pub autosave: SessionsAutosave,
    /// Seconds between snapshots under [`SessionsAutosave::Interval`].
    ///
    /// The default costs 0.015% of one core on a 14-pane server and holds five
    /// hours in twenty generations. Ten seconds costs 1.3% and holds two
    /// hundred once, which is the trade nobody expects from a limit counted in
    /// files rather than in time.
    pub interval_secs: u64,
    /// The schedule under [`SessionsAutosave::Cron`].
    pub cron: String,
    /// How many generations to keep.
    pub keep: usize,
    /// Also keep anything younger than this many days, however many files that
    /// turns out to be. Zero means the count decides on its own.
    pub keep_days: u32,
    /// Whether to capture what was on each pane's screen.
    ///
    /// This is the expensive half at 9.3 ms per pane, and it is the half that
    /// makes a short interval costly. Turning it off leaves the metadata,
    /// which is the half carrying what each pane was running and where.
    pub pane_history: bool,
    /// How many lines of each pane to capture.
    pub pane_history_lines: u32,
    /// Sessions never captured, by name.
    ///
    /// On a shutdown this means the session is not saved and does not come
    /// back, since stopping the server takes every session with it either way.
    pub exclude: Vec<String>,
    /// How long the restore summary counts down before going ahead.
    ///
    /// It only opens when the restore does not know something, so this is the
    /// pause on a restore worth reading rather than a confirmation on every
    /// one. Zero goes ahead without drawing it at all, which is `--yes` made
    /// permanent.
    pub confirm_secs: u64,
}

impl Default for Sessions {
    fn default() -> Self {
        Self {
            autosave: SessionsAutosave::Off,
            interval_secs: 900,
            cron: "0 * * * *".to_string(),
            keep: 20,
            keep_days: 0,
            pane_history: true,
            pane_history_lines: 2000,
            exclude: Vec::new(),
            confirm_secs: 5,
        }
    }
}

impl Sessions {
    /// Whether a session by this name is captured.
    pub fn captures(&self, name: &str) -> bool {
        !self.exclude.iter().any(|e| e == name)
    }
}

/// What a restore is allowed to run in a pane it is rebuilding.
///
/// Default deny. A restore executes commands recorded from a machine's own
/// history, and "re-run anything I saw" is one bad afternoon away from
/// restoring a `curl | sh` that was in a pane six weeks ago. A command no row
/// claims is captured, shown and left to the person.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(deny_unknown_fields, default)]
pub struct Restore {
    /// The rows, tried in the order written.
    pub program: Vec<RestoreProgram>,
}

impl Default for Restore {
    fn default() -> Self {
        let row = |pattern: &str, command: &str| RestoreProgram {
            match_: pattern.to_string(),
            command: command.to_string(),
            run: true,
        };
        Self {
            program: vec![
                // An agent keeps which conversation it is in inside its own
                // arguments, and replaying them verbatim is what brings the
                // conversation back. A bare `claude` stays bare.
                row("^claude( |$)", "{command}"),
                row(
                    "^(codex|gemini|cursor-agent|aider|opencode)( |$)",
                    "{command}",
                ),
                // An editor with a session plugin restores itself from the
                // directory, and one without it opens empty. Either way the
                // arguments are a file list from an hour ago and not worth
                // reopening.
                row("^n?vim( |$)", "nvim"),
                row("^(lazygit|tig|gitui)( |$)", "{command}"),
                row("^(htop|top|btop|watch)( |$)", "{command}"),
                row("^(tail|less|journalctl)( |$)", "{command}"),
                row("^ssh( |$)", "{command}"),
            ],
        }
    }
}

/// One row of the restore table.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RestoreProgram {
    /// A regular expression matched against the whole saved command.
    ///
    /// Against the whole command, not the process name, because the process
    /// name of an agent is its version string and everything worth matching on
    /// is in the arguments.
    #[serde(rename = "match")]
    pub match_: String,
    /// What to run instead. `{command}` is the saved command verbatim and
    /// `{cwd}` the pane's directory.
    pub command: String,
    /// Whether to run it at all.
    ///
    /// A row with `run = false` is how somebody says "never bring this back"
    /// without leaving it to fall through to the unknown pile and be asked
    /// about every time.
    #[serde(default = "yes")]
    pub run: bool,
}

impl RestoreProgram {
    /// Whether this row claims a saved command.
    ///
    /// An unparseable pattern matches nothing rather than panicking, the same
    /// bargain [`JobEntry::matches`] makes: one bad row costs its own line and
    /// not the restore.
    pub fn matches(&self, command: &str) -> bool {
        match regex::Regex::new(&self.match_) {
            Ok(re) => re.is_match(command),
            Err(_) => false,
        }
    }

    /// The command to run, with the placeholders filled in.
    pub fn render(&self, command: &str, cwd: &str) -> String {
        self.command
            .replace("{command}", command)
            .replace("{cwd}", cwd)
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
    /// How many coding agents are running, and how many are waiting on you.
    ///
    /// Not on the default side: a machine with no agents draws nothing for
    /// it, but the daemon would still read the pane list every
    /// `[agents] interval_secs` for a segment nobody asked for.
    Agents,
    /// One mark when the daemon knows something needs a look.
    Health,
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
    /// The file the daemon's stderr goes to. Empty means `daemon.log` in the
    /// state directory.
    ///
    /// A daemon a client starts has no terminal, so this file is where "the
    /// autosave failed" and "the config does not parse" end up. Before it
    /// existed they went to `/dev/null`, and an autosave that failed every
    /// fifteen minutes for a day was found by chance.
    pub log: Option<PathBuf>,
}

impl General {
    /// Where the daemon's stderr goes: the configured file with `~` expanded,
    /// or `daemon.log` under `state_dir`.
    pub fn log_path(&self, home: &str, state_dir: Option<PathBuf>) -> Option<PathBuf> {
        match &self.log {
            Some(p) => Some(expand_home(p, home)),
            None => state_dir.map(|d| d.join("daemon.log")),
        }
    }
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
    ///
    /// The tail the ellipsis keeps is fixed in `segments/git.rs`; a
    /// `branch_tail_len` key was documented for a while and never read, and
    /// is gone rather than wired.
    pub branch_max_len: usize,
    /// What the segment draws, and in what order.
    ///
    /// A part left out of this list is not rendered. Somebody working in a
    /// tree with four hundred untracked build artifacts does not want a count
    /// of them, and somebody who never pushes does not want ahead and behind.
    pub parts: Vec<GitPart>,
    /// Which branch-name prefixes earn which glyph, in the order they are
    /// tried.
    ///
    /// The list replaces the built-ins rather than adding to them, so
    /// `tmux-companion config dump` prints the defaults in the shape you edit
    /// them in. A name matching nothing here keeps the plain branch glyph, so
    /// main, master and dev are not special cases.
    pub branch_types: Vec<BranchType>,
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
            parts: GitPart::all(),
            branch_types: default_branch_types(),
            autofetch: Autofetch::default(),
        }
    }
}

/// A group of branch-name prefixes and the glyph they earn.
///
/// Grouped by glyph rather than one row per prefix, because the spellings of
/// one idea (`feat/`, `feature/`, `features/`) are what a list is for and
/// repeating the icon beside each of them is what makes a config file long
/// enough that nobody edits it.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
#[serde(deny_unknown_fields, default)]
pub struct BranchType {
    /// The glyph drawn before the branch name.
    ///
    /// `{NAME}` expands to the glyph of that name in `src/tmux/icons.rs`, the
    /// same spelling `[status.right] separator_before` uses, so the file stays
    /// readable in an editor with no patched font. Anything else is drawn as
    /// written, which is how a literal emoji or a codepoint your font does
    /// have gets in.
    pub icon: String,
    /// The prefixes that earn it, compared case-insensitively and cut off the
    /// name the bar draws.
    ///
    /// Plain prefixes rather than regular expressions: `posts/` is what people
    /// actually name branches, and an unanchored pattern matching in the
    /// middle of a branch name is a bug report nobody enjoys.
    pub prefixes: Vec<String>,
}

impl BranchType {
    /// The markup this entry draws, with `{NAME}` resolved.
    pub fn glyph(&self) -> String {
        expand_glyphs(&self.icon)
    }

    /// What is left of `branch` after the first prefix that claims it, or
    /// `None` when none does.
    pub fn strip<'a>(&self, branch: &'a str) -> Option<&'a str> {
        self.prefixes.iter().find_map(|p| {
            let head = branch.get(..p.len())?;
            head.eq_ignore_ascii_case(p).then(|| &branch[p.len()..])
        })
    }
}

/// The prefixes the bar knew before any of this was configurable.
///
/// Kept as the default rather than as a hardcoded fallback so that a config
/// dump shows them, and so that dropping one is done by deleting a line rather
/// than by finding a flag that turns it off.
fn default_branch_types() -> Vec<BranchType> {
    let group = |icon: &str, prefixes: &[&str]| BranchType {
        icon: icon.to_string(),
        prefixes: prefixes.iter().map(|p| p.to_string()).collect(),
    };
    vec![
        group("{FEATURE}", &["feat/", "feature/", "features/"]),
        group("{BUGFIX}", &["fix/", "fixes/", "bugfix/", "bugfixes/"]),
        group("{HOTFIX}", &["hotfix/"]),
        group("{CHORE}", &["chore/", "chores/"]),
        group("{RELEASE}", &["release/", "releases/"]),
        group("{TAG}", &["tag/", "tags/"]),
    ]
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

/// What a parsed config can still get wrong, which serde's types cannot say.
///
/// `dirs_source` is a name out of a fixed list, and a typo in it would
/// otherwise fall back to zoxide and look like the setting being ignored.
/// `[sessions] interval_secs` has a floor, and a number under it would
/// otherwise be accepted and then quietly clamped, which is the same failure
/// wearing a different hat.
fn validate(config: Config, path: &std::path::Path) -> Result<Config, ConfigError> {
    let secs = config.sessions.interval_secs;
    if secs < MIN_SESSIONS_INTERVAL_SECS {
        return Err(ConfigError {
            path: path.to_path_buf(),
            message: format!(
                "`[sessions] interval_secs` is {secs}, and the floor is {MIN_SESSIONS_INTERVAL_SECS}: below that the writes start overlapping the capture on a busy machine"
            ),
            did_you_mean: Some(MIN_SESSIONS_INTERVAL_SECS.to_string()),
        });
    }

    let name = &config.project.dirs_source;
    if crate::dirsource::DirsSource::from_name(name).is_none() {
        let known: Vec<String> = crate::dirsource::NAMES
            .iter()
            .map(|s| (*s).to_string())
            .collect();
        return Err(ConfigError {
            path: path.to_path_buf(),
            message: format!(
                "unknown value for `project.dirs_source`: `{name}`, expected one of `{}`",
                known.join("`, `")
            ),
            did_you_mean: closest_key(name, &known),
        });
    }
    Ok(config)
}

/// What the config says that still works but should be spelled another way.
///
/// One line each, printed by `config check`. A deprecation nobody is told
/// about is a deprecation that surprises somebody on the release that removes
/// it, and `config check` is where they are already looking.
pub fn deprecations(config: &Config) -> Vec<String> {
    let mut out = Vec::new();
    if config.autosave.enabled {
        out.push(
            "`[autosave]` is deprecated; `[sessions] autosave` keeps generations of its own and records what each pane was running"
                .to_string(),
        );
    }
    if !config.project.zoxide {
        out.push(
            "`[project] zoxide = false` is deprecated; use `dirs_source = \"none\"`".to_string(),
        );
    }
    out
}

/// Parse a config from TOML text, naming the file in any error.
pub fn parse(text: &str, path: &std::path::Path) -> Result<Config, ConfigError> {
    match toml::from_str::<Config>(text) {
        Ok(c) => validate(c, path),
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

/// What `config init` writes.
///
/// Short on purpose. `config dump` prints every setting and
/// `docs/config.example.toml` explains every one, and neither is a file
/// anybody wants to start editing; this is the four decisions a new install
/// actually has to make, with the rest left to the defaults. The layout pair
/// is commented out because a project that opens as one plain shell is the
/// shipped default and the pair is one laptop's habit, not a recommendation.
pub const STARTER: &str = r#"# tmux-companion configuration. `tmux-companion config check` says whether it
# parses, `config dump` prints every setting with its default, and
# docs/config.example.toml explains each one.

[glyphs]
# "nerd-font-v3" needs a patched font; "ascii" draws with plain characters.
preset = "nerd-font-v3"

[project]
# Where the project picker gets its directories: zoxide, z, cdr, ghq or none.
dirs_source = "zoxide"

[sessions]
# Snapshot every session on a timer: "interval", "cron" or "off".
autosave = "interval"
interval_secs = 900

# Uncomment the pair below to open every project as an editor beside an agent.
#
# [[layout]]
# name = "default"
#
# [[layout.window]]
# name = "edit"
# command = "nvim"
#
# [[layout.window]]
# name = "ai"
# command = "claude"
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_starter_parses_and_names_only_real_keys() {
        // `parse` goes through `deny_unknown_fields` on every table, so a key
        // this file sets that the struct does not have fails here rather than
        // on somebody's first `config check`.
        let path = std::path::Path::new("/tmp/starter.toml");
        let c = parse(STARTER, path).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(c.glyphs.preset, Preset::NerdFontV3);
        assert_eq!(c.sessions.autosave, SessionsAutosave::Interval);
        assert_eq!(c.sessions.interval_secs, 900);
        // The layout pair is a comment, so the starter opens a plain shell.
        assert!(c.layout.is_empty());
    }

    #[test]
    fn the_starter_layout_pair_parses_once_uncommented() {
        // The commented lines are meant to be uncommented, so they had better
        // be valid TOML for the struct once they are.
        let uncommented: String = STARTER
            .lines()
            .map(|l| {
                l.strip_prefix("# ")
                    .filter(|r| r.starts_with('[') || r.contains(" = "))
                    .unwrap_or(l)
            })
            .collect::<Vec<_>>()
            .join("\n");
        let c = parse(&uncommented, std::path::Path::new("/tmp/starter.toml"))
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(c.layout.len(), 1);
        assert_eq!(c.layout[0].window.len(), 2);
        assert_eq!(c.layout[0].window[1].command, "claude");
    }

    #[test]
    fn a_tilde_path_expands_against_home_and_a_bare_one_does_not() {
        use std::path::Path;
        assert_eq!(
            expand_home(Path::new("~/x/y.sh"), "/home/me"),
            PathBuf::from("/home/me/x/y.sh")
        );
        assert_eq!(
            expand_home(Path::new("~"), "/home/me"),
            PathBuf::from("/home/me")
        );
        assert_eq!(
            expand_home(Path::new("/abs/y.sh"), "/home/me"),
            PathBuf::from("/abs/y.sh")
        );
        // `~user` is the shell's business, not this tool's.
        assert_eq!(
            expand_home(Path::new("~bob/y"), "/home/me"),
            PathBuf::from("~bob/y")
        );
    }

    #[test]
    fn the_autosave_script_written_with_a_tilde_resolves() {
        // The bug: `script = "~/.config/.../save.sh"` was opened literally and
        // reported missing while the file was there.
        let a = Autosave {
            script: Some(PathBuf::from(
                "~/.config/tmux/plugins/tmux-resurrect/scripts/save.sh",
            )),
            ..Autosave::default()
        };
        assert_eq!(
            a.script_path("/home/me"),
            PathBuf::from("/home/me/.config/tmux/plugins/tmux-resurrect/scripts/save.sh")
        );
        assert_eq!(
            Autosave::default().script_path("/home/me"),
            PathBuf::from("/home/me/.config/tmux/plugins/tmux-resurrect/scripts/save.sh")
        );
    }

    #[test]
    fn the_daemon_log_defaults_to_the_state_dir_and_honours_a_tilde() {
        let state = Some(PathBuf::from("/state"));
        assert_eq!(
            General::default().log_path("/home/me", state.clone()),
            Some(PathBuf::from("/state/daemon.log"))
        );
        let g = General {
            log: Some(PathBuf::from("~/d.log")),
        };
        assert_eq!(
            g.log_path("/home/me", state),
            Some(PathBuf::from("/home/me/d.log"))
        );
        assert_eq!(General::default().log_path("/home/me", None), None);
    }

    #[test]
    fn the_applications_open_offers_are_read_as_a_list() {
        let c = parse(
            r#"
[open]
split = "bottom"

[[open.application]]
name = "chrome"
command = "open -a 'Google Chrome' {url}"

[[open.application]]
name = "vim"
command = "nvim {path}"
pane = true
"#,
            std::path::Path::new("test.toml"),
        )
        .expect("parses");
        assert_eq!(c.open.applications.len(), 2);
        assert_eq!(c.open.applications[0].name, "chrome");
        assert!(!c.open.applications[0].pane, "a browser wants no pane");
        assert_eq!(c.open.applications[1].name, "vim");
        assert!(c.open.applications[1].pane, "an editor wants one");
    }

    #[test]
    fn one_picker_can_be_given_its_own_look() {
        let c = parse(
            r#"
[picker]
border = "plain"
counter = true

[picker.keys]
border = "double"
preview = "bottom"
"#,
            std::path::Path::new("test.toml"),
        )
        .expect("parses");
        let keys = c.picker.resolved(Picker::Keys);
        assert_eq!(keys.look.border, crate::picker::BorderKind::Double);
        assert_eq!(keys.preview, crate::picker::Preview::Bottom);
        // And what it did not override still comes from [picker].
        assert!(keys.look.counter);
        // Another picker keeps the shared answer.
        let theme = c.picker.resolved(Picker::Theme);
        assert_eq!(theme.look.border, crate::picker::BorderKind::Plain);
    }

    #[test]
    fn a_global_setting_does_not_stop_a_picker_using_its_own_default() {
        // This is what the old "equals the default means nobody wrote it" test
        // got wrong. `bottom-center` is a real preference somebody typed, and
        // it happened to make every picker's own answer for the *other*
        // settings unreachable. Layers, not comparisons.
        let c = parse(
            r#"
[picker]
preview_label_position = "bottom-center"

[picker.theme]
border = "none"
"#,
            std::path::Path::new("test.toml"),
        )
        .expect("parses");

        let theme = c.picker.resolved(Picker::Theme);
        // Written globally, so it wins over the theme picker's own.
        assert_eq!(
            theme.look.preview_label_position,
            crate::picker::LabelPosition::BottomCenter
        );
        // Not written anywhere, so the theme picker's own answers stand: a
        // framed card beside a narrow column of names.
        assert_eq!(theme.preview, crate::picker::Preview::Right);
        assert_eq!(theme.preview_percent, 70);
        assert_eq!(
            theme.look.preview_border,
            crate::picker::PreviewBorder::Full
        );
        // And its own table still wins over both.
        assert_eq!(theme.look.border, crate::picker::BorderKind::None);

        // A different picker keeps its own shape throughout.
        let keys = c.picker.resolved(Picker::Keys);
        assert_eq!(keys.preview, crate::picker::Preview::Bottom);
        assert_eq!(keys.preview_percent, 30);
    }

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
    fn the_defaults_deprecate_nothing() {
        assert!(deprecations(&Config::default()).is_empty());
    }

    #[test]
    fn the_old_zoxide_flag_is_called_out() {
        let c = parse(
            "[project]\nzoxide = false\n",
            std::path::Path::new("t.toml"),
        )
        .unwrap();
        let notes = deprecations(&c);
        assert_eq!(notes.len(), 1);
        assert!(notes[0].contains("dirs_source"), "{}", notes[0]);
    }

    #[test]
    fn an_unknown_dirs_source_names_the_ones_that_exist() {
        let e = parse(
            "[project]\ndirs_source = \"zoxid\"\n",
            std::path::Path::new("t.toml"),
        )
        .unwrap_err();
        let s = e.to_string();
        assert!(s.contains("dirs_source"), "{s}");
        assert!(s.contains("zoxide"), "{s}");
        assert_eq!(e.did_you_mean.as_deref(), Some("zoxide"));
    }

    #[test]
    fn every_dirs_source_name_parses() {
        for name in crate::dirsource::NAMES {
            let text = format!("[project]\ndirs_source = \"{name}\"\n");
            let c = parse(&text, std::path::Path::new("t.toml")).unwrap();
            assert_eq!(c.project.dirs_source, name);
        }
    }

    #[test]
    fn a_dirs_command_is_a_list_of_words() {
        // One string would have to be split, and splitting a command line
        // correctly is a parser nobody wants in a config loader.
        let c = parse(
            "[project]\ndirs_command = [\"ghq\", \"list\", \"-p\"]\n",
            std::path::Path::new("t.toml"),
        )
        .unwrap();
        assert_eq!(c.project.dirs_command, vec!["ghq", "list", "-p"]);
    }

    #[test]
    fn auto_reads_the_history_of_the_shell_you_actually_run() {
        use HistorySource as S;
        assert_eq!(S::Auto.resolve("/bin/bash"), S::Bash);
        assert_eq!(S::Auto.resolve("/usr/local/bin/fish"), S::Fish);
        assert_eq!(S::Auto.resolve("/bin/zsh"), S::Zsh);
        assert_eq!(S::Auto.resolve("bash"), S::Bash);
    }

    #[test]
    fn an_unknown_shell_reads_as_zsh() {
        // The zsh parser passes a plain one-command-per-line history through
        // untouched, which is what an unknown shell most likely writes, so it
        // is the safe fallback rather than an empty list.
        use HistorySource as S;
        assert_eq!(S::Auto.resolve("/bin/ksh"), S::Zsh);
        assert_eq!(S::Auto.resolve(""), S::Zsh);
    }

    #[test]
    fn a_named_history_source_ignores_the_environment() {
        // Somebody who wrote `history = "bash"` means it, whatever $SHELL says
        // -- that is the whole reason the setting still exists.
        use HistorySource as S;
        assert_eq!(S::Bash.resolve("/bin/zsh"), S::Bash);
        assert_eq!(S::Atuin.resolve("/bin/bash"), S::Atuin);
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
    fn with_no_config_a_project_opens_as_a_plain_shell() {
        // The editor-and-agent pair that used to ship here was what one laptop
        // runs; a machine without nvim or claude got two windows of "command
        // not found" on its first `start`.
        let c = Config::default();
        assert!(c.layout_for("/anywhere", "/home/me").is_none());
        assert_eq!(
            c.project.layout, "default",
            "the name a config's layout gets by writing one"
        );
    }

    #[test]
    fn the_previewed_window_defaults_to_the_current_one_and_can_be_named() {
        assert_eq!(Project::default().preview_window, "");
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
    fn an_interval_under_the_floor_is_refused_with_both_numbers() {
        let text = "[sessions]\nautosave = \"interval\"\ninterval_secs = 5\n";
        let err = parse(text, std::path::Path::new("c.toml")).expect_err("under the floor");
        let said = err.to_string();
        assert!(said.contains("interval_secs"), "{said}");
        assert!(said.contains('5'), "{said}");
        assert!(
            said.contains(&MIN_SESSIONS_INTERVAL_SECS.to_string()),
            "{said}"
        );
    }

    #[test]
    fn the_floor_itself_is_allowed() {
        let text = "[sessions]\nautosave = \"interval\"\ninterval_secs = 10\n";
        let config = parse(text, std::path::Path::new("c.toml")).expect("at the floor");
        assert_eq!(config.sessions.interval_secs, 10);
        assert_eq!(config.sessions.autosave, SessionsAutosave::Interval);
    }

    #[test]
    fn the_mode_that_was_cut_is_refused_by_name_and_the_three_are_listed() {
        // `aggressive` was a real mode in the design document before the
        // numbers were measured, so somebody who read that draft will write it.
        let text = "[sessions]\nautosave = \"aggressive\"\n";
        let err = parse(text, std::path::Path::new("c.toml")).expect_err("no such mode");
        let said = err.to_string();
        assert!(said.contains("aggressive"), "{said}");
        for mode in ["off", "interval", "cron"] {
            assert!(said.contains(mode), "{mode} missing from: {said}");
        }
    }

    #[test]
    fn a_misspelled_sessions_key_names_the_one_that_was_meant() {
        let text = "[sessions]\nkeep_day = 3\n";
        let err = parse(text, std::path::Path::new("c.toml")).expect_err("no such key");
        assert_eq!(err.did_you_mean.as_deref(), Some("keep_days"));
    }

    #[test]
    fn the_defaults_are_off_with_twenty_generations_and_history_on() {
        let sessions = Sessions::default();
        assert_eq!(sessions.autosave, SessionsAutosave::Off);
        assert_eq!(sessions.interval_secs, 900);
        assert_eq!(sessions.keep, 20);
        assert_eq!(sessions.keep_days, 0);
        assert!(sessions.pane_history);
        assert_eq!(sessions.pane_history_lines, 2000);
        assert!(sessions.exclude.is_empty());
    }

    #[test]
    fn an_excluded_session_is_not_captured_and_the_rest_still_are() {
        let sessions = Sessions {
            exclude: vec!["y".to_string()],
            ..Sessions::default()
        };
        assert!(!sessions.captures("y"));
        assert!(sessions.captures("mysetup"));
        // Not a prefix match: a session called `yogesh_lonkar_org` is a
        // different session from `y` and stays captured.
        assert!(sessions.captures("yogesh_lonkar_org"));
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
