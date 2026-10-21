//! the switcher: a session's windows, and the server's sessions, in the order you used them, a press to move and Enter to go
//!
//! Cmd-Tab's shape. One press is where you just were, each further press the
//! next older, Shift goes back, and you move only when you commit. A terminal
//! never sees Alt let go, so the list waits for Enter; `[switcher] commit =
//! "pause"` lets a pause after the last press stand in for letting go.
//!
//! The order comes from two tmux hooks that only set options, so a switch
//! starts no process: every window entered takes the next number of a
//! server-wide counter as `@tc_win_used`, every session switched to takes it
//! as `@tc_ses_used`. tmux's own `window_last_flag` is one step of history and
//! `session_last_attached` is whole seconds; the counter is every step and
//! can't tie. `docs/dev/design-switcher.md` has the measurements.
//!
//! Before a popup is open each press is a `run-shell` and a process. With
//! `first = "flip"` the first press goes to the last window and draws
//! nothing, `flip_wait_ms` after the press: a second press inside that wait
//! opens the list where the presses began, with nothing moved, and one
//! after it but inside `pause_ms` opens it over the flip. The wait is a
//! process of its own, so the press returns at once and tmux reads the next
//! key. Once
//! the popup is up, tmux hands it every key, so the rest of the presses are
//! key events in one process. What a press needs to know about the ones
//! before it is kept in the session option `@tc_switch`.

use crate::config::{SwitchCommit, SwitchFirst, Switcher as Settings};

/// The global counter the hooks count switches with.
pub const SEQ: &str = "@tc_seq";

/// The window option a window's last entry is stamped in.
///
/// Not the same name as the session one: a format looks an `@` option up in
/// the window before the session, so a shared name read back as the current
/// window's stamp in `list-sessions`.
pub const WIN_USED: &str = "@tc_win_used";

/// The session option a session's last switch is stamped in.
pub const SES_USED: &str = "@tc_ses_used";

/// The session option holding the presses in progress.
pub const STATE: &str = "@tc_switch";

/// Where in each hook's array the switcher's entry goes: well clear of `[0]`,
/// which is where a plain `set-hook -g` in somebody's tmux.conf lands.
pub const HOOK_INDEX: u32 = 73;

/// The hooks, as tmux commands, ready to chain in front of another call.
///
/// Setting them again is harmless, so every `switch` sets them on its way and
/// a binding is all anybody needs; the example configs set them as well, so
/// the history starts with the server rather than with the first press.
pub fn hooks() -> Vec<Vec<String>> {
    let count = format!("set -gF {SEQ} \"#{{e|+:#{{{SEQ}}},1}}\"");
    [
        (
            "session-window-changed",
            format!("set -wF {WIN_USED} \"#{{{SEQ}}}\""),
        ),
        (
            "client-session-changed",
            format!("set -F {SES_USED} \"#{{{SEQ}}}\""),
        ),
        // An attach from outside is a switch to that session too, and fires
        // only this one.
        (
            "client-attached",
            format!("set -F {SES_USED} \"#{{{SEQ}}}\""),
        ),
    ]
    .into_iter()
    .map(|(hook, stamp)| {
        vec![
            "set-hook".to_string(),
            "-g".to_string(),
            format!("{hook}[{HOOK_INDEX}]"),
            format!("{count} ; {stamp}"),
        ]
    })
    .collect()
}

/// What `list-windows -F` is asked for, tab-separated, the name last because
/// it may hold anything.
pub const LIST_FORMAT: &str = "#{window_index}\t#{window_id}\t#{window_active}\t#{window_last_flag}\t#{@tc_win_used}\t#{window_name}";

/// One window of the session.
#[derive(Debug, Clone, PartialEq)]
pub struct Window {
    pub index: u32,
    pub id: String,
    pub active: bool,
    pub last: bool,
    /// The counter's value when the window was last entered, when the hook
    /// has seen it entered.
    pub used: Option<u64>,
    pub name: String,
}

/// The session's windows, as [`LIST_FORMAT`] lists them.
///
/// The window pockets are parked in is not one to switch to, so it is left
/// out unless it is where you are.
pub fn parse(list: &str) -> Vec<Window> {
    list.lines()
        .filter_map(|l| {
            let mut f = l.splitn(6, '\t');
            let index = f.next()?.trim().parse().ok()?;
            let id = f.next()?.to_string();
            let active = f.next()? == "1";
            let last = f.next()? == "1";
            let used = f.next()?.trim().parse().ok();
            let name = f.next().unwrap_or_default().to_string();
            if !active && name == crate::pocket::WINDOW {
                return None;
            }
            Some(Window {
                index,
                id,
                active,
                last,
                used,
                name,
            })
        })
        .collect()
}

/// Most recently used first.
///
/// The window you are in is first whatever its stamp says. Then the stamps,
/// highest first. A window the hook has never seen entered comes after every
/// one it has, tmux's last window first and then by index, which makes the
/// first press right on a server that has never run the hook.
pub fn by_recency(mut windows: Vec<Window>) -> Vec<Window> {
    windows.sort_by(|a, b| {
        b.active
            .cmp(&a.active)
            .then(b.used.cmp(&a.used))
            .then(b.last.cmp(&a.last))
            .then(a.index.cmp(&b.index))
    });
    windows
}

/// The order as it was before the presses began: the window they began in
/// first, the rest as they are.
///
/// After a flip the window gone to has the newest stamp and the one left the
/// next, so putting the one left back in front is the order the first press
/// saw.
pub fn origin_first(mut order: Vec<Window>, origin: &str) -> Vec<Window> {
    if let Some(i) = order.iter().position(|w| w.id == origin) {
        let w = order.remove(i);
        order.insert(0, w);
    }
    order
}

/// A position counted in presses, either way, as a row of `n`.
pub fn wrap(pos: i64, n: usize) -> usize {
    if n == 0 {
        return 0;
    }
    let n = n as i64;
    (((pos % n) + n) % n) as usize
}

/// The presses in progress, as [`STATE`] holds them.
#[derive(Debug, Clone, PartialEq)]
pub struct Pressed {
    /// When the latest press was, in milliseconds since the epoch.
    pub ms: u64,
    /// The window the presses began in.
    pub origin: String,
    /// Where a flip landed, or `None` once a popup has been asked for.
    pub flipped_to: Option<String>,
    /// The flip is still waiting out `flip_wait_ms`: `flipped_to` is where
    /// it will go, and nothing has moved yet.
    pub waiting: bool,
    /// How many presses forward, less how many back, since they began.
    pub pos: i64,
}

impl Pressed {
    /// `1790067600000 @3 @5 1`, `... ~@5 1` while the flip waits, or
    /// `... - 2` once a popup is on its way.
    pub fn encode(&self) -> String {
        let to = match (&self.flipped_to, self.waiting) {
            (None, _) => "-".to_string(),
            (Some(id), true) => format!("~{id}"),
            (Some(id), false) => id.clone(),
        };
        format!("{} {} {} {}", self.ms, self.origin, to, self.pos)
    }

    /// The other half of [`Pressed::encode`]; anything else is no presses.
    pub fn decode(s: &str) -> Option<Self> {
        let mut f = s.split_whitespace();
        let ms = f.next()?.parse().ok()?;
        let origin = f.next()?.to_string();
        let (flipped_to, waiting) = match f.next()? {
            "-" => (None, false),
            id => match id.strip_prefix('~') {
                Some(id) => (Some(id.to_string()), true),
                None => (Some(id.to_string()), false),
            },
        };
        let pos = f.next()?.parse().ok()?;
        Some(Self {
            ms,
            origin,
            flipped_to,
            waiting,
            pos,
        })
    }

    /// Whether a popup has been asked for and these presses are its.
    pub fn opening(&self) -> bool {
        self.flipped_to.is_none()
    }
}

/// What one press from the root table does.
#[derive(Debug, Clone, PartialEq)]
pub enum Act {
    /// One window, or none: nowhere to go.
    Nothing,
    /// Go to the window at this place in the order now, and keep the presses.
    Flip(usize, Pressed),
    /// Keep the presses, and go to the window at this place once
    /// `flip_wait_ms` has gone without another press.
    Wait(usize, Pressed),
    /// Open the list, and keep the presses for it to read.
    Open(Pressed),
    /// A popup is on its way: count this press for it.
    Count(Pressed),
}

/// What a press does, given the windows most recent first, the presses kept
/// by the last one, the time, and which way this press goes (`1` or `-1`).
/// `wait_ms` is `flip_wait_ms`: 0 flips at once.
pub fn on_press(
    order: &[Window],
    kept: Option<&Pressed>,
    now: u64,
    step: i64,
    first: SwitchFirst,
    pause_ms: u64,
    wait_ms: u64,
) -> Act {
    if order.len() < 2 {
        return Act::Nothing;
    }
    let here = &order[0].id;
    let recent = kept.filter(|p| now >= p.ms && now - p.ms < pause_ms);
    if let Some(p) = recent {
        if p.opening() {
            return Act::Count(Pressed {
                ms: now,
                pos: p.pos + step,
                ..p.clone()
            });
        }
        // A second press after a flip, still on the window the flip went to,
        // or before a waiting flip went anywhere.
        let still = if p.waiting {
            p.flipped_to.is_some() && &p.origin == here
        } else {
            p.flipped_to.as_deref() == Some(here)
        };
        if still {
            return Act::Open(Pressed {
                ms: now,
                origin: p.origin.clone(),
                flipped_to: None,
                waiting: false,
                pos: p.pos + step,
            });
        }
    }
    match first {
        SwitchFirst::Flip => {
            let i = wrap(step, order.len());
            let p = Pressed {
                ms: now,
                origin: here.clone(),
                flipped_to: Some(order[i].id.clone()),
                waiting: wait_ms > 0,
                pos: step,
            };
            if wait_ms > 0 {
                Act::Wait(i, p)
            } else {
                Act::Flip(i, p)
            }
        }
        SwitchFirst::List => Act::Open(Pressed {
            ms: now,
            origin: here.clone(),
            flipped_to: None,
            waiting: false,
            pos: step,
        }),
    }
}

/// The windows to select, in order, to commit to `target` from `current`
/// when the presses began in `origin`.
///
/// Through the origin when a flip moved away from it, so the hooks stamp it
/// before the target and the order afterwards is the one Cmd-Tab leaves: the
/// window gone to, then the one the presses began in.
pub fn commit_path(current: &str, origin: &str, target: &str) -> Vec<String> {
    if target == current {
        return Vec::new();
    }
    if origin != current && target != origin {
        return vec![origin.to_string(), target.to_string()];
    }
    vec![target.to_string()]
}

/// The window to go back to on Esc: where the presses began, if a flip left
/// it.
pub fn cancel_path(current: &str, origin: &str) -> Vec<String> {
    if origin == current {
        Vec::new()
    } else {
        vec![origin.to_string()]
    }
}

/// What the message line says for a candidate: its name and its place.
pub fn announcement(name: &str, at: usize, of: usize) -> String {
    format!("{name}, {at} of {of}")
}

/// The line of keys the window list shows.
pub fn footer(key: char, commit: SwitchCommit) -> String {
    let upper = key.to_ascii_uppercase();
    match commit {
        SwitchCommit::Pause => {
            format!("enter go   alt-{key} next   alt-{upper} back   esc stay   a pause goes too")
        }
        SwitchCommit::Enter => format!("enter go   alt-{key} next   alt-{upper} back   esc stay"),
    }
}

/// The popup's width and height for `n` windows whose longest name is
/// `longest`, under a line of keys `footer` long, inside a client this size.
///
/// Six rows go to chrome: the border's two, the line of keys, the two rules
/// either side of the list and the query. The width is the line of keys, or
/// the index, the name and the `now` column if they are wider.
pub fn popup_size(n: usize, longest: usize, footer: usize, client: (u16, u16)) -> (u16, u16) {
    let (cw, ch) = client;
    let w = (longest + 20).max(footer + 4) as u16;
    let h = (n.min(20) + 6) as u16;
    (
        w.min(cw.saturating_sub(4)).max(20),
        h.min(ch.saturating_sub(2)).max(7),
    )
}

/// Milliseconds since the epoch.
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// `a ; b ; c` as one tmux argument list.
fn chained(cmds: Vec<Vec<String>>) -> Vec<String> {
    let mut out = Vec::new();
    for c in cmds {
        if !out.is_empty() {
            out.push(";".to_string());
        }
        out.extend(c);
    }
    out
}

fn args(a: &[&str]) -> Vec<String> {
    a.iter().map(|s| s.to_string()).collect()
}

/// What one tmux call tells a press or a popup: the session's id, its
/// current window, the client's size, the presses kept, and the windows.
struct Seen {
    session: String,
    current: String,
    client: (u16, u16),
    kept: Option<Pressed>,
    /// tmux's version, for the one flag the popup needs 3.3 for.
    version: String,
    /// `@tc-host` on the session: set on a mirror session, whose windows are
    /// the host's, so the key goes to the host.
    host: String,
    /// `@tc-remote` on a mirror session: the session on the host.
    remote: String,
    windows: Vec<Window>,
}

/// Set the hooks, then read everything in one call. `session` is a name or
/// an id; `None` is the session tmux thinks is current.
async fn look(session: Option<&str>, client: Option<&str>) -> Seen {
    let target = session.map(|s| {
        if s.starts_with('$') {
            format!("{s}:")
        } else {
            format!("={s}:")
        }
    });
    let mut display = args(&["display-message", "-p"]);
    if let Some(c) = client {
        display.extend(args(&["-c", c]));
    }
    if let Some(t) = &target {
        display.extend(args(&["-t", t]));
    }
    display.push(format!(
        "#{{session_id}}\t#{{window_id}}\t#{{client_width}}\t#{{client_height}}\t#{{version}}\t#{{{HOST}}}\t#{{{REMOTE}}}\t#{{{STATE}}}",
        HOST = crate::link::HOST_OPTION,
        REMOTE = crate::link::REMOTE_OPTION
    ));
    let mut list = args(&["list-windows", "-F", LIST_FORMAT]);
    if let Some(t) = &target {
        list.extend(args(&["-t", t.trim_end_matches(':')]));
    }
    let mut cmds = hooks();
    cmds.push(display);
    cmds.push(list);
    let out = crate::tmux::command()
        .args(chained(cmds))
        .output()
        .await
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    let mut lines = out.lines();
    let head = lines.next().unwrap_or_default();
    let mut f = head.split('\t');
    let session = f.next().unwrap_or_default().to_string();
    let current = f.next().unwrap_or_default().to_string();
    let w = f.next().and_then(|v| v.parse().ok()).unwrap_or(80);
    let h = f.next().and_then(|v| v.parse().ok()).unwrap_or(24);
    let version = f.next().unwrap_or_default().to_string();
    let host = f.next().unwrap_or_default().to_string();
    let remote = f.next().unwrap_or_default().to_string();
    let kept = f.next().and_then(Pressed::decode);
    let rest: String = lines.map(|l| format!("{l}\n")).collect();
    Seen {
        session,
        current,
        client: (w, h),
        kept,
        version,
        host,
        remote,
        windows: parse(&rest),
    }
}

/// The session's id when only a client was named: one more call, made only
/// when the binding passed no session.
async fn session_of(client: &str) -> Option<String> {
    let out = crate::tmux::command()
        .args(["display-message", "-p", "-c", client, "#{session_id}"])
        .output()
        .await
        .ok()?;
    let id = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!id.is_empty()).then_some(id)
}

/// `display-message` for a candidate, on the client when it is known.
fn say(client: Option<&str>, text: &str) -> Vec<String> {
    let mut a = args(&["display-message"]);
    if let Some(c) = client {
        a.extend(args(&["-c", c]));
    }
    a.push(crate::picker::literal_for_tmux(text));
    a
}

/// The key a press of the window key is, as `send-keys` names it: `M-a`,
/// or `M-A` going the other way.
pub fn key_name(key: char, reverse: bool) -> String {
    let k = if reverse {
        key.to_ascii_uppercase()
    } else {
        key
    };
    format!("M-{k}")
}

/// In a mirror session the windows are the host's: the press goes on to the
/// mirror pane, where the host's own switcher takes it. tmux writes `M-a` as
/// `ESC a` in one write, which the host's tmux reads as `M-a` again.
async fn to_host(seen: &Seen, key: char, reverse: bool) -> anyhow::Result<()> {
    let target = format!("{}:", seen.session);
    crate::cli::tmux(&["send-keys", "-t", &target, &key_name(key, reverse)]).await;
    Ok(())
}

/// One press of the window key from tmux's root table: `run-shell`, no
/// terminal.
pub async fn press(
    session: Option<String>,
    client: Option<String>,
    reverse: bool,
    settings: &Settings,
) -> anyhow::Result<()> {
    let session = match (session, &client) {
        (Some(s), _) => Some(s),
        (None, Some(c)) => session_of(c).await,
        (None, None) => None,
    };
    let seen = look(session.as_deref(), client.as_deref()).await;
    if !seen.host.is_empty() {
        return to_host(&seen, settings.windows_key, reverse).await;
    }
    let order = by_recency(seen.windows.clone());
    let step = if reverse { -1 } else { 1 };
    let at = format!("{}:", seen.session);
    let keep = |p: &Pressed| args(&["set-option", "-t", &at, STATE, &p.encode()]);
    match on_press(
        &order,
        seen.kept.as_ref(),
        now_ms(),
        step,
        settings.first,
        settings.pause_ms,
        settings.flip_wait_ms,
    ) {
        Act::Nothing => Ok(()),
        Act::Count(p) => {
            crate::cli::tmux(&borrow(&keep(&p))).await;
            Ok(())
        }
        Act::Flip(i, p) => {
            flip(&order, i, &p, &at, client.as_deref(), settings.announce).await;
            Ok(())
        }
        Act::Wait(_, p) => {
            crate::cli::tmux(&borrow(&keep(&p))).await;
            // Not waited for, like the popup: this press returns now, so tmux
            // reads the next key while the wait runs.
            let exe = std::env::current_exe()?;
            let mut waiter = std::process::Command::new(exe);
            waiter.args(["switch", "windows", &seen.session, "--land", &p.encode()]);
            if let Some(c) = &client {
                waiter.args(["--client", c]);
            }
            waiter
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()?;
            Ok(())
        }
        Act::Open(p) => {
            let longest = order
                .iter()
                .map(|w| w.name.chars().count())
                .max()
                .unwrap_or(0);
            let keys = footer(settings.windows_key, settings.commit)
                .chars()
                .count();
            let (w, h) = popup_size(order.len(), longest, keys, seen.client);
            let exe = std::env::current_exe()?;
            let mut inner = format!(
                "{} switch windows {} --pressed",
                quote(&exe.display().to_string()),
                quote(&seen.session)
            );
            if let Some(c) = &client {
                inner.push_str(&format!(" --client {}", quote(c)));
            }
            let mut popup = args(&["display-popup"]);
            if let Some(c) = &client {
                popup.extend(args(&["-c", c]));
            }
            popup.extend(args(&["-t", &at]));
            // The picker draws its own border; tmux's goes when it can, from
            // 3.3, and a 3.2 popup has both.
            if borderless(&seen.version) {
                popup.push("-B".to_string());
            }
            popup.extend(args(&[
                "-E",
                "-w",
                &w.to_string(),
                "-h",
                &h.to_string(),
                &inner,
            ]));
            // Not waited for: `display-popup` from a command client holds
            // until the popup closes, and `run-shell` would hold the client's
            // command queue for as long. Nothing of it is read, so nothing
            // keeps the run-shell's pipe open either.
            crate::tmux::command_sync()
                .args(chained(vec![keep(&p), popup]))
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()?;
            Ok(())
        }
    }
}

/// Go to the window at `i` in `order`, keep the presses, and say where.
async fn flip(
    order: &[Window],
    i: usize,
    p: &Pressed,
    at: &str,
    client: Option<&str>,
    announce: bool,
) {
    let w = &order[i];
    let mut cmds = vec![
        args(&["select-window", "-t", &format!("{at}{}", w.id)]),
        args(&["set-option", "-t", at, STATE, &p.encode()]),
    ];
    if announce {
        cmds.push(say(client, &announcement(&w.name, i + 1, order.len())));
    }
    crate::cli::tmux(&borrow(&chained(cmds))).await;
}

/// The waiting flip a press left: once `flip_wait_ms` from the press has
/// gone, go where it said, unless another press has taken the presses on or
/// the window has changed some other way.
///
/// Read and then act, two tmux calls: a press landing between them opens the
/// list over a flip, which is the path a press after the wait takes anyway,
/// and Esc there still goes back to where the presses began.
pub async fn land(
    session: Option<String>,
    client: Option<String>,
    waiting: &str,
    settings: &Settings,
) -> anyhow::Result<()> {
    let Some(p) = Pressed::decode(waiting).filter(|p| p.waiting) else {
        return Ok(());
    };
    let due = p.ms + settings.flip_wait_ms;
    tokio::time::sleep(std::time::Duration::from_millis(
        due.saturating_sub(now_ms()),
    ))
    .await;
    let seen = look(session.as_deref(), client.as_deref()).await;
    if seen.kept.as_ref() != Some(&p) || seen.current != p.origin {
        return Ok(());
    }
    let order = by_recency(seen.windows);
    let Some(i) = order
        .iter()
        .position(|w| Some(&w.id) == p.flipped_to.as_ref())
    else {
        return Ok(());
    };
    let landed = Pressed {
        waiting: false,
        ..p
    };
    let at = format!("{}:", seen.session);
    flip(
        &order,
        i,
        &landed,
        &at,
        client.as_deref(),
        settings.announce,
    )
    .await;
    Ok(())
}

/// Whether this tmux has `display-popup -B`, which came in 3.3. `3.5a` and
/// `next-3.6` count by their numbers; an unreadable version is taken as new.
pub fn borderless(version: &str) -> bool {
    let digits: String = version
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    let mut parts = digits.split('.').map(|p| p.parse::<u32>().ok());
    match (parts.next().flatten(), parts.next().flatten()) {
        (Some(a), Some(b)) => (a, b) >= (3, 3),
        _ => true,
    }
}

fn borrow(v: &[String]) -> Vec<&str> {
    v.iter().map(String::as_str).collect()
}

/// Quote for the shell `display-popup` hands its command to.
fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// The list itself, in a popup: `switch windows` with a terminal.
///
/// `pressed` says a press opened it and left `@tc_switch` for it to read:
/// where the presses began and how far they have gone. Without it, from a
/// binding that opens the popup directly, the presses begin here and this is
/// the first.
pub async fn list(
    session: Option<String>,
    client: Option<String>,
    reverse: bool,
    pressed: bool,
    settings: &Settings,
    picker: &crate::config::PickerLayout,
) -> anyhow::Result<()> {
    let seen = look(session.as_deref(), client.as_deref()).await;
    if !seen.host.is_empty() {
        return to_host(&seen, settings.windows_key, reverse).await;
    }
    let step: i64 = if reverse { -1 } else { 1 };
    let (origin, pos) = match seen.kept.as_ref().filter(|_| pressed) {
        Some(p) => (p.origin.clone(), p.pos),
        None => (seen.current.clone(), step),
    };
    let order = origin_first(by_recency(seen.windows.clone()), &origin);
    let at = format!("{}:", seen.session);
    let clear = args(&["set-option", "-u", "-t", &at, STATE]);
    if order.len() < 2 {
        crate::cli::tmux(&borrow(&clear)).await;
        crate::picker::say_nothing_to_show("no other window in this session", false).await;
        return Ok(());
    }

    let items: Vec<crate::picker::Item> = order
        .iter()
        .map(|w| {
            use crate::picker::Cell;
            let mut cells = vec![Cell::dim(w.index.to_string()), Cell::strong(w.name.clone())];
            // In words as well as first in the list: where you are is said,
            // not only shown by position.
            if w.id == origin {
                cells.push(Cell::dim("now"));
            }
            crate::picker::Item::new(format!("{} {}", w.index, w.name)).in_cells(cells)
        })
        .collect();

    let key = settings.windows_key;
    let upper = key.to_ascii_uppercase();
    let footer = footer(key, settings.commit);
    let on_step = settings.announce.then(|| {
        let names: Vec<String> = order.iter().map(|w| w.name.clone()).collect();
        let client = client.clone();
        crate::picker::OnStep::new(move |i| {
            let text = announcement(&names[i], i + 1, names.len());
            let _ = crate::tmux::command_sync()
                .args(say(client.as_deref(), &text))
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn();
        })
    });
    let chrome = crate::picker::Chrome {
        title: "[ Windows ]".into(),
        icon: crate::tmux::icons::SESSION.into(),
        footer,
        preview_title: String::new(),
        steps: vec![(key, 1), (upper, -1)],
        commit_after: match settings.commit {
            SwitchCommit::Pause => Some(std::time::Duration::from_millis(settings.pause_ms)),
            SwitchCommit::Enter => None,
        },
        on_step,
        ..Default::default()
    }
    .configured(picker, crate::config::Picker::Switch);

    let start = wrap(pos, order.len());
    let outcome = crate::picker::run_from(items, "", &chrome, start, None)?;

    // The current window and the presses again: a press made while the popup
    // was being put up went to the root table and was counted there.
    let now = look(session.as_deref(), client.as_deref()).await;
    let late = match (pressed, now.kept.as_ref()) {
        (true, Some(p)) if p.origin == origin => p.pos - pos,
        _ => 0,
    };
    let path = match outcome {
        crate::picker::Outcome::Chosen(i) => {
            let i = wrap(i as i64 + late, order.len());
            commit_path(&now.current, &origin, &order[i].id)
        }
        _ => cancel_path(&now.current, &origin),
    };
    let mut cmds: Vec<Vec<String>> = path
        .iter()
        .map(|id| args(&["select-window", "-t", &format!("{at}{id}")]))
        .collect();
    cmds.push(clear);
    crate::cli::tmux(&borrow(&chained(cmds))).await;
    Ok(())
}

/// `switch windows --print`: index and name, most recent first, one a line.
///
/// In a mirror session the windows are the host's, as its bar shows them:
/// the current one first, then the rest by index, from what the daemon last
/// heard. Until it has heard them it says so in one line and lists the
/// mirror session's own window.
pub async fn print(session: Option<String>, client: Option<String>) -> anyhow::Result<()> {
    let seen = look(session.as_deref(), client.as_deref()).await;
    if !seen.host.is_empty() {
        match crate::remote::mirror_windows(&seen.host, &seen.remote).await {
            Ok(mut wins) => {
                wins.sort_by_key(|w| (!w.active, w.index));
                for w in wins {
                    println!("{} {}", w.index, w.name);
                }
                return Ok(());
            }
            Err(why) => println!("{why}"),
        }
    }
    for w in by_recency(seen.windows) {
        println!("{} {}", w.index, w.name);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Windows from a picture: `a* b:3 c- d:1`, where `*` is the active one,
    /// `-` tmux's last, and `:n` the stamp. Index from 1, id `@` and the index.
    fn session(picture: &str) -> Vec<Window> {
        let list: String = picture
            .split_whitespace()
            .enumerate()
            .map(|(i, w)| {
                let (w, used) = w.split_once(':').unwrap_or((w, ""));
                let name = w.trim_end_matches(['*', '-']);
                format!(
                    "{}\t@{}\t{}\t{}\t{used}\t{name}\n",
                    i + 1,
                    i + 1,
                    u8::from(w.ends_with('*')),
                    u8::from(w.ends_with('-')),
                )
            })
            .collect();
        parse(&list)
    }

    fn names(order: &[Window]) -> String {
        order
            .iter()
            .map(|w| w.name.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Select a window the way tmux does with the hooks set: it becomes
    /// active and takes the next stamp.
    fn select(windows: &mut [Window], id: &str, seq: &mut u64) {
        for w in windows.iter_mut() {
            w.last = w.active && w.id != id;
            w.active = w.id == id;
        }
        *seq += 1;
        if let Some(w) = windows.iter_mut().find(|w| w.id == id) {
            w.used = Some(*seq);
        }
    }

    #[test]
    fn the_order_is_the_order_the_windows_were_entered_in() {
        let order = by_recency(session("a:2 b*:5 c:4 d:1"));
        assert_eq!(names(&order), "b c a d");
    }

    #[test]
    fn unstamped_windows_follow_the_stamped_ones_last_window_first() {
        // A server that has never run the hook: the last window is still
        // the first press, which is what tmux alone can say.
        let order = by_recency(session("a b c- d*"));
        assert_eq!(names(&order), "d c a b");
        let order = by_recency(session("a b:1 c- d*:2"));
        assert_eq!(names(&order), "d b c a");
    }

    #[test]
    fn the_window_you_are_in_is_first_whatever_its_stamp() {
        let order = by_recency(session("a:9 b*:1"));
        assert_eq!(names(&order), "b a");
    }

    #[test]
    fn the_parked_pocket_is_left_out_unless_you_are_in_it() {
        let list = "1\t@1\t1\t0\t3\tedit\n2\t@2\t0\t0\t2\t_pocket\n3\t@3\t0\t1\t1\tmy logs\n";
        assert_eq!(names(&parse(list)), "edit my logs");
        let list = "1\t@1\t0\t1\t3\tedit\n2\t@2\t1\t0\t4\t_pocket\n";
        assert_eq!(names(&parse(list)), "edit _pocket");
    }

    #[test]
    fn a_tab_in_a_name_stays_in_the_name() {
        let w = parse("1\t@1\t1\t0\t\tlogs\tand more\n");
        assert_eq!(w[0].name, "logs\tand more");
        assert_eq!(w[0].used, None);
    }

    #[test]
    fn wrap_counts_either_way_round() {
        assert_eq!(wrap(1, 4), 1);
        assert_eq!(wrap(5, 4), 1);
        assert_eq!(wrap(-1, 4), 3);
        assert_eq!(wrap(-5, 4), 3);
        assert_eq!(wrap(0, 0), 0);
    }

    #[test]
    fn the_presses_read_back_what_they_wrote() {
        let p = Pressed {
            ms: 1_790_067_600_000,
            origin: "@3".into(),
            flipped_to: Some("@5".into()),
            waiting: false,
            pos: -2,
        };
        assert_eq!(Pressed::decode(&p.encode()), Some(p.clone()));
        let open = Pressed {
            flipped_to: None,
            ..p
        };
        assert_eq!(open.encode(), "1790067600000 @3 - -2");
        assert_eq!(Pressed::decode(&open.encode()), Some(open));
        assert_eq!(Pressed::decode(""), None);
        assert_eq!(Pressed::decode("x @3 - 1"), None);
    }

    #[test]
    fn one_press_flips_to_where_you_were_and_draws_nothing() {
        let order = by_recency(session("a:1 b:2 c*:3"));
        match on_press(&order, None, 1_000, 1, SwitchFirst::Flip, 500, 0) {
            Act::Flip(i, p) => {
                assert_eq!(order[i].name, "b");
                assert_eq!(p.origin, "@3");
                assert_eq!(p.flipped_to.as_deref(), Some("@2"));
                assert_eq!(p.pos, 1);
            }
            other => panic!("{other:?}"),
        }
        // Shift from rest goes round to the oldest.
        match on_press(&order, None, 1_000, -1, SwitchFirst::Flip, 500, 0) {
            Act::Flip(i, _) => assert_eq!(order[i].name, "a"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_second_press_inside_the_pause_opens_the_list_on_the_next_older() {
        let mut w = session("a:1 b:2 c*:3 d");
        let mut seq = 3;
        let order = by_recency(w.clone());
        let Act::Flip(i, kept) = on_press(&order, None, 1_000, 1, SwitchFirst::Flip, 500, 0) else {
            panic!("the first press flips");
        };
        let flipped = order[i].id.clone();
        select(&mut w, &flipped, &mut seq);
        let order = by_recency(w.clone());
        assert_eq!(names(&order), "b c a d");
        let Act::Open(p) = on_press(&order, Some(&kept), 1_300, 1, SwitchFirst::Flip, 500, 0)
        else {
            panic!("the second press opens the list");
        };
        // The list is the order the first press saw, on its third row.
        let list = origin_first(order, &p.origin);
        assert_eq!(names(&list), "c b a d");
        assert_eq!(list[wrap(p.pos, list.len())].name, "a");
    }

    #[test]
    fn with_a_wait_the_first_press_moves_nothing_yet() {
        let order = by_recency(session("a:1 b:2 c*:3"));
        match on_press(&order, None, 1_000, 1, SwitchFirst::Flip, 500, 150) {
            Act::Wait(i, p) => {
                assert_eq!(order[i].name, "b");
                assert_eq!(p.origin, "@3");
                assert_eq!(p.flipped_to.as_deref(), Some("@2"));
                assert!(p.waiting);
                assert_eq!(p.encode(), "1000 @3 ~@2 1");
                assert_eq!(Pressed::decode(&p.encode()), Some(p));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_second_press_inside_the_wait_opens_the_list_where_it_began() {
        let order = by_recency(session("a:1 b:2 c*:3 d"));
        let Act::Wait(_, kept) = on_press(&order, None, 1_000, 1, SwitchFirst::Flip, 500, 150)
        else {
            panic!("the first press waits");
        };
        // Still in c: the flip never landed.
        let Act::Open(p) = on_press(&order, Some(&kept), 1_100, 1, SwitchFirst::Flip, 500, 150)
        else {
            panic!("the second press opens the list");
        };
        assert!(p.opening());
        // The same row a second press after a landed flip opens on.
        let list = origin_first(order, &p.origin);
        assert_eq!(names(&list), "c b a d");
        assert_eq!(list[wrap(p.pos, list.len())].name, "a");
    }

    #[test]
    fn a_waiting_flip_left_by_another_key_is_not_carried_on() {
        // The flip from c was waiting, but a was selected some other way.
        let order = by_recency(session("a*:5 b:2 c:3"));
        let kept = Pressed {
            ms: 1_000,
            origin: "@3".into(),
            flipped_to: Some("@2".into()),
            waiting: true,
            pos: 1,
        };
        assert!(matches!(
            on_press(&order, Some(&kept), 1_100, 1, SwitchFirst::Flip, 500, 150),
            Act::Wait(..)
        ));
    }

    #[test]
    fn a_press_after_the_pause_is_a_new_flip() {
        let order = by_recency(session("a:1 b*:3 c:2"));
        let kept = Pressed {
            ms: 1_000,
            origin: "@3".into(),
            flipped_to: Some("@2".into()),
            waiting: false,
            pos: 1,
        };
        match on_press(&order, Some(&kept), 1_500, 1, SwitchFirst::Flip, 500, 0) {
            Act::Flip(i, p) => {
                assert_eq!(order[i].name, "c");
                assert_eq!(p.origin, "@2");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_flip_left_by_another_key_is_not_carried_on() {
        // The flip went to b, but a was selected some other way since.
        let order = by_recency(session("a*:5 b:4 c:3"));
        let kept = Pressed {
            ms: 1_000,
            origin: "@3".into(),
            flipped_to: Some("@2".into()),
            waiting: false,
            pos: 1,
        };
        assert!(matches!(
            on_press(&order, Some(&kept), 1_100, 1, SwitchFirst::Flip, 500, 0),
            Act::Flip(..)
        ));
    }

    #[test]
    fn presses_while_the_popup_is_on_its_way_are_counted_for_it() {
        let order = by_recency(session("a:1 b*:3 c:2"));
        let kept = Pressed {
            ms: 1_000,
            origin: "@3".into(),
            flipped_to: None,
            waiting: false,
            pos: 2,
        };
        match on_press(&order, Some(&kept), 1_050, -1, SwitchFirst::Flip, 500, 0) {
            Act::Count(p) => {
                assert_eq!(p.pos, 1);
                assert_eq!(p.ms, 1_050);
                assert_eq!(p.origin, "@3");
            }
            other => panic!("{other:?}"),
        }
        // A popup that never came is forgotten after the pause.
        assert!(matches!(
            on_press(&order, Some(&kept), 2_000, 1, SwitchFirst::Flip, 500, 0),
            Act::Flip(..)
        ));
    }

    #[test]
    fn list_first_opens_on_the_first_press_and_moves_nothing() {
        let order = by_recency(session("a:1 b*:3 c:2"));
        match on_press(&order, None, 0, 1, SwitchFirst::List, 500, 0) {
            Act::Open(p) => {
                assert_eq!(p.origin, "@2");
                assert_eq!(p.pos, 1);
                assert!(p.opening());
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn one_window_goes_nowhere() {
        let order = by_recency(session("a*"));
        assert_eq!(
            on_press(&order, None, 0, 1, SwitchFirst::Flip, 500, 0),
            Act::Nothing
        );
        assert_eq!(
            on_press(&[], None, 0, 1, SwitchFirst::List, 500, 0),
            Act::Nothing
        );
    }

    #[test]
    fn a_commit_after_a_flip_goes_through_where_the_presses_began() {
        // Began in c, flipped to b, committed to a: c then a, so the order
        // afterwards is a, c, b, the one Cmd-Tab leaves.
        assert_eq!(commit_path("@2", "@3", "@1"), ["@3", "@1"]);
        // Committed to where the flip already is: nothing to do.
        assert!(commit_path("@2", "@3", "@2").is_empty());
        // Committed back to the start.
        assert_eq!(commit_path("@2", "@3", "@3"), ["@3"]);
        // No flip (the list opened first): straight there.
        assert_eq!(commit_path("@3", "@3", "@1"), ["@1"]);
    }

    #[test]
    fn the_whole_run_leaves_the_order_cmd_tab_would() {
        // Entered a, b, c, d in that order; d is where you are.
        let mut w = session("a:1 b:2 c:3 d*:4");
        let mut seq = 4;
        let order = by_recency(w.clone());
        let Act::Flip(i, kept) = on_press(&order, None, 0, 1, SwitchFirst::Flip, 500, 0) else {
            panic!()
        };
        let id = order[i].id.clone();
        select(&mut w, &id, &mut seq);
        let Act::Open(p) = on_press(
            &by_recency(w.clone()),
            Some(&kept),
            100,
            1,
            SwitchFirst::Flip,
            500,
            0,
        ) else {
            panic!()
        };
        let list = origin_first(by_recency(w.clone()), &p.origin);
        // One more press inside the popup, then the pause: the third press
        // from d is a.
        let target = list[wrap(p.pos + 1, list.len())].id.clone();
        assert_eq!(target, "@1");
        let current = w.iter().find(|x| x.active).unwrap().id.clone();
        for id in commit_path(&current, &p.origin, &target) {
            select(&mut w, &id, &mut seq);
        }
        assert_eq!(names(&by_recency(w.clone())), "a d c b");
        // And Esc instead would have left it as it began.
        let mut w2 = session("a:1 b:2 c*:5 d:4");
        let mut seq2 = 5;
        for id in cancel_path("@3", "@4") {
            select(&mut w2, &id, &mut seq2);
        }
        assert_eq!(names(&by_recency(w2)), "d c b a");
    }

    #[test]
    fn the_hooks_count_and_stamp_with_tmux_commands_only() {
        let h = hooks();
        assert_eq!(h.len(), 3);
        assert_eq!(h[0][..3], ["set-hook", "-g", "session-window-changed[73]"]);
        assert_eq!(
            h[0][3],
            "set -gF @tc_seq \"#{e|+:#{@tc_seq},1}\" ; set -wF @tc_win_used \"#{@tc_seq}\""
        );
        assert_eq!(h[1][2], "client-session-changed[73]");
        assert!(h[1][3].ends_with("set -F @tc_ses_used \"#{@tc_seq}\""));
        assert_eq!(h[2][2], "client-attached[73]");
        // Nothing in either starts a process.
        assert!(h.iter().all(|c| !c[3].contains("run-shell")));
    }

    #[test]
    fn the_popup_fits_the_list_and_the_client() {
        let keys = footer('a', SwitchCommit::Pause).chars().count();
        assert_eq!(keys, 64);
        assert_eq!(popup_size(4, 10, keys, (200, 50)), (68, 10));
        assert_eq!(popup_size(40, 60, keys, (200, 50)), (80, 26));
        // A small client clamps it.
        assert_eq!(popup_size(40, 60, keys, (50, 12)), (46, 10));
        assert_eq!(
            footer('a', SwitchCommit::Enter),
            "enter go   alt-a next   alt-A back   esc stay"
        );
    }

    #[test]
    fn the_border_comes_off_from_tmux_3_3() {
        assert!(borderless("3.4"));
        assert!(borderless("3.5a"));
        assert!(borderless("next-3.6"));
        assert!(borderless("3.3"));
        assert!(!borderless("3.2a"));
        assert!(borderless(""));
    }

    #[test]
    fn the_announcement_is_the_name_and_the_place() {
        assert_eq!(announcement("ai", 2, 4), "ai, 2 of 4");
    }
}
