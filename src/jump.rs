//! Jumping to any text on the screen: type a few of its characters, then the
//! label that appears beside the one you meant.
//!
//! This is the motion `folke/flash.nvim` gives neovim, over every pane in the
//! window. `schasse/tmux-jump` did the one-character version of it and was the
//! last plugin in the config: it asked for a single character through
//! `command-prompt -1`, labelled every occurrence on the pane, and moved copy
//! mode there with one `cursor-right` per character on the screen, which on
//! tmux 3.5 and later overshoots by one column per line above the target and
//! had to be patched locally to land at all.
//!
//! What is here instead is a borderless popup laid exactly over the window.
//! It draws what every visible pane shows, greyed, and as characters are typed
//! the matches light up and each gets a label. The labels are picked so that
//! none of them is the character after any match: typing on extends the
//! search, typing a label jumps, and the two can never be confused. A match
//! keeps its label while the search narrows, so the letter you were reaching
//! for does not move. Enter takes the nearest match, backspace on an empty
//! search or escape closes it.
//!
//! The jump selects the pane, puts it in copy mode and moves the cursor to the
//! match by row and column, so a selection already started extends to it, as
//! a flash jump does in visual mode.
//!
//! Like `panes` and `search`, this runs in the **client**: it needs a
//! terminal, and nothing in it is worth a daemon caching.

use std::collections::{HashMap, HashSet};

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use unicode_width::UnicodeWidthChar;

use crate::picker::{Paint, Tone};

/// The labels, in the order they are handed out: home row first.
pub const LABELS: &str = "asdfghjklqwertyuiopzxcvbnm";

/// What `list-panes` is asked for, one pane per line.
pub const LISTING: &str = "#{pane_id}\t#{pane_left}\t#{pane_top}\t#{pane_width}\t#{pane_height}\t#{pane_active}\t#{pane_in_mode}\t#{scroll_position}\t#{cursor_x}\t#{cursor_y}\t#{copy_cursor_x}\t#{copy_cursor_y}\t#{window_zoomed_flag}";

/// One pane of the window, where it sits and what its view is scrolled to.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Pane {
    /// `%N`.
    pub id: String,
    /// Column of the window the pane starts at.
    pub left: u16,
    /// Row of the window the pane starts at.
    pub top: u16,
    /// Columns.
    pub width: u16,
    /// Rows.
    pub height: u16,
    /// The pane the binding was pressed in.
    pub active: bool,
    /// In copy mode, or another mode with a view of its own.
    pub in_mode: bool,
    /// How far the view is scrolled back into the history.
    pub scroll: u32,
    /// Where the cursor is, in the pane: the copy cursor when in a mode.
    pub cursor: (u16, u16),
}

/// Every pane `list-panes` printed, less the ones a zoom hides.
pub fn parse_panes(listing: &str) -> Vec<Pane> {
    let mut zoomed = false;
    let mut panes: Vec<Pane> = listing
        .lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.split('\t').collect();
            if f.len() < 13 {
                return None;
            }
            let n = |s: &str| s.parse::<u16>().unwrap_or(0);
            let in_mode = f[6] == "1";
            zoomed |= f[12] == "1";
            let cursor = if in_mode {
                (n(f[10]), n(f[11]))
            } else {
                (n(f[8]), n(f[9]))
            };
            Some(Pane {
                id: f[0].to_string(),
                left: n(f[1]),
                top: n(f[2]),
                width: n(f[3]),
                height: n(f[4]),
                active: f[5] == "1",
                in_mode,
                scroll: f[7].parse().unwrap_or(0),
                cursor,
            })
        })
        .collect();
    if zoomed {
        panes.retain(|p| p.active);
    }
    panes
}

/// The `capture-pane` arguments that read what a pane shows right now,
/// scrolled back as far as its view is.
pub fn capture_args(pane: &Pane) -> Vec<String> {
    let mut args = vec![
        "capture-pane".to_string(),
        "-p".to_string(),
        "-t".to_string(),
        pane.id.clone(),
    ];
    if pane.scroll > 0 {
        let start = -i64::from(pane.scroll);
        let end = i64::from(pane.height) - 1 + start;
        args.extend([
            "-S".to_string(),
            start.to_string(),
            "-E".to_string(),
            end.to_string(),
        ]);
    }
    args
}

/// A pane and the lines it shows, each split into characters.
#[derive(Debug, Clone, Default)]
pub struct Screen {
    /// Where it is.
    pub pane: Pane,
    /// What it shows, one entry per row.
    pub lines: Vec<Vec<char>>,
}

impl Screen {
    /// A pane and its capture, cut to the pane's height.
    pub fn new(pane: Pane, capture: &str) -> Self {
        let lines = capture
            .lines()
            .take(usize::from(pane.height))
            .map(|l| l.chars().collect())
            .collect();
        Self { pane, lines }
    }
}

/// One place the search matched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hit {
    /// Which screen.
    pub screen: usize,
    /// Row in the pane.
    pub row: u16,
    /// Characters before it on the row: what `cursor-right` counts.
    pub col: usize,
    /// Characters it covers.
    pub len: usize,
    /// The character after it, which typing on would add to the search.
    pub next: Option<char>,
}

impl Hit {
    /// What a hit is known by from one keystroke to the next.
    fn key(&self) -> (usize, u16, usize) {
        (self.screen, self.row, self.col)
    }
}

/// Whether a search ignores case: it does unless it holds a capital, which is
/// vim's `smartcase` and what flash does under it.
pub fn folds(pattern: &str) -> bool {
    !pattern.chars().any(char::is_uppercase)
}

fn lower(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

/// Every place `pattern` appears, nearest to the cursor first.
///
/// Matches on a row do not overlap: in `aaaa` the search `aa` finds two, not
/// three, so no label lands on another match.
pub fn hits(screens: &[Screen], pattern: &str) -> Vec<Hit> {
    let pat: Vec<char> = pattern.chars().collect();
    if pat.is_empty() {
        return Vec::new();
    }
    let fold = folds(pattern);
    let same = |a: char, b: char| if fold { lower(a) == lower(b) } else { a == b };
    let mut out = Vec::new();
    for (s, screen) in screens.iter().enumerate() {
        for (row, line) in screen.lines.iter().enumerate() {
            let mut col = 0;
            while col + pat.len() <= line.len() {
                if pat.iter().zip(&line[col..]).all(|(&p, &c)| same(p, c)) {
                    out.push(Hit {
                        screen: s,
                        row: u16::try_from(row).unwrap_or(u16::MAX),
                        col,
                        len: pat.len(),
                        next: line.get(col + pat.len()).copied(),
                    });
                    col += pat.len();
                } else {
                    col += 1;
                }
            }
        }
    }
    let origin = screens
        .iter()
        .find(|s| s.pane.active)
        .map(|s| {
            (
                i32::from(s.pane.left) + i32::from(s.pane.cursor.0),
                i32::from(s.pane.top) + i32::from(s.pane.cursor.1),
            )
        })
        .unwrap_or((0, 0));
    out.sort_by_key(|h| {
        let pane = &screens[h.screen].pane;
        let x = i32::from(pane.left)
            + cells_before(&screens[h.screen].lines[usize::from(h.row)], h.col);
        let y = i32::from(pane.top) + i32::from(h.row);
        (!pane.active, (y - origin.1).abs(), (x - origin.0).abs())
    });
    out
}

/// Terminal cells taken by the first `col` characters of a line.
fn cells_before(line: &[char], col: usize) -> i32 {
    line.iter()
        .take(col)
        .map(|c| c.width().unwrap_or(0))
        .sum::<usize>()
        .try_into()
        .unwrap_or(i32::MAX)
}

/// A label for each hit, by the same position, or none when they ran out.
///
/// A letter that is the next character of any match is held back, because
/// typing it has to extend the search. A hit that had a label on the last
/// keystroke keeps it if it still can, so narrowing the search never moves
/// the letter somebody is already reaching for.
pub fn labels(
    hits: &[Hit],
    fold: bool,
    previous: &HashMap<(usize, u16, usize), char>,
) -> Vec<Option<char>> {
    let held: HashSet<char> = hits
        .iter()
        .filter_map(|h| h.next)
        .map(|c| if fold { lower(c) } else { c })
        .collect();
    let mut free: Vec<char> = LABELS.chars().filter(|c| !held.contains(c)).collect();
    let mut out = vec![None; hits.len()];
    for (i, hit) in hits.iter().enumerate() {
        let kept = previous
            .get(&hit.key())
            .and_then(|&c| free.iter().position(|&f| f == c));
        if let Some(at) = kept {
            out[i] = Some(free.remove(at));
        }
    }
    let mut free = free.into_iter();
    for slot in out.iter_mut().filter(|l| l.is_none()) {
        match free.next() {
            Some(c) => *slot = Some(c),
            None => break,
        }
    }
    out
}

/// The one tmux command line that selects the pane and moves its copy cursor
/// to a row and column of its view, `;`-separated so it runs as one client.
///
/// Row and column rather than a count of steps from the top: `cursor-right`
/// at the end of a line wraps to the next one on tmux 3.5 and later, so a
/// count that crosses lines lands one column further per line it crossed.
pub fn land_command(id: &str, in_mode: bool, row: u16, col: usize) -> Vec<String> {
    let mut out: Vec<String> = vec!["select-pane".into(), "-t".into(), id.into()];
    let mut then = |args: &[&str]| {
        out.push(";".into());
        out.extend(args.iter().map(|s| s.to_string()));
    };
    if !in_mode {
        then(&["copy-mode", "-t", id]);
    }
    then(&["send-keys", "-X", "-t", id, "top-line"]);
    if row > 0 {
        then(&[
            "send-keys",
            "-X",
            "-N",
            &row.to_string(),
            "-t",
            id,
            "cursor-down",
        ]);
    }
    then(&["send-keys", "-X", "-t", id, "start-of-line"]);
    if col > 0 {
        then(&[
            "send-keys",
            "-X",
            "-N",
            &col.to_string(),
            "-t",
            id,
            "cursor-right",
        ]);
    }
    out
}

/// What the person did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Closed it without going anywhere.
    Cancelled,
    /// Picked this hit.
    Land(Hit),
}

/// The search as typed so far, the hits it has, and their labels.
#[derive(Debug, Default)]
pub struct State {
    /// What has been typed.
    pub pattern: String,
    /// Where it matches, nearest first.
    pub hits: Vec<Hit>,
    /// A label per hit.
    pub labels: Vec<Option<char>>,
}

/// What a key does to the search.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    /// A character.
    Char(char),
    /// Backspace.
    Back,
    /// Enter.
    Enter,
    /// Escape or ctrl-c.
    Cancel,
}

impl State {
    /// One key, and the outcome if it ended the jump.
    pub fn press(&mut self, screens: &[Screen], key: Key) -> Option<Outcome> {
        match key {
            Key::Cancel => return Some(Outcome::Cancelled),
            Key::Enter => {
                return Some(match self.hits.first() {
                    Some(&h) => Outcome::Land(h),
                    None => Outcome::Cancelled,
                });
            }
            Key::Back => {
                if self.pattern.pop().is_none() {
                    return Some(Outcome::Cancelled);
                }
            }
            Key::Char(c) => {
                if let Some(i) = self.labels.iter().position(|l| *l == Some(c)) {
                    return Some(Outcome::Land(self.hits[i]));
                }
                self.pattern.push(c);
            }
        }
        let previous: HashMap<_, _> = self
            .hits
            .iter()
            .zip(&self.labels)
            .filter_map(|(h, l)| l.map(|c| (h.key(), c)))
            .collect();
        self.hits = hits(screens, &self.pattern);
        self.labels = labels(&self.hits, folds(&self.pattern), &previous);
        None
    }
}

/// Draw the window as the overlay shows it: every pane greyed, the borders
/// between them, the matches in the accent and their labels on it.
pub fn draw(buf: &mut Buffer, screens: &[Screen], state: &State, paint: &Paint) {
    let area = buf.area;
    let dim = paint.style(Tone::Dim);
    let frame = paint.frame();

    // The borders first: any cell no pane covers is a border, and which way
    // it runs follows from which of its neighbours are borders too.
    let covered = |x: u16, y: u16| {
        screens.iter().any(|s| {
            let p = &s.pane;
            x >= p.left && x < p.left + p.width && y >= p.top && y < p.top + p.height
        })
    };
    let gap = |x: i32, y: i32| {
        x >= 0
            && y >= 0
            && x < i32::from(area.width)
            && y < i32::from(area.height)
            && !covered(x as u16, y as u16)
    };
    for y in 0..area.height {
        for x in 0..area.width {
            if covered(x, y) {
                continue;
            }
            let (xi, yi) = (i32::from(x), i32::from(y));
            let vertical = gap(xi, yi - 1) || gap(xi, yi + 1);
            let horizontal = gap(xi - 1, yi) || gap(xi + 1, yi);
            let glyph = match (vertical, horizontal) {
                (true, true) => "┼",
                (true, false) => "│",
                _ => "─",
            };
            buf[(x, y)].set_symbol(glyph).set_style(frame);
        }
    }

    for screen in screens {
        let p = &screen.pane;
        let rect = Rect::new(p.left, p.top, p.width, p.height).intersection(area);
        if rect.is_empty() {
            continue;
        }
        for (row, line) in screen.lines.iter().enumerate() {
            let y = p.top + u16::try_from(row).unwrap_or(u16::MAX);
            if y >= rect.bottom() {
                break;
            }
            let text: String = line.iter().collect();
            buf.set_stringn(p.left, y, &text, usize::from(rect.width), dim);
        }
    }

    let label_style = if paint.colour {
        Style::default()
            .fg(paint.accent)
            .add_modifier(Modifier::REVERSED | Modifier::BOLD)
    } else {
        Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD)
    };
    for (hit, label) in state.hits.iter().zip(&state.labels) {
        let screen = &screens[hit.screen];
        let p = &screen.pane;
        let line = &screen.lines[usize::from(hit.row)];
        let y = p.top + hit.row;
        let start = u16::try_from(cells_before(line, hit.col)).unwrap_or(u16::MAX);
        let end = u16::try_from(cells_before(line, hit.col + hit.len)).unwrap_or(u16::MAX);
        if start >= p.width || p.left + start >= area.width || y >= area.height {
            continue;
        }
        let text: String = line[hit.col..hit.col + hit.len].iter().collect();
        buf.set_stringn(
            p.left + start,
            y,
            &text,
            usize::from(p.width.min(area.width - p.left) - start),
            paint.matched(false),
        );
        if let Some(c) = label {
            // On the cell after the match, which is the character the label
            // was chosen not to be; on the match's last cell at the edge.
            let at = if end < p.width {
                end
            } else {
                end.saturating_sub(1)
            };
            if p.left + at < area.width {
                buf[(p.left + at, y)]
                    .set_symbol(&c.to_string())
                    .set_style(label_style);
            }
        }
    }

    // The search so far, in the bottom-right corner of the pane it started in.
    if let Some(p) = screens.iter().map(|s| &s.pane).find(|p| p.active) {
        let tone = if !state.pattern.is_empty() && state.hits.is_empty() {
            Tone::Failed
        } else {
            Tone::Strong
        };
        let prompt = format!(" jump {} ", state.pattern);
        let width = u16::try_from(prompt.chars().count()).unwrap_or(u16::MAX);
        if width <= p.width
            && p.height > 0
            && p.left + p.width <= area.width
            && p.top + p.height <= area.height
        {
            let x = p.left + p.width - width;
            let y = p.top + p.height - 1;
            buf.set_string(x, y, " jump ", dim.add_modifier(Modifier::REVERSED));
            buf.set_string(x + 6, y, format!("{} ", state.pattern), paint.style(tone));
        }
    }
}

/// Where the terminal's cursor goes: on the nearest match, the one enter
/// takes, so a screen reader or a braille display reads the line it is on;
/// at the end of the search in the corner while nothing matches.
pub fn cursor_at(screens: &[Screen], state: &State, area: Rect) -> Option<(u16, u16)> {
    let at = match state.hits.first() {
        Some(hit) => {
            let screen = &screens[hit.screen];
            let p = &screen.pane;
            let line = &screen.lines[usize::from(hit.row)];
            let x = u16::try_from(cells_before(line, hit.col)).ok()?;
            (p.left.checked_add(x)?, p.top.checked_add(hit.row)?)
        }
        None => {
            // The prompt is right-aligned with one space after the search,
            // so the end of the search is the pane's last cell.
            let p = &screens.iter().find(|s| s.pane.active)?.pane;
            (
                (p.left + p.width).checked_sub(1)?,
                (p.top + p.height).checked_sub(1)?,
            )
        }
    };
    (at.0 < area.width && at.1 < area.height).then_some(at)
}

/// `jump`: the overlay, and the pane and place picked from it.
/// Rows a status line takes above the window: what a popup's `-y` has to
/// clear, since on tmux 3.7 a popup's row is counted from the top of the
/// client and not of the window.
pub fn rows_above(status: &str, position: &str) -> u16 {
    if position != "top" {
        return 0;
    }
    match status {
        "off" => 0,
        "on" => 1,
        n => n.parse().unwrap_or(1),
    }
}

/// The `display-popup` that lays the overlay exactly over the window.
///
/// `-y` is where the popup's bottom edge goes, so a popup the window's height
/// that starts on the window's first row asks for the height plus whatever
/// the status line takes above it. `-w` and `-h` take no formats, which is why
/// this is worked out here rather than written into the binding.
pub fn popup_command(
    exe: &str,
    pane: &str,
    client: Option<&str>,
    (width, height): (u16, u16),
    above: u16,
) -> Vec<String> {
    let mut out: Vec<String> = vec!["display-popup".into(), "-B".into(), "-E".into()];
    if let Some(c) = client {
        out.extend(["-c".into(), c.into()]);
    }
    out.extend([
        "-t".into(),
        pane.into(),
        "-x".into(),
        "0".into(),
        "-y".into(),
        (u32::from(height) + u32::from(above)).to_string(),
        "-w".into(),
        width.to_string(),
        "-h".into(),
        height.to_string(),
        format!("{} jump --overlay --pane {}", quote(exe), quote(pane)),
    ]);
    out
}

fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// `jump`: open the overlay over the window of `pane`, or, with `overlay`, be
/// it, and land on the place picked from it.
pub async fn run(
    pane: Option<String>,
    client: Option<String>,
    overlay: bool,
) -> anyhow::Result<()> {
    if std::env::var_os("TMUX").is_none() {
        anyhow::bail!("jump draws over a tmux window, and this is not inside tmux");
    }
    let target = match pane {
        Some(p) => p,
        None => crate::cli::tmux_display("#{pane_id}").await,
    };
    if !overlay {
        return open(&target, client.as_deref()).await;
    }
    let listing = crate::tmux::command()
        .args(["list-panes", "-t", &target, "-F", LISTING])
        .output()
        .await?;
    let mut panes = parse_panes(&String::from_utf8_lossy(&listing.stdout));
    // The pane the key was pressed in, which a `--pane` from a script need
    // not be the active one of.
    if panes.iter().any(|p| p.id == target) {
        for p in &mut panes {
            p.active = p.id == target;
        }
    }
    if panes.is_empty() {
        anyhow::bail!("no panes in the window of {target}");
    }
    // Every pane at once: the overlay is on screen and blank until the last
    // capture is back.
    let captures: Vec<_> = panes
        .iter()
        .map(|p| tokio::spawn(crate::tmux::command().args(capture_args(p)).output()))
        .collect();
    let mut screens = Vec::with_capacity(panes.len());
    for (pane, capture) in panes.into_iter().zip(captures) {
        let out = capture.await??;
        screens.push(Screen::new(pane, &String::from_utf8_lossy(&out.stdout)));
    }

    let outcome = tokio::task::spawn_blocking(move || show(screens)).await??;
    if let (Outcome::Land(hit), screens) = outcome {
        let p = &screens[hit.screen].pane;
        let args = land_command(&p.id, p.in_mode, hit.row, hit.col);
        let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
        crate::cli::tmux(&borrowed).await;
    }
    Ok(())
}

/// Open the popup the overlay runs in.
async fn open(pane: &str, client: Option<&str>) -> anyhow::Result<()> {
    let answer = crate::cli::tmux_display_at(
        Some(pane),
        "#{window_width}\t#{window_height}\t#{status}\t#{status-position}",
    )
    .await;
    let f: Vec<&str> = answer.trim_end().split('\t').collect();
    let [width, height, status, position] = f[..] else {
        anyhow::bail!("tmux did not say how big the window of {pane} is");
    };
    let size = (width.parse()?, height.parse()?);
    let exe = std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "tmux-companion".to_string());
    let args = popup_command(&exe, pane, client, size, rows_above(status, position));
    let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
    crate::cli::tmux(&borrowed).await;
    Ok(())
}

/// The overlay's loop: draw, read a key, until a key ends it.
fn show(screens: Vec<Screen>) -> anyhow::Result<(Outcome, Vec<Screen>)> {
    use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};

    let paint = Paint::detect();
    let mut state = State::default();
    let mut terminal = ratatui::init();
    let result = (|| -> anyhow::Result<Outcome> {
        loop {
            terminal.draw(|frame| {
                draw(frame.buffer_mut(), &screens, &state, &paint);
                if let Some(at) = cursor_at(&screens, &state, frame.area()) {
                    frame.set_cursor_position(at);
                }
            })?;
            let Event::Key(key) = event::read()? else {
                continue;
            };
            if key.kind != KeyEventKind::Press {
                continue;
            }
            let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
            let key = match key.code {
                KeyCode::Esc => Key::Cancel,
                KeyCode::Char('c' | 'g') if ctrl => Key::Cancel,
                KeyCode::Enter => Key::Enter,
                KeyCode::Backspace => Key::Back,
                KeyCode::Char(c) if !ctrl => Key::Char(c),
                _ => continue,
            };
            if let Some(outcome) = state.press(&screens, key) {
                return Ok(outcome);
            }
        }
    })();
    ratatui::restore();
    Ok((result?, screens))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pane(id: &str, left: u16, top: u16, width: u16, height: u16, active: bool) -> Pane {
        Pane {
            id: id.into(),
            left,
            top,
            width,
            height,
            active,
            ..Pane::default()
        }
    }

    fn screen(p: Pane, text: &str) -> Screen {
        Screen::new(p, text)
    }

    #[test]
    fn a_zoomed_window_shows_only_its_active_pane() {
        let listing =
            "%1\t0\t0\t80\t24\t1\t0\t\t3\t4\t\t\t1\n%2\t81\t0\t80\t24\t0\t0\t\t0\t0\t\t\t1\n";
        let panes = parse_panes(listing);
        assert_eq!(panes.len(), 1);
        assert_eq!(panes[0].id, "%1");
        assert_eq!(panes[0].cursor, (3, 4));
    }

    #[test]
    fn a_pane_in_copy_mode_reports_its_copy_cursor_and_scroll() {
        let listing = "%3\t0\t0\t80\t24\t1\t1\t40\t3\t4\t7\t9\t0\n";
        let p = &parse_panes(listing)[0];
        assert!(p.in_mode);
        assert_eq!(p.scroll, 40);
        assert_eq!(p.cursor, (7, 9));
    }

    #[test]
    fn a_scrolled_view_is_captured_where_it_is_scrolled_to() {
        let p = Pane {
            scroll: 40,
            ..pane("%3", 0, 0, 80, 24, true)
        };
        assert_eq!(
            capture_args(&p).join(" "),
            "capture-pane -p -t %3 -S -40 -E -17"
        );
        let live = pane("%3", 0, 0, 80, 24, true);
        assert_eq!(capture_args(&live).join(" "), "capture-pane -p -t %3");
    }

    #[test]
    fn the_search_ignores_case_until_it_holds_a_capital() {
        let s = [screen(
            pane("%1", 0, 0, 80, 3, true),
            "Error here\nerror there",
        )];
        assert_eq!(hits(&s, "err").len(), 2);
        assert_eq!(hits(&s, "Err").len(), 1);
    }

    #[test]
    fn matches_on_a_row_do_not_overlap() {
        let s = [screen(pane("%1", 0, 0, 80, 1, true), "aaaa")];
        let found = hits(&s, "aa");
        assert_eq!(found.iter().map(|h| h.col).collect::<Vec<_>>(), [0, 2]);
    }

    #[test]
    fn the_nearest_match_comes_first_and_the_active_pane_before_others() {
        let mut active = pane("%1", 0, 0, 40, 10, true);
        active.cursor = (0, 8);
        let s = [
            screen(pane("%2", 41, 0, 40, 10, false), "\n\n\n\n\n\n\n\nfoo"),
            screen(active, "foo\n\n\n\n\n\n\nfoo"),
        ];
        let found = hits(&s, "foo");
        assert_eq!(
            found.iter().map(|h| (h.screen, h.row)).collect::<Vec<_>>(),
            [(1, 7), (1, 0), (0, 8)]
        );
    }

    #[test]
    fn no_label_is_a_character_that_would_extend_the_search() {
        let s = [screen(pane("%1", 0, 0, 80, 2, true), "fa fs fd\nff")];
        let found = hits(&s, "f");
        let l = labels(&found, true, &HashMap::new());
        let taken: HashSet<char> = "asdf".chars().collect();
        assert!(l.iter().flatten().all(|c| !taken.contains(c)), "{l:?}");
        assert!(l.iter().all(Option::is_some));
    }

    #[test]
    fn a_label_survives_the_search_narrowing() {
        let s = [screen(pane("%1", 0, 0, 80, 3, true), "abc\nabd\nxyz")];
        let mut state = State::default();
        state.press(&s, Key::Char('a'));
        let before = state.labels[1];
        state.press(&s, Key::Char('b'));
        assert_eq!(state.labels[1], before);
    }

    #[test]
    fn typing_a_label_lands_on_its_match() {
        let s = [screen(pane("%1", 0, 0, 80, 2, true), "one\ntwo")];
        let mut state = State::default();
        assert_eq!(state.press(&s, Key::Char('o')), None);
        let i = state.hits.iter().position(|h| h.row == 1).unwrap();
        let label = state.labels[i].unwrap();
        assert_eq!(
            state.press(&s, Key::Char(label)),
            Some(Outcome::Land(state.hits[i]))
        );
    }

    #[test]
    fn backspace_on_an_empty_search_closes_it() {
        let s = [screen(pane("%1", 0, 0, 80, 1, true), "x")];
        let mut state = State::default();
        assert_eq!(state.press(&s, Key::Back), Some(Outcome::Cancelled));
    }

    #[test]
    fn landing_moves_by_row_and_column_not_by_one_long_count() {
        assert_eq!(
            land_command("%4", false, 3, 5).join(" "),
            "select-pane -t %4 ; copy-mode -t %4 ; \
             send-keys -X -t %4 top-line ; \
             send-keys -X -N 3 -t %4 cursor-down ; \
             send-keys -X -t %4 start-of-line ; \
             send-keys -X -N 5 -t %4 cursor-right"
        );
    }

    #[test]
    fn landing_on_the_top_left_of_a_pane_already_in_copy_mode_counts_nothing() {
        assert_eq!(
            land_command("%4", true, 0, 0).join(" "),
            "select-pane -t %4 ; send-keys -X -t %4 top-line ; \
             send-keys -X -t %4 start-of-line"
        );
    }

    #[test]
    fn the_popup_clears_a_status_line_at_the_top_and_only_there() {
        assert_eq!(rows_above("on", "top"), 1);
        assert_eq!(rows_above("2", "top"), 2);
        assert_eq!(rows_above("off", "top"), 0);
        assert_eq!(rows_above("on", "bottom"), 0);
    }

    #[test]
    fn the_popup_sits_exactly_over_the_window() {
        assert_eq!(
            popup_command("/bin/tc", "%3", Some("/dev/ttys001"), (120, 39), 1).join(" "),
            "display-popup -B -E -c /dev/ttys001 -t %3 -x 0 -y 40 -w 120 -h 39 \
             '/bin/tc' jump --overlay --pane '%3'"
        );
    }

    #[test]
    fn the_cursor_is_on_the_nearest_match_or_after_the_search() {
        let s = [screen(pane("%1", 10, 2, 40, 5, true), "x\n  日本 go")];
        let area = Rect::new(0, 0, 80, 24);
        let mut state = State::default();
        assert_eq!(cursor_at(&s, &state, area), Some((49, 6)));
        state.press(&s, Key::Char('g'));
        // Row 1 of the pane, after two spaces, two wide characters and a space.
        assert_eq!(cursor_at(&s, &state, area), Some((10 + 7, 3)));
    }

    #[test]
    fn a_wide_character_counts_two_cells_but_one_step() {
        let s = [screen(pane("%1", 0, 0, 80, 1, true), "日本 go")];
        let h = hits(&s, "go")[0];
        assert_eq!(h.col, 3);
        assert_eq!(cells_before(&s[0].lines[0], h.col), 5);
    }
}
