//! The router: wrapping tmux's root bindings so a key an app claims is held
//! rather than taken.
//!
//! `keys route`, run from the last line of tmux.conf, reads the live root
//! table and rewrites it. Each root binding in scope moves untouched into the
//! `kc-tmux` table, and root gets a wrapper in its place: the binding shape in
//! `docs/dev/design-key-routing.md`, the one `tests/fixtures/key-hold.conf`
//! holds and the key-hold tests in `tests/e2e.rs` press. Everything here is
//! pure: the listing goes in, a tmux script comes out, and the caller sources
//! it.
//!
//! A wrapper is recognised by what it runs, not by a note, so a second run
//! regenerates it from the binding in `kc-tmux` rather than wrapping a
//! wrapper. A source of tmux.conf puts the originals back in root and the
//! next run wraps them fresh. With routing off, a run moves every original
//! back and empties the router's tables.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use super::spell;

/// The table the original bindings move into.
pub const TMUX_TABLE: &str = "kc-tmux";

/// The prefix of each key's hold table, `kc-hold-M-a`.
pub const HOLD_TABLE: &str = "kc-hold-";

/// The tmux the router needs: `send-keys -K` is 3.4.
pub const MIN_TMUX: (u32, u32) = (3, 4);

/// One binding as `list-keys` prints it, with the command kept byte for byte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binding {
    /// The table it is in.
    pub table: String,
    /// The key, unescaped.
    pub key: String,
    /// Bound with `-r`.
    pub repeat: bool,
    /// The command, exactly as printed.
    pub command: String,
}

/// Every binding in a full `list-keys` listing.
///
/// The full listing rather than `list-keys -T <table>`: tmux 3.7 answers that
/// with nothing for a table a config made, and `kc-tmux` is one.
pub fn parse_listing(listing: &str) -> Vec<Binding> {
    listing.lines().filter_map(parse_line).collect()
}

/// One `bind-key [-r] -T <table> <key> <command>` line. Only the leading words
/// are split on; the command is the rest of the line as it stands, so a
/// double space inside a quoted argument survives the move.
fn parse_line(line: &str) -> Option<Binding> {
    let mut rest = line.trim_start().strip_prefix("bind-key")?;
    let mut repeat = false;
    let mut table = None;
    let mut key = None;
    while key.is_none() {
        rest = rest.trim_start();
        let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
        let word = &rest[..end];
        rest = &rest[end..];
        match word {
            "" => return None,
            "-r" => repeat = true,
            "-T" => {
                rest = rest.trim_start();
                let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
                table = Some(rest[..end].to_string());
                rest = &rest[end..];
            }
            w if w.starts_with('-') && table.is_none() => {}
            w => key = Some(unescape(w)),
        }
    }
    let command = rest.trim().to_string();
    if command.is_empty() {
        return None;
    }
    Some(Binding {
        table: table?,
        key: key?,
        repeat,
        command,
    })
}

/// A key as `list-keys` escapes it, `M-\;`, back to `M-;`.
fn unescape(key: &str) -> String {
    let mut out = String::new();
    let mut chars = key.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(n) = chars.next() {
                out.push(n);
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Root's notes by key, from `list-keys -a -N -T root` and the plain listing.
///
/// `-a` because without it tmux 3.7 answered `-N -T root` with nothing on a
/// server whose root bindings did carry notes. With it, a key with no note is
/// listed with its command where the note would be, so a "note" that is the
/// key's command is no note at all; a note dropped here is a note the wrapper
/// would lose, and one kept wrongly would turn a command into a description.
pub fn root_notes(notes_listing: &str, listing: &[Binding]) -> HashMap<String, String> {
    let squash = |t: &str| t.split_whitespace().collect::<Vec<_>>().join(" ");
    let commands: HashMap<&str, String> = listing
        .iter()
        .filter(|b| b.table == "root")
        .map(|b| (b.key.as_str(), squash(&b.command)))
        .collect();
    crate::keys::parse_notes(notes_listing)
        .into_iter()
        .filter(|(key, note)| commands.get(key.as_str()) != Some(&squash(note)))
        .collect()
}

/// Whether a root command is a wrapper this router wrote.
pub fn is_wrapper(command: &str) -> bool {
    command.contains(&format!("switch-client -T {TMUX_TABLE}"))
}

/// Why a key in scope is left alone, or `None` when it can be routed.
///
/// The key's name goes into a table name, a format and a match pattern, and
/// a comma, a brace, a `#` or a `;` means something in each. Escaping all
/// three correctly is possible and not worth it while every key anybody has
/// asked to share is a letter, a digit or a named key, so those are what is
/// routed and the rest are reported.
pub fn unroutable(key: &str) -> Option<&'static str> {
    if !spell::in_scope(key) {
        return Some("not a Ctrl, Alt or F key");
    }
    let base = key.rsplit_once('-').map_or(key, |(_, b)| b);
    if base.is_empty() || !base.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Some("punctuation in the key's name");
    }
    None
}

/// Whether an app's name can go into a format as it is.
pub fn plain_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

/// The modes a discovered row of `app` counts in, when the config says none:
/// normal mode for a modal editor, every mode for anything else.
pub fn default_modes(app: &str) -> Option<Vec<String>> {
    matches!(app, "nvim" | "vim" | "vi" | "view").then(|| vec!["n".to_string()])
}

/// Which apps claim each key without a publisher, by canonical key: the
/// discovered rows in the modes that count, plus the declared claims.
pub fn static_claims(
    rows: &[super::discover::Row],
    apps: &BTreeMap<String, crate::config::KeyApp>,
) -> BTreeMap<String, BTreeSet<String>> {
    let mut out: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for row in rows {
        if row.layer == "tmux"
            || super::discover::REPORT_ONLY.contains(&row.layer.as_str())
            || !plain_name(&row.layer)
        {
            continue;
        }
        let modes = apps
            .get(&row.layer)
            .and_then(|a| a.modes.clone())
            .or_else(|| default_modes(&row.layer));
        let counts = match &modes {
            None => true,
            Some(m) => row.mode.is_empty() || m.contains(&row.mode),
        };
        if counts {
            out.entry(spell::canonical(&row.key))
                .or_default()
                .insert(row.layer.clone());
        }
    }
    for (app, a) in apps {
        if !plain_name(app) {
            continue;
        }
        for key in &a.claims {
            out.entry(spell::canonical(key))
                .or_default()
                .insert(app.clone());
        }
    }
    out
}

/// The condition under which `key` is held.
///
/// A publisher's claim counts while the app that set `@kc_owner` is in front.
/// An app's static claim counts in two ways. By its process name, while it is
/// in front and no publisher speaks for it: a publisher that claims nothing
/// in this mode unsets `@kc_claim` but keeps `@kc_owner`, and that has to mean
/// nothing is claimed rather than falling back to the static list. Or by
/// `@kc_app`, for an app whose process name says nothing (claude's is its
/// version), while the command its hook recorded in `@kc_owner` is still the
/// one in front, so a claude killed before its `SessionEnd` stops counting
/// once the shell is back.
pub fn condition(key: &str, apps: &BTreeSet<String>) -> String {
    let mut terms = vec![format!(
        "#{{&&:#{{==:#{{pane_current_command}},#{{@kc_owner}}}},#{{m:*|{key}|*,#{{@kc_claim}}}}}}"
    )];
    for app in apps {
        terms.push(format!(
            "#{{&&:#{{==:#{{pane_current_command}},{app}}},#{{!=:#{{@kc_owner}},{app}}}}}"
        ));
        terms.push(format!(
            "#{{&&:#{{==:#{{@kc_app}},{app}}},#{{==:#{{pane_current_command}},#{{@kc_owner}}}}}}"
        ));
    }
    let mut cond = terms.pop().unwrap_or_default();
    while let Some(t) = terms.pop() {
        cond = format!("#{{||:{t},{cond}}}");
    }
    cond
}

/// A note as a double-quoted tmux argument.
fn quoted(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        if matches!(c, '\\' | '"' | '$') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    out
}

/// `-N "note" ` when there is a note, nothing otherwise.
fn note_flag(note: &str) -> String {
    if note.is_empty() {
        String::new()
    } else {
        format!("-N {} ", quoted(note))
    }
}

/// The lines that put `original` in `kc-tmux` and a wrapper for its key in
/// root: a hold, or for `route = "app"` a straight hand-over to the app
/// while it claims the key.
pub fn wrap(
    original: &Binding,
    note: &str,
    hold_ms: u64,
    apps: &BTreeSet<String>,
    route: crate::config::KeyRoute,
) -> String {
    if route == crate::config::KeyRoute::App {
        return wrap_app(original, note, apps);
    }
    let k = &original.key;
    let hold = format!("{:.3}", hold_ms as f64 / 1000.0);
    let cond = condition(k, apps);
    let r = if original.repeat { "-r " } else { "" };
    let n = note_flag(note);
    format!(
        "bind-key {r}{n}-T {TMUX_TABLE} {k} {cmd}\n\
         bind-key {n}-T root {k} {{\n\
         \x20 if -F \"{cond}\" {{\n\
         \x20   switch-client -T {HOLD_TABLE}{k}\n\
         \x20   set -gF @kc_gen \"#{{e|+:#{{@kc_gen}},1}}\"\n\
         \x20   set -gF \"@kc_hold_#{{client_pid}}\" \"#{{@kc_gen}}\"\n\
         \x20   run-shell -b -d {hold} -C \"if -F '##{{&&:##{{==:##{{client_key_table}},{HOLD_TABLE}{k}}},##{{==:##{{@kc_hold_#{{client_pid}}}},#{{@kc_gen}}}}}}' {{ switch-client -c '#{{client_name}}' -T {TMUX_TABLE} ; send-keys -c '#{{client_name}}' -K {k} }}\"\n\
         \x20 }} {{\n\
         \x20   switch-client -T {TMUX_TABLE} ; send-keys -K {k}\n\
         \x20 }}\n\
         }}\n\
         bind-key -T {HOLD_TABLE}{k} {k} {{ switch-client -T root ; send-keys }}\n\
         bind-key -T {HOLD_TABLE}{k} Any {{ switch-client -T {TMUX_TABLE} ; send-keys -K {k} ; send-keys -K }}\n",
        cmd = original.command,
    )
}

/// The `route = "app"` wrapper: a claim sends the key to the pane at once,
/// no claim runs tmux's binding at once. A bare `send-keys` in a binding
/// sends the key that ran it.
fn wrap_app(original: &Binding, note: &str, apps: &BTreeSet<String>) -> String {
    let k = &original.key;
    let r = if original.repeat { "-r " } else { "" };
    let n = note_flag(note);
    format!(
        "bind-key {r}{n}-T {TMUX_TABLE} {k} {cmd}\n\
         bind-key {n}-T root {k} {{\n\
         \x20 if -F \"{cond}\" {{ send-keys }} {{ switch-client -T {TMUX_TABLE} ; send-keys -K {k} }}\n\
         }}\n\
         unbind-key -a -q -T {HOLD_TABLE}{k}\n",
        cmd = original.command,
        cond = condition(k, apps),
    )
}

/// The lines that put `original` back in root and take the router's own
/// bindings for its key away.
pub fn unwrap(original: &Binding, note: &str) -> String {
    let k = &original.key;
    let r = if original.repeat { "-r " } else { "" };
    format!(
        "bind-key {r}{n}-T root {k} {cmd}\n\
         unbind-key -T {TMUX_TABLE} {k}\n\
         unbind-key -a -q -T {HOLD_TABLE}{k}\n",
        n = note_flag(note),
        cmd = original.command,
    )
}

/// What a run decided, and the script that does it.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Plan {
    /// The tmux script, empty when there is nothing to do.
    pub script: String,
    /// Keys wrapped, or wrapped again.
    pub wrapped: Vec<String>,
    /// Keys moved back to root.
    pub unwrapped: Vec<String>,
    /// Keys in scope left alone, and why.
    pub skipped: Vec<(String, String)>,
}

/// Decide what a run does.
///
/// `notes` is root's notes by key; a moved binding's note goes with it and the
/// wrapper carries a copy, so the keys picker and the cheat sheet still find
/// it under the note somebody wrote.
pub fn plan(
    listing: &[Binding],
    notes: &HashMap<String, String>,
    enabled: bool,
    hold_ms: u64,
    claims: &BTreeMap<String, BTreeSet<String>>,
    registry: &BTreeMap<String, crate::config::KeyEntry>,
) -> Plan {
    use crate::config::KeyRoute;
    let entries: HashMap<String, &crate::config::KeyEntry> = registry
        .iter()
        .map(|(k, e)| (spell::canonical(k), e))
        .collect();
    let moved: HashMap<&str, &Binding> = listing
        .iter()
        .filter(|b| b.table == TMUX_TABLE)
        .map(|b| (b.key.as_str(), b))
        .collect();
    let none = BTreeSet::new();
    let mut p = Plan::default();
    for b in listing.iter().filter(|b| b.table == "root") {
        let note = notes.get(&b.key).map_or("", String::as_str);
        let wrapped = is_wrapper(&b.command);
        if !wrapped && !spell::in_scope(&b.key) {
            continue;
        }
        let original = if wrapped {
            match moved.get(b.key.as_str()) {
                Some(o) => Binding {
                    table: "root".into(),
                    ..(*o).clone()
                },
                None => {
                    p.skipped.push((
                        b.key.clone(),
                        "a wrapper with nothing in kc-tmux behind it".into(),
                    ));
                    continue;
                }
            }
        } else {
            b.clone()
        };
        let entry = entries.get(&spell::canonical(&b.key));
        let route = entry.map_or(KeyRoute::Hold, |e| e.route);
        let why = if !enabled {
            Some("routing is off")
        } else if route == KeyRoute::Tmux {
            Some("[keys.key] route = \"tmux\"")
        } else {
            unroutable(&b.key)
        };
        match why {
            None => {
                let apps = claims.get(&spell::canonical(&b.key)).unwrap_or(&none);
                let hold = entry.and_then(|e| e.hold_ms).unwrap_or(hold_ms);
                p.script.push_str(&wrap(&original, note, hold, apps, route));
                p.wrapped.push(b.key.clone());
            }
            Some(_) if wrapped => {
                p.script.push_str(&unwrap(&original, note));
                p.unwrapped.push(b.key.clone());
            }
            Some(why) if enabled => p.skipped.push((b.key.clone(), why.to_string())),
            Some(_) => {}
        }
    }
    p
}

/// `tmux -V` as `(major, minor)`: `tmux 3.5a` and `tmux next-3.6` by their
/// numbers. `None` when there are none.
pub fn tmux_version(text: &str) -> Option<(u32, u32)> {
    let word = text.split_whitespace().last()?;
    let digits: String = word
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    let mut parts = digits.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next().and_then(|m| m.parse().ok()).unwrap_or(0);
    Some((major, minor))
}

#[cfg(test)]
mod tests {
    use super::*;

    const LISTING: &str = r#"bind-key    -T prefix       c                         new-window
bind-key    -T root         M-a                       run-shell "tmux-companion toggle  '#{session_name}'"
bind-key -r -T root         M-b                       next-window
bind-key    -T root         M-\;                      next-window
bind-key    -T root         MouseDown1Pane            select-pane -t =
bind-key    -T root         Enter                     send-keys Enter
"#;

    fn notes() -> HashMap<String, String> {
        HashMap::from([("M-a".to_string(), "companion: next \"window\"".to_string())])
    }

    #[test]
    fn a_listing_line_keeps_its_command_byte_for_byte() {
        let b = parse_listing(LISTING);
        let a = b.iter().find(|b| b.key == "M-a").unwrap();
        assert_eq!(a.table, "root");
        assert_eq!(
            a.command, r#"run-shell "tmux-companion toggle  '#{session_name}'""#,
            "the double space survives"
        );
        assert!(!a.repeat);
        assert!(b.iter().find(|b| b.key == "M-b").unwrap().repeat);
        assert!(b.iter().any(|b| b.key == "M-;"), "the escape comes off");
        assert_eq!(b.len(), 6);
    }

    #[test]
    fn routing_on_wraps_root_keys_in_scope_and_reports_the_rest() {
        let p = plan(
            &parse_listing(LISTING),
            &notes(),
            true,
            170,
            &BTreeMap::new(),
            &BTreeMap::new(),
        );
        assert_eq!(p.wrapped, ["M-a", "M-b"]);
        assert_eq!(
            p.skipped,
            [(
                "M-;".to_string(),
                "punctuation in the key's name".to_string()
            )]
        );
        assert!(p.script.contains("bind-key -N \"companion: next \\\"window\\\"\" -T kc-tmux M-a run-shell \"tmux-companion toggle  '#{session_name}'\""));
        assert!(
            p.script.contains("bind-key -r -T kc-tmux M-b next-window"),
            "the repeat flag moves with it"
        );
        assert!(
            p.script
                .contains("bind-key -N \"companion: next \\\"window\\\"\" -T root M-a {"),
            "the wrapper keeps the note"
        );
        assert!(p.script.contains("run-shell -b -d 0.170 -C"));
        assert!(
            !p.script.contains("Enter"),
            "Enter is out of scope and untouched"
        );
    }

    #[test]
    fn a_second_run_rewraps_from_kc_tmux_rather_than_wrapping_a_wrapper() {
        // root and kc-tmux as tmux lists them after a first run: the original
        // moved, the wrapper in its place, on one line, with no -N
        let listing = "bind-key    -T kc-tmux M-a run-shell \"tmux-companion toggle  '#{session_name}'\"\n\
             bind-key    -T root M-a if-shell -F \"x\" { switch-client -T kc-hold-M-a } { switch-client -T kc-tmux ; send-keys -K M-a }\n";
        let second = plan(
            &parse_listing(listing),
            &notes(),
            true,
            300,
            &BTreeMap::new(),
            &BTreeMap::new(),
        );
        assert_eq!(second.wrapped, ["M-a"]);
        assert!(
            second
                .script
                .contains("-T kc-tmux M-a run-shell \"tmux-companion toggle  '#{session_name}'\""),
            "the original, not the wrapper"
        );
        assert!(second.script.contains("-d 0.300"), "the new hold");
    }

    #[test]
    fn routing_off_moves_every_original_back() {
        let listing = "bind-key -T kc-tmux M-a next-window\n\
                       bind-key -T root M-a if-shell -F \"x\" { switch-client -T kc-tmux ; send-keys -K M-a }\n\
                       bind-key -T root M-b next-window\n";
        let p = plan(
            &parse_listing(listing),
            &HashMap::new(),
            false,
            170,
            &BTreeMap::new(),
            &BTreeMap::new(),
        );
        assert_eq!(p.unwrapped, ["M-a"]);
        assert!(p.wrapped.is_empty());
        assert!(p.skipped.is_empty(), "nothing to report when off");
        assert_eq!(
            p.script,
            "bind-key -T root M-a next-window\nunbind-key -T kc-tmux M-a\nunbind-key -a -q -T kc-hold-M-a\n"
        );
    }

    #[test]
    fn routing_off_with_nothing_wrapped_does_nothing() {
        let p = plan(
            &parse_listing(LISTING),
            &notes(),
            false,
            170,
            &BTreeMap::new(),
            &BTreeMap::new(),
        );
        assert_eq!(p, Plan::default());
    }

    #[test]
    fn the_condition_is_the_publisher_or_any_static_claim() {
        assert_eq!(
            condition("M-a", &BTreeSet::new()),
            "#{&&:#{==:#{pane_current_command},#{@kc_owner}},#{m:*|M-a|*,#{@kc_claim}}}"
        );
        let apps = BTreeSet::from(["nano".to_string(), "nvim".to_string()]);
        let c = condition("M-a", &apps);
        assert!(c.starts_with("#{||:#{&&:#{==:#{pane_current_command},#{@kc_owner}}"));
        assert!(c.contains("#{&&:#{==:#{pane_current_command},nano},#{!=:#{@kc_owner},nano}}"));
        assert!(c.contains("#{&&:#{==:#{pane_current_command},nvim},#{!=:#{@kc_owner},nvim}}"));
        assert!(
            c.contains("#{&&:#{==:#{@kc_app},nano},#{==:#{pane_current_command},#{@kc_owner}}}")
        );
        assert_eq!(c.matches("#{||:").count(), 4, "five terms, four ors");
    }

    #[test]
    fn static_claims_take_the_modes_that_count() {
        use super::super::discover::{Row, Rung};
        let row = |layer: &str, mode: &str, key: &str| Row {
            layer: layer.into(),
            mode: mode.into(),
            key: key.into(),
            desc: String::new(),
            source: String::new(),
            rung: Rung::Live,
            pane: String::new(),
        };
        let rows = [
            row("tmux", "root", "M-a"),
            row("nvim", "n", "M-1"),
            row("nvim", "i", "M-q"),
            row("fzf", "", "M-a"),
            row("odd name", "", "M-z"),
            row("zsh", "emacs", "C-r"),
        ];
        let mut apps = BTreeMap::new();
        apps.insert(
            "nano".to_string(),
            crate::config::KeyApp {
                claims: vec!["C-o".into()],
                modes: None,
            },
        );
        let c = static_claims(&rows, &apps);
        assert_eq!(c.get("M-1").map(|s| s.len()), Some(1));
        assert!(
            !c.contains_key("M-q"),
            "insert mode doesn't count for nvim by default"
        );
        assert!(c["M-a"].contains("fzf"));
        assert!(
            !c.contains_key("M-z"),
            "a name that can't go in a format is left out"
        );
        assert!(c["C-o"].contains("nano"));
        assert!(!c.contains_key("C-r"), "zsh is reported, never routed");

        apps.insert(
            "nvim".to_string(),
            crate::config::KeyApp {
                claims: vec![],
                modes: Some(vec!["n".into(), "i".into()]),
            },
        );
        assert!(
            static_claims(&rows, &apps).contains_key("M-q"),
            "configured modes win"
        );
    }

    #[test]
    fn a_note_that_is_only_the_command_is_no_note() {
        let listing = parse_listing(
            "bind-key -T root M-a send-keys -l [T]\nbind-key -T root M-b next-window\n",
        );
        let notes = "C-b M-a                       companion: mark\nC-b M-b                       next-window\n";
        let n = root_notes(notes, &listing);
        assert_eq!(n.get("M-a").map(String::as_str), Some("companion: mark"));
        assert!(
            !n.contains_key("M-b"),
            "-a lists the command where a note would be"
        );
    }

    #[test]
    fn the_registry_routes_a_key_to_tmux_or_the_app_and_overrides_its_hold() {
        use crate::config::{KeyEntry, KeyRoute};
        let mut registry = BTreeMap::new();
        registry.insert(
            "M-a".to_string(),
            KeyEntry {
                route: KeyRoute::App,
                ..Default::default()
            },
        );
        registry.insert(
            "M-b".to_string(),
            KeyEntry {
                route: KeyRoute::Tmux,
                ..Default::default()
            },
        );
        let p = plan(
            &parse_listing(LISTING),
            &notes(),
            true,
            170,
            &BTreeMap::new(),
            &registry,
        );
        assert_eq!(p.wrapped, ["M-a"]);
        assert!(
            p.skipped
                .iter()
                .any(|(k, why)| k == "M-b" && why.contains("tmux"))
        );
        assert!(
            p.script
                .contains("{ send-keys } { switch-client -T kc-tmux ; send-keys -K M-a }"),
            "app: no hold"
        );
        assert!(
            !p.script.contains("run-shell -b"),
            "no timer for an app route"
        );

        registry.remove("M-b");
        registry.insert(
            "M-a".to_string(),
            KeyEntry {
                hold_ms: Some(250),
                ..Default::default()
            },
        );
        let p = plan(
            &parse_listing(LISTING),
            &notes(),
            true,
            170,
            &BTreeMap::new(),
            &registry,
        );
        assert!(p.script.contains("-d 0.250 "), "the key's own hold");
        assert!(p.script.contains("-d 0.170 "), "the default for M-b");
    }

    #[test]
    fn keys_with_punctuation_are_not_routed() {
        assert_eq!(unroutable("M-a"), None);
        assert_eq!(unroutable("C-Tab"), None);
        assert_eq!(unroutable("F5"), None);
        assert_eq!(unroutable("C-M-S-Left"), None);
        assert!(unroutable("M-;").is_some());
        assert!(unroutable("M-,").is_some());
        assert!(unroutable("M-}").is_some());
        assert!(unroutable("Enter").is_some());
    }

    #[test]
    fn versions_read_as_tmux_prints_them() {
        assert_eq!(tmux_version("tmux 3.7c"), Some((3, 7)));
        assert_eq!(tmux_version("tmux next-3.6"), Some((3, 6)));
        assert_eq!(tmux_version("tmux 3.4"), Some((3, 4)));
        assert_eq!(tmux_version("tmux master"), None);
    }
}
