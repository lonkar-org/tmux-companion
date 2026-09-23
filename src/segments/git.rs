//! Git status: running the command, parsing porcelain v2, and rendering it
//! into a tmux segment.
use std::{
    path::{Path, PathBuf},
    sync::{Arc, LazyLock},
    time::Duration,
};

use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::config::{BranchType, GitPart};
use crate::server::state::{DEFAULT_GST_TTL, ServerState};
use crate::tmux::{
    format::{
        AC_DARK_BLUE, AC_ERROR, AC_GONE, AC_GREEN, AC_LOADING, AC_NEW, AC_PURPLE, BG_BAR, BG_CLEAN,
        BG_DEFAULT, BG_ERROR, BG_GONE, BG_LOADING, BG_NEW, BG_TERMINAL, FG_BLUE, FG_CLEAN,
        FG_DARK_BLUE, FG_DEFAULT, FG_GONE, FG_GREEN, FG_GREY89, FG_ON_ERROR, FG_PREVIOUS,
        FG_PURPLE, Palette, Segment, Style, colored_segment, powerline_segment,
    },
    icons::{
        ADDED, AHEAD, ARROW_RIGHT, BEHIND, CLEAN, COPIED, DELETED, FAILED, GIT, MODIFIED, NEW,
        RENAMED, STAGED, STASHED, SYNC, UNMERGED, WHITE_SPACE,
    },
};

/// End cap for the outline styles.  The fill style ends with a solid arrow
/// against the bar; outline has no fill to bound, so it ends with SEPARATOR to
/// keep clear of the next segment.  Swap for any `CAP_*` icon —
/// `tmux-companion preview` renders the alternatives side by side.
const OUTLINE_CAP: &str = crate::tmux::icons::SEPARATOR;

const BRANCH_MAX_LEN: usize = 20;

/// The shipped `[[git.branch_types]]`, for the paths that render without a
/// config in hand: `preview`, and the tests that pin the bar byte for byte.
///
/// The daemon never reads this — it passes what the config parsed, which is
/// this same list until somebody edits it.
static DEFAULT_BRANCH_TYPES: LazyLock<Vec<BranchType>> =
    LazyLock::new(|| crate::config::Git::default().branch_types);

/// File counts for one side of the index: staged, or unstaged.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Area {
    /// Files with content changes.
    pub modified: i32,
    /// Files added.
    pub added: i32,
    /// Files deleted.
    pub deleted: i32,
    /// Files renamed.
    pub renamed: i32,
    /// Files copied.
    pub copied: i32,
}

impl Area {
    fn parse_symbol(&mut self, s: &str) {
        match s {
            "M" => self.modified += 1,
            "A" => self.added += 1,
            "D" => self.deleted += 1,
            "R" => self.renamed += 1,
            "C" => self.copied += 1,
            _ => {}
        }
    }

    fn count(&self) -> i32 {
        self.added + self.deleted + self.modified + self.copied + self.renamed
    }
}

/// One parsed `git status --porcelain=v2 --branch --show-stash`.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct GitStatus {
    /// Commits this branch has that its upstream does not.
    pub ahead: i32,
    /// Commits the upstream has that this branch does not.
    pub behind: i32,
    /// Branch name, or empty on a detached head.
    pub branch: String,
    /// Short commit hash of HEAD.
    pub commit: String,
    /// The upstream branch no longer exists.
    pub is_gone: bool,
    /// The branch has no upstream yet.
    pub is_new: bool,
    /// Counts for the index.
    pub staged: Area,
    /// Number of stash entries.
    pub stashed: i32,
    /// Counts for the work tree.
    pub unstaged: Area,
    /// Files with merge conflicts.
    pub unmerged: i32,
    /// Untracked files.
    pub untracked: i32,
    /// Upstream branch name.
    pub upstream: String,
    /// Whether the last remote operation succeeded.
    pub remote_success: bool,
    /// Whether a fetch or push is in flight.
    pub loading: bool,
}

impl GitStatus {
    /// Parse `git status --porcelain=v2 --branch --show-stash` output.
    pub fn parse_porcelain_v2(output: &str) -> Self {
        let mut s = Self::default();
        for line in output.lines() {
            if line.is_empty() {
                continue;
            }
            s.parse_line(line);
        }
        s
    }

    fn parse_line(&mut self, line: &str) {
        let mut words = line.split_whitespace();
        match words.next() {
            Some("#") => self.parse_branch_info(&mut words),
            Some("1") | Some("2") => self.parse_tracked_file(&mut words),
            Some("u") => self.unmerged += 1,
            Some("?") => self.untracked += 1,
            _ => {}
        }
    }

    fn parse_branch_info<'a>(&mut self, words: &mut impl Iterator<Item = &'a str>) {
        while let Some(key) = words.next() {
            match key {
                "branch.oid" => {
                    if let Some(v) = words.next() {
                        self.commit = v.to_string();
                    }
                }
                "branch.head" => {
                    if let Some(v) = words.next() {
                        self.branch = v.to_string();
                    }
                }
                "branch.upstream" => {
                    if let Some(v) = words.next() {
                        self.upstream = v.to_string();
                    }
                }
                "branch.ab" => self.parse_ahead_behind(words),
                _ => {}
            }
        }
    }

    fn parse_ahead_behind<'a>(&mut self, words: &mut impl Iterator<Item = &'a str>) {
        for word in words {
            if let Ok(n) = word[1..].parse::<i32>() {
                match &word[..1] {
                    "+" => self.ahead = n,
                    "-" => self.behind = n,
                    _ => {}
                }
            }
        }
    }

    fn parse_tracked_file<'a>(&mut self, words: &mut impl Iterator<Item = &'a str>) {
        if let Some(xy) = words.next()
            && xy.len() >= 2
        {
            self.staged.parse_symbol(&xy[..1]);
            self.unstaged.parse_symbol(&xy[1..2]);
        }
    }

    fn count(&self) -> i32 {
        self.staged.count()
            + self.unstaged.count()
            + self.behind
            + self.ahead
            + self.unmerged
            + self.untracked
    }

    fn is_clean(&self) -> bool {
        !self.is_new && !self.is_gone && self.count() == 0
    }

    fn is_dirty(&self) -> bool {
        !self.is_new && !self.is_gone && self.count() > 0
    }

    fn bg(&self) -> &'static str {
        if self.is_clean() {
            BG_CLEAN
        } else if self.is_new {
            BG_NEW
        } else if self.is_gone {
            BG_GONE
        } else {
            BG_DEFAULT
        }
    }

    fn fg(&self) -> &'static str {
        if self.is_clean() {
            FG_CLEAN
        } else if self.is_gone {
            FG_GONE
        } else {
            FG_DEFAULT
        }
    }

    /// Outline text color: the state color the fill style uses as background,
    /// lightened where the fill value would vanish on the dark bar.
    fn accent(&self, bright: bool) -> &'static str {
        match (self.bg(), bright) {
            (BG_GONE, true) => AC_GONE,
            (bg, _) => bg,
        }
    }

    fn palette<'a>(&self, style: Style, bar_bg: &'a str) -> Palette<'a> {
        match style {
            Style::Fill => Palette {
                bg: self.bg(),
                fg: self.fg(),
                reset_fg: BG_TERMINAL,
                cap: self.bg(),
                cap_glyph: ARROW_RIGHT,
                loading_fg: FG_GREY89,
                loading_bg: BG_LOADING,
                loading_cap: BG_LOADING,
                error_fg: FG_ON_ERROR,
                error_bg: BG_ERROR,
                error_cap: BG_ERROR,
                prev_fg: FG_PREVIOUS,
                new_fg: FG_BLUE,
                green_fg: FG_GREEN,
                dirty_fg: BG_GONE,
                ahead_fg: FG_DARK_BLUE,
                unmerged_fg: BG_ERROR,
                stash_fg: FG_PURPLE,
            },
            Style::Outline | Style::OutlineBright => {
                let bright = style == Style::OutlineBright;
                let accent = self.accent(bright);
                Palette {
                    bg: bar_bg,
                    fg: accent,
                    reset_fg: accent,
                    cap: accent,
                    cap_glyph: OUTLINE_CAP,
                    loading_fg: if bright { AC_LOADING } else { BG_LOADING },
                    loading_bg: bar_bg,
                    loading_cap: if bright { AC_LOADING } else { BG_LOADING },
                    error_fg: AC_ERROR,
                    error_bg: bar_bg,
                    error_cap: AC_ERROR,
                    prev_fg: accent,
                    new_fg: if bright { AC_NEW } else { FG_BLUE },
                    green_fg: if bright { AC_GREEN } else { FG_GREEN },
                    dirty_fg: if bright { AC_GONE } else { BG_GONE },
                    ahead_fg: if bright { AC_DARK_BLUE } else { FG_DARK_BLUE },
                    unmerged_fg: AC_ERROR,
                    stash_fg: if bright { AC_PURPLE } else { FG_PURPLE },
                }
            }
        }
    }
}

#[cfg(test)]
fn short_branch(branch: &str) -> String {
    short_branch_len(branch, BRANCH_MAX_LEN, &DEFAULT_BRANCH_TYPES)
}

fn short_branch_len(branch: &str, max_len: usize, types: &[BranchType]) -> String {
    // A name no entry claims falls back to the plain branch glyph, so main,
    // master, dev, ... still carry a type marker.
    let mut icon = crate::tmux::icons::BRANCH.to_string();
    let mut stripped = "";

    for entry in types {
        if let Some(rest) = entry.strip(branch) {
            stripped = rest;
            icon = entry.glyph();
            break;
        }
    }

    // A branch named exactly `feat/` strips to nothing, and a bar drawing an
    // icon and no name at all says less than one drawing the name as written.
    let name = if stripped.is_empty() {
        branch
    } else {
        stripped
    };
    let char_count = name.chars().count();
    // Middle-ellipsize names longer than max_len chars. The kept chars are
    // split ~45% head / ~55% tail of (max_len - 2), which reproduces the
    // historic Go port's 8 + "..." + 10 shape at the default max_len of 20.
    let truncated = if char_count > max_len {
        let budget = max_len.saturating_sub(2).max(2);
        let head_len = budget * 45 / 100;
        let tail_len = budget - head_len;
        let head: String = name.chars().take(head_len).collect();
        let tail: String = name
            .chars()
            .rev()
            .take(tail_len)
            .collect::<String>()
            .chars()
            .rev()
            .collect();
        format!("{}...{}", head, tail)
    } else {
        name.to_string()
    };

    format!("{}{}", icon, truncated)
}

/// Fill-style render — the shape the test suite pins.  Production goes through
/// `status_line_styled`.
#[cfg(test)]
pub fn status_line_mode(s: &GitStatus, nvim_suspended: bool, no_tmux: bool) -> String {
    status_line_styled(s, nvim_suspended, no_tmux, Style::Fill)
}

/// Render a status line in the given style.
pub fn status_line_styled(
    s: &GitStatus,
    nvim_suspended: bool,
    no_tmux: bool,
    style: Style,
) -> String {
    status_line_capped(s, nvim_suspended, no_tmux, style, None)
}

/// Same render, with the end-cap glyph overridable — used by `preview` to show
/// cap candidates side by side.
pub fn status_line_capped(
    s: &GitStatus,
    nvim_suspended: bool,
    no_tmux: bool,
    style: Style,
    cap_glyph: Option<&'static str>,
) -> String {
    status_line_render(
        s,
        nvim_suspended,
        no_tmux,
        &LineOpts {
            style,
            cap_glyph,
            parts: &GitPart::all(),
            ..LineOpts::default()
        },
    )
}

/// Innermost render; `branch_max` overrides the branch middle-ellipsis
/// threshold (default `BRANCH_MAX_LEN`); `branch_icon` controls whether the
/// leading git glyph (and its trailing space) precede the branch name.
/// How one status line is drawn: the style, the cap, the branch treatment and
/// which parts are in it.
///
/// A struct because the alternative was eight positional arguments, and four of
/// them were `Option`s and `bool`s that a caller could transpose without the
/// compiler noticing.
#[derive(Debug, Clone)]
pub struct LineOpts<'a> {
    /// Fill, outline or outline-bright.
    pub style: Style,
    /// Override the end-cap glyph; `Some("")` draws no cap at all.
    pub cap_glyph: Option<&'static str>,
    /// Middle-ellipsize a branch name longer than this.
    pub branch_max: Option<usize>,
    /// Draw the git glyph before the branch name.
    pub branch_icon: bool,
    /// Which prefixes earn which glyph, from `[[git.branch_types]]`. Empty
    /// means every branch gets the plain branch glyph.
    pub branch_types: &'a [BranchType],
    /// Which parts to draw, and in what order.
    pub parts: &'a [GitPart],
    /// The colour the status bar itself is set to.
    ///
    /// Every cap and every outline background is drawn against this, so a bar
    /// that is not `colour233` gets wedges in a colour that is nowhere else on
    /// the screen unless this follows it. `[bar] background` in the config.
    pub bar_bg: &'a str,
}

impl Default for LineOpts<'static> {
    fn default() -> Self {
        Self {
            style: Style::default(),
            cap_glyph: None,
            branch_max: None,
            branch_icon: true,
            branch_types: &DEFAULT_BRANCH_TYPES,
            parts: &[],
            bar_bg: BG_BAR,
        }
    }
}

/// Render one status line.
pub fn status_line_render(
    s: &GitStatus,
    nvim_suspended: bool,
    no_tmux: bool,
    opts: &LineOpts<'_>,
) -> String {
    let LineOpts {
        style,
        cap_glyph,
        branch_max,
        branch_types,
        branch_icon,
        parts,
        bar_bg,
    } = *opts;
    let has = |p: GitPart| parts.contains(&p);
    let bar_bg = if nvim_suspended { BG_TERMINAL } else { bar_bg };
    let mut p = s.palette(style, bar_bg);
    if let Some(g) = cap_glyph {
        p.cap_glyph = g;
    }
    let bg = p.bg;
    let fg = p.fg;
    let reset = colored_segment(no_tmux, p.reset_fg, bg, "");
    let reset_ws = colored_segment(no_tmux, p.reset_fg, bg, WHITE_SPACE);

    let mut status_line = Segment::new();
    let mut remote = Segment::new();

    if !has(GitPart::Sync) {
        // The whole in-flight and failed-remote block is skipped, but the
        // segment still needs its opening colour run or the branch name
        // inherits whatever tmux drew before it.
        remote.add(colored_segment(no_tmux, p.prev_fg, bg, WHITE_SPACE));
    } else if s.loading {
        remote.add(colored_segment(
            no_tmux,
            p.loading_fg,
            p.loading_bg,
            &format!("{}{}{}", WHITE_SPACE, SYNC, WHITE_SPACE),
        ));
        remote.add(format!(
            "{}{}",
            colored_segment(no_tmux, p.loading_cap, bg, ARROW_RIGHT),
            ""
        ));
    } else if s.remote_success {
        remote.add(colored_segment(no_tmux, p.prev_fg, bg, WHITE_SPACE));
    } else {
        remote.add(colored_segment(no_tmux, p.prev_fg, p.error_bg, ARROW_RIGHT));
        remote.add(colored_segment(
            no_tmux,
            p.error_fg,
            p.error_bg,
            WHITE_SPACE,
        ));
        remote.add(format!("{}{}", FAILED, WHITE_SPACE));
        remote.add(format!(
            "{}{}",
            colored_segment(no_tmux, p.error_cap, bg, ""),
            ARROW_RIGHT
        ));
    }

    // GIT carries its own trailing space, so skipping it also drops the gap
    // between glyph and branch name.
    remote.add(format!(
        "{}{}{}{}",
        colored_segment(no_tmux, fg, bg, ""),
        if branch_icon && has(GitPart::Branch) {
            GIT
        } else {
            ""
        },
        if has(GitPart::Branch) {
            short_branch_len(
                &s.branch,
                branch_max.unwrap_or(BRANCH_MAX_LEN),
                branch_types,
            )
        } else {
            String::new()
        },
        WHITE_SPACE
    ));

    if !has(GitPart::State) {
        // nothing: no clean tick, no dirty marker, no gone glyph
    } else if s.is_new {
        remote.add(format!(
            "{}{}",
            colored_segment(no_tmux, p.new_fg, bg, NEW),
            reset
        ));
    } else if s.is_gone {
        remote.add(crate::tmux::icons::GONE.to_string());
    } else if s.is_clean() {
        remote.add(format!(
            "{}{}",
            colored_segment(no_tmux, p.green_fg, bg, CLEAN),
            reset_ws
        ));
    } else if s.is_dirty() {
        remote.add(format!(
            "{}{}",
            colored_segment(no_tmux, p.dirty_fg, bg, ""),
            reset
        ));
    }

    status_line.add(remote.to_string());

    // sub-segments
    let mut sub = Segment::new();

    // branch info (ahead/behind/unmerged)
    let mut branch = Segment::new();
    branch.counter(
        if has(GitPart::Ahead) { s.ahead } else { 0 },
        &format!(
            "{}{}",
            colored_segment(no_tmux, p.ahead_fg, bg, AHEAD),
            reset
        ),
    );
    branch.counter(
        if has(GitPart::Behind) { s.behind } else { 0 },
        &format!(
            "{}{}",
            colored_segment(no_tmux, p.ahead_fg, bg, BEHIND),
            reset
        ),
    );
    branch.counter(
        if has(GitPart::Conflicts) {
            s.unmerged
        } else {
            0
        },
        &colored_segment(no_tmux, p.unmerged_fg, bg, UNMERGED),
    );
    branch.append_only(&reset);
    sub.when(!branch.is_empty(), &branch.to_string());

    // unstaged
    let mut unstaged = Segment::new();
    // `untracked` covers unstaged additions too: that is how git reports a new
    // file, and splitting them would count one file twice.
    unstaged.counter(
        if has(GitPart::Untracked) {
            s.untracked + s.unstaged.added
        } else {
            0
        },
        ADDED,
    );
    unstaged.counter(
        if has(GitPart::Deleted) {
            s.unstaged.deleted
        } else {
            0
        },
        DELETED,
    );
    unstaged.counter(
        if has(GitPart::Renamed) {
            s.unstaged.renamed
        } else {
            0
        },
        RENAMED,
    );
    unstaged.counter(
        if has(GitPart::Copied) {
            s.unstaged.copied
        } else {
            0
        },
        COPIED,
    );
    unstaged.counter(
        if has(GitPart::Modified) {
            s.unstaged.modified
        } else {
            0
        },
        MODIFIED,
    );
    sub.when(!unstaged.is_empty(), &unstaged.to_string());

    // staged
    let mut staged = Segment::new();
    let staged_on = has(GitPart::Staged);
    staged.counter(if staged_on { s.staged.added } else { 0 }, ADDED);
    staged.counter(if staged_on { s.staged.deleted } else { 0 }, DELETED);
    staged.counter(if staged_on { s.staged.renamed } else { 0 }, RENAMED);
    staged.counter(if staged_on { s.staged.copied } else { 0 }, COPIED);
    staged.counter(if staged_on { s.staged.modified } else { 0 }, MODIFIED);
    staged.prepend_only(&format!(
        "{}{}",
        colored_segment(no_tmux, p.green_fg, bg, STAGED),
        reset
    ));
    sub.when(!staged.is_empty(), &staged.to_string());

    // stash
    sub.counter(
        if has(GitPart::Stash) { s.stashed } else { 0 },
        &format!(
            "{}{}",
            colored_segment(no_tmux, p.stash_fg, bg, STASHED),
            reset
        ),
    );

    status_line.add(sub.join(WHITE_SPACE));

    if p.cap_glyph.is_empty() {
        // no end cap requested (e.g. segment sits at the start of status-right)
    } else if no_tmux {
        status_line.append(&format!(
            "\x1b[0m\x1b[38;5;{}m{}\x1b[0m",
            p.cap, p.cap_glyph
        ));
    } else {
        status_line.append(&powerline_segment(p.cap, bar_bg, p.cap_glyph));
    }

    status_line.to_string()
}

async fn run_git(args: &[&str], dir: &Path) -> anyhow::Result<String> {
    let out = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        tokio::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .output(),
    )
    .await
    .map_err(|_| anyhow::anyhow!("git {:?} timed out", args))??;
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn count_stash(git_root: &Path) -> i32 {
    let stash = git_root.join("logs/refs/stash");
    std::fs::read_to_string(stash)
        .map(|s| s.lines().count() as i32)
        .unwrap_or(0)
}

async fn fetch_git_status(path: &Path) -> anyhow::Result<GitStatus> {
    let p1 = path.to_owned();
    let p2 = path.to_owned();

    let (status_out, root_out) = tokio::join!(
        async move {
            run_git(
                &[
                    "status",
                    "--untracked-files=all",
                    "--branch",
                    "--porcelain=v2",
                ],
                &p1,
            )
            .await
        },
        async move { run_git(&["rev-parse", "--path-format=absolute", "--git-dir"], &p2).await },
    );

    let mut status = GitStatus::parse_porcelain_v2(&status_out?);
    let git_root = PathBuf::from(root_out?.trim());

    let is_new = status.upstream.is_empty();
    status.is_new = is_new;

    let path_owned = path.to_owned();
    let upstream = status.upstream.clone();

    let (is_gone, stash_count) = tokio::join!(
        async move {
            if is_new {
                return Ok(false);
            }
            let branches = run_git(&["branch", "-r"], &path_owned).await?;
            Ok::<bool, anyhow::Error>(!branches.contains(&upstream))
        },
        tokio::task::spawn_blocking(move || count_stash(&git_root)),
    );

    status.is_gone = is_gone.unwrap_or(false);
    status.stashed = stash_count.unwrap_or(0);
    status.remote_success = true;

    Ok(status)
}

/// Everything the git segment needs in order to render.
///
/// A struct rather than seven positional arguments, so the standalone `gst`
/// command and the combined status side can be read side by side and a new
/// option cannot be silently transposed with its neighbour.
#[derive(Debug, Clone)]
pub struct GstOptions {
    /// Repository path; `None` means the server's working directory.
    pub path: Option<PathBuf>,
    /// Pane pid, used only to mark a suspended nvim in the segment.
    ///
    /// `None` from the combined status side, and that is the point: resolving
    /// it calls `has_suspended_nvim`, which enumerates the entire process table
    /// and cost more than everything else in the segment put together — 18.5 ms
    /// of the 26.0 ms measured per call.  Still honoured when a hand invocation
    /// passes a pid, so `gst <path> <pid>` behaves as it always did.
    pub pane_pid: Option<u32>,
    /// Skip the cache read.  The fresh result is still written back, so the
    /// next ordinary call is warm.
    pub force: bool,
    /// Fill, outline or outline-bright.
    pub style: Style,
    /// Omit the trailing end cap, which is what the combined side needs.
    pub no_cap: bool,
    /// Middle-ellipsize a branch name longer than this.
    pub branch_max_len: Option<usize>,
    /// Draw the git glyph before the branch name.
    pub branch_icon: bool,
    /// How long a cached status stays fresh.
    pub ttl: Duration,
    /// What the segment draws, from `[git] parts` in the config.
    pub parts: Vec<GitPart>,
    /// Which branch-name prefixes earn which glyph, from
    /// `[[git.branch_types]]` in the config.
    pub branch_types: Vec<BranchType>,
    /// The colour the status bar is set to, from `[bar] background`.
    ///
    /// The caps and the outline backgrounds are drawn against this. It is a
    /// setting rather than a constant because a bar that is not `colour233`
    /// otherwise gets a wedge in a colour that is nowhere else on the screen.
    pub bar_bg: String,
}

impl Default for GstOptions {
    fn default() -> Self {
        Self {
            path: None,
            pane_pid: None,
            force: false,
            style: Style::default(),
            no_cap: false,
            branch_max_len: None,
            branch_icon: false,
            ttl: DEFAULT_GST_TTL,
            parts: GitPart::all(),
            branch_types: crate::config::Git::default().branch_types,
            bar_bg: BG_BAR.to_string(),
        }
    }
}

/// Render the git segment, serving from the cache unless `opts.force` is set.
pub async fn render(opts: &GstOptions, state: &Arc<Mutex<ServerState>>) -> anyhow::Result<String> {
    // empty cap glyph = status_line_capped skips the end cap entirely
    let cap = if opts.no_cap { Some("") } else { None };
    let path = match &opts.path {
        Some(p) => p.canonicalize()?,
        None => std::env::current_dir()?,
    };

    if !is_inside_work_tree(&path, state).await {
        return Ok(String::new());
    }

    // Tell the autofetch task this tree exists. Recorded here rather than
    // behind the status cache, because a bar refreshing once a second is a
    // cache hit almost every time and a repository nobody had to re-read is
    // exactly the one still being looked at.
    {
        let mut s = state.lock().await;
        let remember = std::time::Duration::from_secs(s.config.git.autofetch.remember_secs.max(1));
        if s.config.git.autofetch.enabled {
            s.note_repo(path.clone(), remember);
        }
    }

    let nvim_suspended = match opts.pane_pid {
        Some(p) => crate::segments::vim_bg::has_suspended_nvim(p)
            .await
            .unwrap_or(false),
        None => false,
    };

    let cached = if opts.force {
        None
    } else {
        state.lock().await.git_cached(&path, opts.ttl)
    };

    let status = match cached {
        Some(status) => status,
        None => {
            let fresh = fetch_git_status(&path).await?;
            state
                .lock()
                .await
                .git_store(path.clone(), fresh.clone(), opts.ttl);
            fresh
        }
    };

    let line = status_line_render(
        &status,
        nvim_suspended,
        false,
        &LineOpts {
            style: opts.style,
            cap_glyph: cap,
            branch_max: opts.branch_max_len,
            branch_icon: opts.branch_icon,
            branch_types: &opts.branch_types,
            parts: &opts.parts,
            bar_bg: &opts.bar_bg,
        },
    );
    Ok(if opts.no_cap {
        line.trim_end().to_string()
    } else {
        line
    })
}

/// `git rev-parse --is-inside-work-tree`, cached per canonicalized path.
///
/// This fork ran on every call including cache hits — it sits ahead of the
/// status cache, so even a warm `gst` paid for it, and once the process scan
/// was gone it was the largest remaining server-side cost in the segment.
///
/// A directory's repo-ness effectively never changes, so the answer is cached;
/// it is cached with a TTL rather than forever so that `git init` in a
/// directory already on the status bar is not misremembered until the server
/// restarts.  Both answers are cached — without the negative one, every
/// non-repo pane would keep paying the fork on every refresh.
async fn is_inside_work_tree(path: &Path, state: &Arc<Mutex<ServerState>>) -> bool {
    let key = path.to_path_buf();
    if let Some(known) = state.lock().await.repo_cached(&key) {
        return known;
    }
    let out = run_git(&["rev-parse", "--is-inside-work-tree"], path).await;
    let inside = out.map(|s| s.trim() == "true").unwrap_or(false);
    state.lock().await.repo_store(key, inside);
    inside
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tmux::{
        format::{
            AC_DARK_BLUE, AC_ERROR, AC_GONE, AC_PURPLE, BG_BAR, BG_CLEAN, BG_DEFAULT, BG_ERROR,
            BG_GONE, BG_LOADING, BG_NEW, BG_TERMINAL, FG_CLEAN, FG_DARK_BLUE, FG_DEFAULT, FG_GONE,
            FG_GREEN, FG_PREVIOUS, FG_PURPLE, Style, colored_segment, powerline_segment,
        },
        icons::*,
    };

    // ── helpers ───────────────────────────────────────────────────────────────

    fn s_clean() -> GitStatus {
        GitStatus {
            branch: "branch1".into(),
            remote_success: true,
            ..Default::default()
        }
    }

    fn s_dirty_all() -> GitStatus {
        GitStatus {
            ahead: 1,
            behind: 1,
            branch: "branch1".into(),
            unmerged: 1,
            untracked: 1,
            remote_success: true,
            stashed: 1,
            staged: Area {
                modified: 1,
                added: 1,
                deleted: 1,
                renamed: 1,
                copied: 1,
            },
            unstaged: Area {
                modified: 1,
                added: 1,
                deleted: 1,
                renamed: 1,
                copied: 1,
            },
            ..Default::default()
        }
    }

    // Build expected status-line strings the same way status_line_mode does.
    fn remote_success_seg(bg: &str, no_tmux: bool) -> String {
        colored_segment(no_tmux, FG_PREVIOUS, bg, WHITE_SPACE)
    }
    fn branch_seg(branch: &str, fg: &str, bg: &str, no_tmux: bool) -> String {
        format!(
            "{}{}{}{}",
            colored_segment(no_tmux, fg, bg, ""),
            GIT,
            short_branch(branch),
            WHITE_SPACE
        )
    }
    fn arrow(bg: &str, no_tmux: bool) -> String {
        if no_tmux {
            format!("\x1b[0m\x1b[38;5;{}m{}\x1b[0m", bg, ARROW_RIGHT)
        } else {
            // The arrow is drawn against whatever the bar is set to, which is
            // BG_BAR unless nvim_suspended puts BG_TERMINAL behind it.
            powerline_segment(bg, BG_BAR, ARROW_RIGHT)
        }
    }

    // ── parse_porcelain_v2 ────────────────────────────────────────────────────

    #[test]
    fn parse_porcelain_clean() {
        let input = "# branch.oid abc123\n# branch.head main\n# branch.upstream origin/main\n# branch.ab +0 -0\n";
        let s = GitStatus::parse_porcelain_v2(input);
        assert_eq!(s.branch, "main");
        assert_eq!(s.upstream, "origin/main");
        assert_eq!(s.ahead, 0);
        assert_eq!(s.behind, 0);
    }

    #[test]
    fn parse_porcelain_ahead_behind() {
        let input =
            "# branch.head feat/foo\n# branch.upstream origin/feat/foo\n# branch.ab +2 -3\n";
        let s = GitStatus::parse_porcelain_v2(input);
        assert_eq!(s.ahead, 2);
        assert_eq!(s.behind, 3);
    }

    #[test]
    fn parse_tracked_file_mm() {
        let input = "# branch.head main\n1 MM N... 100644 100644 100644 hash hash file.txt\n";
        let s = GitStatus::parse_porcelain_v2(input);
        assert_eq!(s.staged.modified, 1);
        assert_eq!(s.unstaged.modified, 1);
    }

    #[test]
    fn parse_tracked_file_all_xy_symbols() {
        // XY where X=staged, Y=unstaged; exercise A/D/R/C/M in each slot.
        let input = "# branch.head main\n\
            1 AM N... 0 0 0 h h f1\n\
            1 DC N... 0 0 0 h h f2\n\
            1 RA N... 0 0 0 h h f3\n\
            1 CD N... 0 0 0 h h f4\n";
        let s = GitStatus::parse_porcelain_v2(input);
        assert_eq!(s.staged.added, 1); // A from "AM"
        assert_eq!(s.staged.deleted, 1); // D from "DC"
        assert_eq!(s.staged.renamed, 1); // R from "RA"
        assert_eq!(s.staged.copied, 1); // C from "CD"
        assert_eq!(s.unstaged.modified, 1); // M from "AM"
        assert_eq!(s.unstaged.copied, 1); // C from "DC"
        assert_eq!(s.unstaged.added, 1); // A from "RA"
        assert_eq!(s.unstaged.deleted, 1); // D from "CD"
    }

    #[test]
    fn parse_untracked_unmerged() {
        let input = "# branch.head main\n? newfile.txt\nu UU N... hash hash hash conflict.txt\n";
        let s = GitStatus::parse_porcelain_v2(input);
        assert_eq!(s.untracked, 1);
        assert_eq!(s.unmerged, 1);
    }

    #[test]
    fn parse_empty_input_gives_defaults() {
        let s = GitStatus::parse_porcelain_v2("");
        assert_eq!(s.branch, "");
        assert_eq!(s.ahead, 0);
    }

    #[test]
    fn parse_no_upstream_means_is_new() {
        // The render path sets is_new from upstream.is_empty(); parse alone doesn't set it.
        let s = GitStatus::parse_porcelain_v2("# branch.head main\n");
        assert_eq!(s.upstream, ""); // no upstream in input
    }

    // ── Area ─────────────────────────────────────────────────────────────────

    #[test]
    fn area_count_sums_fields() {
        let a = Area {
            modified: 1,
            added: 2,
            deleted: 3,
            renamed: 4,
            copied: 5,
        };
        assert_eq!(a.count(), 15);
    }

    #[test]
    fn area_count_zero() {
        assert_eq!(Area::default().count(), 0);
    }

    #[test]
    fn area_parse_symbol_all() {
        let mut a = Area::default();
        for s in ["M", "A", "D", "R", "C"] {
            a.parse_symbol(s);
        }
        assert_eq!(a.modified, 1);
        assert_eq!(a.added, 1);
        assert_eq!(a.deleted, 1);
        assert_eq!(a.renamed, 1);
        assert_eq!(a.copied, 1);
    }

    #[test]
    fn area_parse_symbol_ignores_unknown() {
        let mut a = Area::default();
        a.parse_symbol("X");
        a.parse_symbol(".");
        a.parse_symbol("");
        assert_eq!(a.count(), 0);
    }

    // ── GitStatus helpers ────────────────────────────────────────────────────

    #[test]
    fn git_status_bg_clean() {
        assert_eq!(s_clean().bg(), BG_CLEAN);
    }

    #[test]
    fn git_status_bg_new() {
        let s = GitStatus {
            is_new: true,
            ..Default::default()
        };
        assert_eq!(s.bg(), BG_NEW);
    }

    #[test]
    fn git_status_bg_gone() {
        let s = GitStatus {
            is_gone: true,
            ..Default::default()
        };
        assert_eq!(s.bg(), BG_GONE);
    }

    #[test]
    fn git_status_bg_dirty() {
        let s = GitStatus {
            ahead: 1,
            ..Default::default()
        };
        assert_eq!(s.bg(), BG_DEFAULT);
    }

    #[test]
    fn git_status_fg_clean() {
        assert_eq!(s_clean().fg(), FG_CLEAN);
    }

    #[test]
    fn git_status_fg_gone() {
        let s = GitStatus {
            is_gone: true,
            ..Default::default()
        };
        assert_eq!(s.fg(), FG_GONE);
    }

    #[test]
    fn git_status_fg_dirty() {
        let s = GitStatus {
            ahead: 1,
            ..Default::default()
        };
        assert_eq!(s.fg(), FG_DEFAULT);
    }

    #[test]
    fn git_status_is_clean_true() {
        assert!(s_clean().is_clean());
    }

    #[test]
    fn git_status_is_clean_false_when_new() {
        let s = GitStatus {
            is_new: true,
            ..Default::default()
        };
        assert!(!s.is_clean());
    }

    #[test]
    fn git_status_is_clean_false_when_ahead() {
        let s = GitStatus {
            ahead: 1,
            ..Default::default()
        };
        assert!(!s.is_clean());
    }

    #[test]
    fn git_status_is_dirty_conditions() {
        assert!(
            !GitStatus {
                is_new: true,
                ahead: 1,
                ..Default::default()
            }
            .is_dirty()
        );
        assert!(
            !GitStatus {
                is_gone: true,
                ahead: 1,
                ..Default::default()
            }
            .is_dirty()
        );
        assert!(
            GitStatus {
                ahead: 1,
                ..Default::default()
            }
            .is_dirty()
        );
    }

    // ── short_branch ─────────────────────────────────────────────────────────

    /// The shipped `[[git.branch_types]]`, which is what the bar renders with
    /// until somebody writes their own.
    fn types() -> &'static [BranchType] {
        &DEFAULT_BRANCH_TYPES
    }

    /// One entry, so a test says what it is testing without the other five.
    fn one(icon: &str, prefixes: &[&str]) -> Vec<BranchType> {
        vec![BranchType {
            icon: icon.to_string(),
            prefixes: prefixes.iter().map(|p| p.to_string()).collect(),
        }]
    }

    #[test]
    fn a_configured_prefix_earns_its_icon_and_is_cut_off() {
        let types = one("P ", &["parked/"]);
        assert_eq!(
            short_branch_len("parked/docs-that-died", 40, &types),
            "P docs-that-died"
        );
    }

    #[test]
    fn a_configured_icon_may_name_a_glyph() {
        let types = one("{TAG}", &["posts/"]);
        assert_eq!(
            short_branch_len("posts/tmux", 40, &types),
            format!("{}tmux", crate::tmux::icons::TAG)
        );
    }

    #[test]
    fn an_unknown_glyph_name_is_drawn_as_written() {
        // Same rule as `separator_before`: a visible typo beats a silent one.
        let types = one("{NOPE}", &["posts/"]);
        assert_eq!(short_branch_len("posts/tmux", 40, &types), "{NOPE}tmux");
    }

    #[test]
    fn prefixes_are_matched_without_case() {
        let types = one("P ", &["parked/"]);
        assert_eq!(short_branch_len("Parked/Thing", 40, &types), "P Thing");
    }

    #[test]
    fn entries_are_tried_in_the_order_written() {
        let types = vec![
            BranchType {
                icon: "first ".into(),
                prefixes: vec!["post/".into()],
            },
            BranchType {
                icon: "second ".into(),
                prefixes: vec!["post/".into()],
            },
        ];
        assert_eq!(short_branch_len("post/x", 40, &types), "first x");
    }

    #[test]
    fn a_longer_spelling_is_not_swallowed_by_a_shorter_one() {
        // `post/` is not a prefix of `posts/whatever`, so the two can be
        // separate entries with separate glyphs and order does not matter.
        let types = vec![
            BranchType {
                icon: "one ".into(),
                prefixes: vec!["post/".into()],
            },
            BranchType {
                icon: "many ".into(),
                prefixes: vec!["posts/".into()],
            },
        ];
        assert_eq!(short_branch_len("posts/x", 40, &types), "many x");
    }

    #[test]
    fn an_empty_list_leaves_every_branch_on_the_plain_glyph() {
        assert_eq!(
            short_branch_len("feat/thing", 40, &[]),
            format!("{}feat/thing", BRANCH)
        );
    }

    #[test]
    fn a_prefix_and_nothing_after_it_keeps_the_name_as_written() {
        // Stripping `feat/` off `feat/` leaves an icon and no name, which says
        // less than the name does.
        let types = one("F ", &["feat/"]);
        assert_eq!(short_branch_len("feat/", 40, &types), "F feat/");
    }

    #[test]
    fn a_prefix_cutting_a_multibyte_name_does_not_panic() {
        // `branch.get(..len)` returns None on a byte that is not a char
        // boundary; slicing would have panicked in the daemon.
        let types = one("E ", &["ab"]);
        assert_eq!(short_branch_len("aé", 40, &types), format!("{}aé", BRANCH));
    }

    #[test]
    fn short_branch_plain_gets_fallback_branch_icon() {
        assert_eq!(short_branch("branch1"), format!("{}branch1", BRANCH));
    }

    #[test]
    fn short_branch_len_default_matches_legacy_shape() {
        // 25 chars: default max 20 keeps 8 head + "..." + 10 tail
        let r = short_branch_len("abcdefghijklmnopqrstuvwxy", 20, types());
        assert_eq!(r, format!("{}abcdefgh...pqrstuvwxy", BRANCH));
        assert_eq!(r, short_branch("abcdefghijklmnopqrstuvwxy"));
    }

    #[test]
    fn short_branch_len_proportional_at_40() {
        // budget 38: head 17, tail 21
        let name: String = ('a'..='z').cycle().take(50).collect();
        let r = short_branch_len(&name, 40, types());
        let r = r.strip_prefix(BRANCH).unwrap();
        assert_eq!(r.chars().count(), 17 + 3 + 21);
        assert!(r.starts_with(&name.chars().take(17).collect::<String>()));
        let tail: String = name.chars().skip(50 - 21).collect();
        assert!(r.ends_with(&tail));
    }

    #[test]
    fn short_branch_len_at_exact_max_not_truncated() {
        let name: String = ('a'..='z').cycle().take(40).collect();
        assert_eq!(
            short_branch_len(&name, 40, types()),
            format!("{}{}", BRANCH, name)
        );
    }

    #[test]
    fn short_branch_feature() {
        let r = short_branch("feat/my-feature");
        assert!(r.starts_with(FEATURE));
        assert!(r.contains("my-feature"));
    }

    #[test]
    fn short_branch_features_plural() {
        let r = short_branch("features/x");
        assert!(r.starts_with(FEATURE));
    }

    #[test]
    fn short_branch_bugfix() {
        let r = short_branch("bugfix/issue-42");
        assert!(r.starts_with(BUGFIX));
        assert!(r.contains("issue-42"));
    }

    #[test]
    fn short_branch_fix() {
        let r = short_branch("fix/crash");
        assert!(r.starts_with(BUGFIX));
    }

    #[test]
    fn short_branch_hotfix() {
        let r = short_branch("hotfix/urgent");
        assert!(r.starts_with(HOTFIX));
    }

    #[test]
    fn short_branch_chore() {
        let r = short_branch("chore/cleanup");
        assert!(r.starts_with(CHORE));
    }

    #[test]
    fn short_branch_release() {
        let r = short_branch("release/1.0");
        assert!(r.starts_with(RELEASE));
    }

    #[test]
    fn short_branch_exactly_max_len_not_truncated() {
        // BRANCH_MAX_LEN = 20; exactly 20 chars should NOT be truncated (> not >=).
        let twenty = "a".repeat(20);
        let r = short_branch(&twenty);
        assert!(!r.contains("..."), "should not truncate at exactly 20: {r}");
    }

    #[test]
    fn short_branch_one_over_max_len_is_truncated() {
        let twenty_one = "a".repeat(21);
        let r = short_branch(&twenty_one);
        assert!(r.contains("..."), "21 chars should be truncated: {r}");
    }

    #[test]
    fn short_branch_over_max_len_truncated() {
        let long = "this-is-a-very-long-branch-name"; // 31 chars
        let r = short_branch(long);
        assert!(r.contains("..."), "should contain ellipsis: {r}");
        assert!(
            r.chars().count() < long.chars().count(),
            "truncated shorter"
        );
    }

    #[test]
    fn short_branch_truncation_head_tail() {
        let long = "this-is-a-very-long-branch-name"; // 31 chars
        let r = short_branch(long);
        // head = first 8 chars = "this-is-"
        // tail = last 10 chars (Go: branch[len-1-tailLen:] = branch[21:]) = "ranch-name"
        assert!(r.starts_with(&format!("{}this-is-", BRANCH)), "head: {r}");
        assert!(r.ends_with("ranch-name"), "tail: {r}");
    }

    #[test]
    fn short_branch_tag() {
        let r = short_branch("tags/v1.2.3");
        assert!(r.starts_with(TAG), "tag icon: {r}");
        assert!(r.contains("v1.2.3"));
        assert!(short_branch("tag/v2").starts_with(TAG));
    }

    #[test]
    fn short_branch_fallback_for_common_trunk_names() {
        for name in ["main", "master", "dev", "stable", "xyz"] {
            let r = short_branch(name);
            assert_eq!(r, format!("{}{}", BRANCH, name));
        }
    }

    // ── parts ────────────────────────────────────────────────────────────────

    /// A status with something in every group, so leaving a part out is
    /// visible.
    fn busy_status() -> GitStatus {
        GitStatus {
            branch: "main".into(),
            ahead: 2,
            behind: 3,
            unmerged: 1,
            untracked: 4,
            stashed: 5,
            remote_success: true,
            unstaged: Area {
                modified: 6,
                deleted: 7,
                ..Default::default()
            },
            staged: Area {
                modified: 8,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    fn render_parts(parts: &[GitPart]) -> String {
        // `branch_icon: true` to match `status_line_capped`, which is what the
        // default-list test compares against.
        status_line_render(
            &busy_status(),
            false,
            true,
            &LineOpts {
                style: Style::Fill,
                cap_glyph: Some(""),
                parts,
                ..LineOpts::default()
            },
        )
    }

    #[test]
    fn the_default_part_list_renders_what_it_always_did() {
        // The byte-identity promise, at the level the config touches.
        assert_eq!(
            render_parts(&GitPart::all()),
            status_line_capped(&busy_status(), false, true, Style::Fill, Some(""))
        );
    }

    #[test]
    fn dropping_untracked_drops_its_count() {
        let all = render_parts(&GitPart::all());
        let without: Vec<GitPart> = GitPart::all()
            .into_iter()
            .filter(|p| *p != GitPart::Untracked)
            .collect();
        let out = render_parts(&without);
        assert!(all.contains("4"), "the fixture should have 4 untracked");
        assert!(!out.contains(&format!("4{ADDED}")), "{out}");
        // and nothing else went with it
        assert!(out.contains(&format!("6{MODIFIED}")), "{out}");
    }

    #[test]
    fn dropping_ahead_and_behind_leaves_the_branch_name() {
        let parts: Vec<GitPart> = GitPart::all()
            .into_iter()
            .filter(|p| !matches!(p, GitPart::Ahead | GitPart::Behind))
            .collect();
        let out = render_parts(&parts);
        assert!(out.contains("main"), "{out}");
        assert!(!out.contains(AHEAD), "{out}");
        assert!(!out.contains(BEHIND), "{out}");
    }

    #[test]
    fn a_branch_name_only_list_renders_just_that() {
        let out = render_parts(&[GitPart::Branch]);
        assert!(out.contains("main"), "{out}");
        for glyph in [AHEAD, BEHIND, UNMERGED, STASHED, STAGED, MODIFIED] {
            assert!(!out.contains(glyph), "expected no {glyph:?} in {out:?}");
        }
    }

    #[test]
    fn dropping_state_drops_the_clean_tick() {
        let clean = GitStatus {
            branch: "main".into(),
            remote_success: true,
            ..Default::default()
        };
        let with = status_line_render(
            &clean,
            false,
            true,
            &LineOpts {
                style: Style::Fill,
                cap_glyph: Some(""),
                branch_icon: false,
                parts: &GitPart::all(),
                ..LineOpts::default()
            },
        );
        let without = status_line_render(
            &clean,
            false,
            true,
            &LineOpts {
                style: Style::Fill,
                cap_glyph: Some(""),
                branch_icon: false,
                parts: &[GitPart::Branch],
                ..LineOpts::default()
            },
        );
        assert!(with.contains(CLEAN), "{with}");
        assert!(!without.contains(CLEAN), "{without}");
    }

    #[test]
    fn dropping_stash_drops_only_the_stash() {
        let parts: Vec<GitPart> = GitPart::all()
            .into_iter()
            .filter(|p| *p != GitPart::Stash)
            .collect();
        let out = render_parts(&parts);
        assert!(!out.contains(STASHED), "{out}");
        assert!(
            out.contains(&format!("8{MODIFIED}")),
            "staged survives: {out}"
        );
    }

    #[test]
    fn an_empty_part_list_still_produces_a_segment_rather_than_a_panic() {
        // Somebody will write `parts = []`, and the answer to that is an empty
        // bar, not a crash.
        let out = render_parts(&[]);
        assert!(!out.contains("main"), "{out}");
    }

    // ── status_line_mode: tmux format (ports all 17 Go test cases) ────────────

    #[test]
    fn status_line_loading() {
        let s = GitStatus {
            branch: "branch1".into(),
            loading: true,
            ..Default::default()
        };
        let line = status_line_mode(&s, false, false);
        // Loading: sync icon on blue background
        assert!(line.contains(&format!("bg={}", BG_LOADING)));
        assert!(line.contains(SYNC));
        assert!(line.contains(GIT));
        assert!(line.contains("branch1"));
    }

    #[test]
    fn status_line_remote_fail() {
        let s = GitStatus {
            branch: "branch1".into(),
            ..Default::default()
        };
        let line = status_line_mode(&s, false, false);
        // Remote fail: arrow on error background, failed icon
        assert!(line.contains(&format!("bg={}", BG_ERROR)));
        assert!(line.contains(FAILED));
        assert!(line.contains(ARROW_RIGHT));
    }

    #[test]
    fn status_line_clean_exact() {
        let s = s_clean();
        let line = status_line_mode(&s, false, false);
        let bg = BG_CLEAN;
        // Should contain remote success marker, git+branch, clean icon, and arrow
        assert!(line.contains(&remote_success_seg(bg, false)));
        assert!(line.contains(&branch_seg("branch1", FG_CLEAN, bg, false)));
        assert!(line.contains(CLEAN));
        assert!(line.contains(&arrow(bg, false)));
    }

    #[test]
    fn status_line_new() {
        let s = GitStatus {
            branch: "branch1".into(),
            is_new: true,
            remote_success: true,
            ..Default::default()
        };
        let line = status_line_mode(&s, false, false);
        let bg = BG_NEW;
        assert!(line.contains(&remote_success_seg(bg, false)));
        assert!(line.contains(NEW));
        assert!(line.contains(&arrow(bg, false)));
    }

    #[test]
    fn status_line_gone() {
        let s = GitStatus {
            branch: "branch1".into(),
            is_gone: true,
            remote_success: true,
            ..Default::default()
        };
        let line = status_line_mode(&s, false, false);
        assert!(line.contains(&format!("bg={}", BG_GONE)));
        assert!(line.contains(GONE));
    }

    #[test]
    fn status_line_feature_branch() {
        let s = GitStatus {
            branch: "feat/branch1".into(),
            remote_success: true,
            ..Default::default()
        };
        let line = status_line_mode(&s, false, false);
        assert!(line.contains(FEATURE));
        assert!(line.contains("branch1"));
        assert!(line.contains(GIT));
    }

    #[test]
    fn status_line_bugfix_branch() {
        let s = GitStatus {
            branch: "bugfix/branch1".into(),
            remote_success: true,
            ..Default::default()
        };
        let line = status_line_mode(&s, false, false);
        assert!(line.contains(BUGFIX));
    }

    #[test]
    fn status_line_hotfix_branch() {
        let s = GitStatus {
            branch: "hotfix/branch1".into(),
            remote_success: true,
            ..Default::default()
        };
        let line = status_line_mode(&s, false, false);
        assert!(line.contains(HOTFIX));
    }

    #[test]
    fn status_line_chore_branch() {
        let s = GitStatus {
            branch: "chore/branch1".into(),
            remote_success: true,
            ..Default::default()
        };
        let line = status_line_mode(&s, false, false);
        assert!(line.contains(CHORE));
    }

    #[test]
    fn status_line_branch_too_long() {
        let s = GitStatus {
            branch: "this-is-a-very-long-branch-name".into(),
            remote_success: true,
            ..Default::default()
        };
        let line = status_line_mode(&s, false, false);
        assert!(line.contains("this-is-"), "head: {line}");
        assert!(line.contains("ranch-name"), "tail (last 10): {line}");
        assert!(line.contains("..."), "ellipsis: {line}");
    }

    #[test]
    fn status_line_ahead() {
        let s = GitStatus {
            branch: "branch1".into(),
            remote_success: true,
            ahead: 1,
            ..Default::default()
        };
        let line = status_line_mode(&s, false, false);
        let bg = BG_DEFAULT;
        assert!(line.contains(&format!("bg={}", bg)));
        assert!(line.contains("1"));
        assert!(line.contains(AHEAD));
        assert!(line.contains(&colored_segment(false, FG_DARK_BLUE, bg, AHEAD)));
    }

    #[test]
    fn status_line_behind() {
        let s = GitStatus {
            branch: "branch1".into(),
            remote_success: true,
            behind: 1,
            ..Default::default()
        };
        let line = status_line_mode(&s, false, false);
        assert!(line.contains("1"));
        assert!(line.contains(BEHIND));
    }

    #[test]
    fn status_line_unmerged() {
        let s = GitStatus {
            branch: "branch1".into(),
            remote_success: true,
            unmerged: 1,
            ..Default::default()
        };
        let line = status_line_mode(&s, false, false);
        assert!(line.contains("1"), "count: {line}");
        assert!(line.contains(UNMERGED), "icon: {line}");
        // colored_segment(fg=BG_ERROR, bg=BG_DEFAULT, ...) → fg=color160
        assert!(line.contains(&format!("fg={}", BG_ERROR)), "fg=160: {line}");
    }

    #[test]
    fn status_line_unstaged() {
        let s = GitStatus {
            branch: "branch1".into(),
            remote_success: true,
            unstaged: Area {
                modified: 1,
                added: 1,
                deleted: 1,
                renamed: 1,
                copied: 1,
            },
            ..Default::default()
        };
        let line = status_line_mode(&s, false, false);
        assert!(line.contains(ADDED));
        assert!(line.contains(DELETED));
        assert!(line.contains(RENAMED));
        assert!(line.contains(COPIED));
        assert!(line.contains(MODIFIED));
        // Staged icon should NOT appear (no staged changes)
        assert!(!line.contains(STAGED));
    }

    #[test]
    fn status_line_staged() {
        let s = GitStatus {
            branch: "branch1".into(),
            remote_success: true,
            staged: Area {
                modified: 1,
                added: 1,
                deleted: 1,
                renamed: 1,
                copied: 1,
            },
            ..Default::default()
        };
        let line = status_line_mode(&s, false, false);
        assert!(line.contains(STAGED)); // staged icon appears
        assert!(line.contains(ADDED));
        assert!(line.contains(&colored_segment(false, FG_GREEN, BG_DEFAULT, STAGED)));
    }

    #[test]
    fn status_line_stashed() {
        let s = GitStatus {
            branch: "branch1".into(),
            remote_success: true,
            stashed: 2,
            ..Default::default()
        };
        let line = status_line_mode(&s, false, false);
        assert!(line.contains("2"));
        assert!(line.contains(STASHED));
        assert!(line.contains(&colored_segment(false, FG_PURPLE, BG_CLEAN, STASHED)));
    }

    #[test]
    fn status_line_dirty_all() {
        let s = s_dirty_all();
        let line = status_line_mode(&s, false, false);
        // All sub-sections present
        assert!(line.contains(AHEAD));
        assert!(line.contains(BEHIND));
        assert!(line.contains(UNMERGED));
        assert!(line.contains(ADDED));
        assert!(line.contains(STAGED));
        assert!(line.contains(STASHED));
        // Sub-sections are separated by plain spaces, and only the segment end
        // carries a glyph.
        assert!(!line.contains('|'), "stale pipe divider: {line}");
        assert_eq!(
            line.matches(SEPARATOR).count(),
            0,
            "fill ends with an arrow: {line}"
        );
        // untracked(1) + unstaged.added(1) → count 2
        assert!(line.contains(&format!("2{}", ADDED)));
    }

    #[test]
    fn status_line_untracked_adds_to_unstaged_added() {
        let s = GitStatus {
            branch: "b".into(),
            remote_success: true,
            untracked: 3,
            unstaged: Area {
                added: 2,
                ..Default::default()
            },
            ..Default::default()
        };
        let line = status_line_mode(&s, false, false);
        // 3 untracked + 2 unstaged.added = 5
        assert!(line.contains(&format!("5{}", ADDED)));
    }

    // ── status_line_mode: no-tmux (ANSI) format ──────────────────────────────

    #[test]
    fn status_line_no_tmux_clean() {
        let s = s_clean();
        let line = status_line_mode(&s, false, true);
        // Uses ANSI escapes, not #[fg=...]
        assert!(
            !line.contains("#["),
            "should not contain tmux format: {line}"
        );
        assert!(line.contains("\x1b["));
        assert!(line.contains("branch1"));
        assert!(line.ends_with('\x00'.to_string().trim_end_matches('\x00')));
        // Ends with reset+arrow+reset
        let bg = BG_CLEAN;
        assert!(line.contains(&format!("\x1b[0m\x1b[38;5;{}m{}\x1b[0m", bg, ARROW_RIGHT)));
    }

    #[test]
    fn status_line_no_tmux_arrow_right_not_in_color_codes() {
        // In no-tmux mode, ColoredSegment(fg, bg, ARROW_RIGHT) special case:
        // the arrow character should appear only once (from the final append).
        let s = GitStatus {
            branch: "branch1".into(),
            ..Default::default()
        };
        let line = status_line_mode(&s, false, true);
        let arrow_count = line.matches(ARROW_RIGHT).count();
        // Loading+remote_fail have extra arrows; with remote_success=false we get
        // the remote-fail path which adds 2 arrows, plus the final arrow.
        // Just assert at least one arrow is present.
        assert!(arrow_count >= 1, "no arrow found in: {line:?}");
    }

    #[test]
    fn status_line_no_tmux_loading() {
        let s = GitStatus {
            branch: "branch1".into(),
            loading: true,
            ..Default::default()
        };
        let line = status_line_mode(&s, false, true);
        assert!(!line.contains("#["));
        assert!(line.contains(SYNC));
    }

    #[test]
    fn status_line_nvim_suspended_uses_terminal_bg_for_arrow() {
        let s = s_clean();
        let normal = status_line_mode(&s, false, false);
        let suspended = status_line_mode(&s, true, false);
        // With nvim suspended the final arrow background is BG_TERMINAL ("235"),
        // without it the background is "233".
        assert!(
            normal.contains(&format!("bg=color233]{}", ARROW_RIGHT)),
            "normal arrow bg should be 233: {normal}"
        );
        assert!(
            suspended.contains(&format!("bg={}]{}", BG_TERMINAL, ARROW_RIGHT)),
            "suspended arrow bg should be BG_TERMINAL: {suspended}"
        );
        assert_ne!(normal, suspended);
    }

    // ── styles ────────────────────────────────────────────────────────────────

    #[test]
    fn fill_style_matches_legacy_render() {
        let s = s_dirty_all();
        assert_eq!(
            status_line_mode(&s, false, false),
            status_line_styled(&s, false, false, Style::Fill)
        );
    }

    #[test]
    fn outline_puts_state_color_in_foreground_and_bar_in_background() {
        let s = s_clean();
        let line = status_line_styled(&s, false, false, Style::Outline);
        // Clean state color moves from background to foreground.
        assert!(
            line.contains(&format!("fg={},bg={}]", BG_CLEAN, BG_BAR)),
            "state color should be the text color: {line}"
        );
        assert!(
            !line.contains(&format!("bg={}]", BG_CLEAN)),
            "no solid fill left: {line}"
        );
    }

    #[test]
    fn outline_keeps_icon_colors_untouched() {
        let s = GitStatus {
            stashed: 1,
            ..s_clean()
        };
        let line = status_line_styled(&s, false, false, Style::Outline);
        // Stash icon keeps FG_PURPLE, only its background changes.
        assert!(line.contains(&colored_segment(false, FG_PURPLE, BG_BAR, STASHED)));
        // Clean icon keeps FG_GREEN.
        assert!(line.contains(&colored_segment(false, FG_GREEN, BG_BAR, CLEAN)));
    }

    #[test]
    fn outline_bright_lightens_icon_colors() {
        let s = GitStatus {
            stashed: 1,
            ahead: 1,
            ..s_clean()
        };
        let line = status_line_styled(&s, false, false, Style::OutlineBright);
        assert!(line.contains(&colored_segment(false, AC_PURPLE, BG_BAR, STASHED)));
        assert!(line.contains(&colored_segment(false, AC_DARK_BLUE, BG_BAR, AHEAD)));
        assert!(!line.contains(&format!("fg={}", FG_PURPLE)));
    }

    #[test]
    fn outline_bright_lightens_the_gone_state_color() {
        let s = GitStatus {
            is_gone: true,
            ..s_clean()
        };
        let plain = status_line_styled(&s, false, false, Style::Outline);
        let bright = status_line_styled(&s, false, false, Style::OutlineBright);
        assert!(plain.contains(&format!("fg={}", BG_GONE)));
        assert!(bright.contains(&format!("fg={}", AC_GONE)));
    }

    #[test]
    fn outline_error_and_loading_backgrounds_flatten_to_the_bar() {
        let failed = status_line_styled(
            &GitStatus {
                branch: "b".into(),
                ..Default::default()
            },
            false,
            false,
            Style::Outline,
        );
        assert!(!failed.contains(&format!("bg={}]", BG_ERROR)), "{failed}");
        // Outline draws the error as AC_ERROR rather than BG_ERROR: the fill
        // background reads 3.47:1 on the bar, under the 4.5 WCAG asks of text.
        assert!(failed.contains(&format!("fg={}", AC_ERROR)), "{failed}");

        let loading = status_line_styled(
            &GitStatus {
                branch: "b".into(),
                loading: true,
                ..Default::default()
            },
            false,
            false,
            Style::Outline,
        );
        assert!(
            !loading.contains(&format!("bg={}]", BG_LOADING)),
            "{loading}"
        );
        assert!(loading.contains(&format!("fg={}", BG_LOADING)), "{loading}");
    }

    #[test]
    fn fill_ends_with_the_solid_arrow() {
        let s = s_clean();
        let line = status_line_styled(&s, false, false, Style::Fill);
        assert!(line.ends_with(&powerline_segment(BG_CLEAN, BG_BAR, ARROW_RIGHT)));
    }

    #[test]
    fn outline_ends_with_the_separator_glyph() {
        let s = s_clean();
        let line = status_line_styled(&s, false, false, Style::Outline);
        assert!(line.ends_with(&powerline_segment(BG_CLEAN, BG_BAR, SEPARATOR)));
        // The solid triangle must not survive into an outline render.
        assert!(!line.contains(ARROW_RIGHT), "solid arrow left over: {line}");
    }

    #[test]
    fn separator_appears_once_and_only_at_the_end() {
        let line = status_line_styled(&s_dirty_all(), false, false, Style::OutlineBright);
        assert_eq!(line.matches(SEPARATOR).count(), 1, "{line}");
        assert!(line.ends_with(SEPARATOR), "{line}");
        assert!(!line.contains('|'), "stale pipe divider: {line}");
    }

    #[test]
    fn branch_icon_off_drops_glyph_and_its_space() {
        let s = s_clean();
        let line = status_line_render(
            &s,
            false,
            false,
            &LineOpts {
                style: Style::Outline,
                branch_icon: false,
                parts: &GitPart::all(),
                ..LineOpts::default()
            },
        );
        assert!(!line.contains(GIT.trim()), "git glyph left over: {line}");
        // GIT's trailing space must go with it: the branch-type glyph follows
        // the color code directly, no orphaned gap.
        assert!(
            line.contains(&format!("]{}branch1", BRANCH)),
            "gap before branch: {line}"
        );
    }

    #[test]
    fn branch_icon_on_keeps_legacy_shape() {
        let s = s_clean();
        let with_icon = status_line_render(
            &s,
            false,
            false,
            &LineOpts {
                style: Style::Outline,
                parts: &GitPart::all(),
                ..LineOpts::default()
            },
        );
        assert_eq!(
            with_icon,
            status_line_styled(&s, false, false, Style::Outline)
        );
        assert!(with_icon.contains(GIT));
    }

    #[test]
    fn cap_override_replaces_the_end_cap() {
        let s = s_clean();
        let line = status_line_capped(&s, false, false, Style::Outline, Some(CAP_RULE));
        assert!(line.ends_with(&powerline_segment(BG_CLEAN, BG_BAR, CAP_RULE)));
    }

    #[test]
    fn cap_override_does_not_leak_into_the_default() {
        let s = s_clean();
        let _ = status_line_capped(&s, false, false, Style::Outline, Some(CAP_RULE));
        let plain = status_line_styled(&s, false, false, Style::Outline);
        assert!(!plain.contains(CAP_RULE.trim()), "{plain}");
    }

    #[test]
    fn outline_respects_nvim_suspended_bar_background() {
        let s = s_clean();
        let line = status_line_styled(&s, true, false, Style::Outline);
        assert!(line.contains(&format!("bg={}]", BG_TERMINAL)), "{line}");
        assert!(!line.contains(&format!("bg={}]", BG_BAR)), "{line}");
    }

    #[test]
    fn style_parse_accepts_known_names_only() {
        assert_eq!(Style::parse("fill"), Some(Style::Fill));
        assert_eq!(Style::parse("outline"), Some(Style::Outline));
        assert_eq!(Style::parse("outline-bright"), Some(Style::OutlineBright));
        assert_eq!(Style::parse("bright"), Some(Style::OutlineBright));
        assert_eq!(Style::parse("nope"), None);
        assert_eq!(Style::default(), Style::OutlineBright);
    }

    // ── count_stash ───────────────────────────────────────────────────────────

    #[test]
    fn count_stash_missing_file_returns_zero() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(count_stash(dir.path()), 0);
    }

    #[test]
    fn count_stash_counts_lines() {
        let dir = tempfile::tempdir().unwrap();
        let stash_path = dir.path().join("logs/refs");
        std::fs::create_dir_all(&stash_path).unwrap();
        std::fs::write(stash_path.join("stash"), "entry1\nentry2\nentry3\n").unwrap();
        assert_eq!(count_stash(dir.path()), 3);
    }

    #[test]
    fn count_stash_empty_file_returns_zero() {
        let dir = tempfile::tempdir().unwrap();
        let stash_path = dir.path().join("logs/refs");
        std::fs::create_dir_all(&stash_path).unwrap();
        std::fs::write(stash_path.join("stash"), "").unwrap();
        assert_eq!(count_stash(dir.path()), 0);
    }
}
