//! Whether each item is on, worked out from what tmux, the config and the
//! machine say rather than from what `setup` remembers doing.
//!
//! Every decision is a pure function over captured text: the `list-keys`
//! listing, `show-hooks -g`, an option's value, the parsed config and the raw
//! file beside it, which programs are on PATH, and what the daemon has seen.
//! [`gather`] is the one place that asks for them, so the same answers serve
//! `setup`, `setup --print`, `doctor` and the brief, and the tests drive the
//! rules with strings.

use std::collections::{HashMap, HashSet};

use super::catalog::{Item, Kind};

/// What an item's state is for this user.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum State {
    /// Turned on.
    On,
    /// Not on, and nothing says anybody decided that.
    Open,
    /// Written off in the config, which is a decision rather than a gap.
    OffByChoice,
    /// Put aside from the picker.
    Skipped,
    /// Nothing here can say, such as a font, or tmux not running.
    CantTell,
}

impl State {
    /// The word for it, in `--print` and in the picker.
    pub fn word(self) -> &'static str {
        // @Yogesh(word): the five state names, in the picker's first column and in --print
        match self {
            State::On => "on",
            State::Open => "open",
            State::OffByChoice => "off-by-choice",
            State::Skipped => "skipped",
            State::CantTell => "cant-tell",
        }
    }
}

/// One binding, from `list-keys` or from a tmux.conf.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bound {
    /// The key table.
    pub table: String,
    /// The key.
    pub key: String,
    /// The `-N` note, when the source has one.
    pub note: String,
    /// The command, whitespace collapsed.
    pub command: String,
}

/// Everything the rules read.
#[derive(Debug, Clone, Default)]
pub struct Inputs {
    /// Whether a tmux server answered at all. Without one, what tmux holds
    /// cannot be told, and saying `open` would be a guess.
    pub tmux_up: bool,
    /// Every binding in every table.
    pub bindings: Vec<Bound>,
    /// `show-hooks -g`.
    pub hooks: String,
    /// Global option values, by name, for the options items name.
    pub options: HashMap<String, String>,
    /// Whether any pane carries `@tmux-companion-marks`.
    pub marks: bool,
    /// The config in force, as TOML, defaults filled in.
    pub effective: Option<toml::Value>,
    /// The config file as written, nothing filled in.
    pub raw: Option<toml::Value>,
    /// Programs asked about, and whether each is on PATH.
    pub on_path: HashMap<String, bool>,
    /// The clipboard program the config would use.
    pub clipboard_program: String,
    /// Whether the project picker reads zoxide.
    pub dirs_source_zoxide: bool,
    /// Whether the glyph preset is ascii, which is a decision about the font.
    pub ascii_glyphs: bool,
    /// Whether the daemon has had an agent report since it started; `None`
    /// when there was no daemon to ask.
    pub agent_reported: Option<bool>,
    /// Whether ~/.claude/settings.json runs `tmux-companion agent`.
    pub claude_settings_hooked: bool,
}

// ── Reading commands ─────────────────────────────────────────────────────────

/// One call of the binary, found in a command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Call {
    /// The subcommand, and the second word when the first has subcommands.
    pub path: Vec<String>,
    /// The long flags after it.
    pub flags: Vec<String>,
}

/// The subcommands that have subcommands of their own, from clap.
///
/// `project save` is two words of path and `pocket logs` is one word and a
/// name, and only clap knows which is which.
pub fn parent_commands() -> HashSet<String> {
    use clap::CommandFactory;
    crate::cli::Cli::command()
        .get_subcommands()
        .filter(|c| c.has_subcommands())
        .map(|c| c.get_name().to_string())
        .collect()
}

/// Every call of the binary in a command line.
///
/// Quotes, backslashes and semicolons are taken as separators, which is enough
/// for the three shapes a command reaches this in: as written in tmux.conf, as
/// `list-keys` prints it back with its escapes, and as `show-hooks` prints it.
pub fn invocations(command: &str, parents: &HashSet<String>) -> Vec<Call> {
    let cleaned: String = command
        .chars()
        .map(|c| {
            if matches!(c, '"' | '\'' | '\\' | ';') {
                ' '
            } else {
                c
            }
        })
        .collect();
    let words: Vec<&str> = cleaned.split_whitespace().collect();
    let bare = |w: &str| {
        !w.is_empty()
            && w.starts_with(|c: char| c.is_ascii_lowercase())
            && w.chars().all(|c| c.is_ascii_lowercase() || c == '-')
    };
    let mut out = Vec::new();
    let mut i = 0;
    while i < words.len() {
        if !words[i].ends_with("tmux-companion") {
            i += 1;
            continue;
        }
        i += 1;
        let mut path = Vec::new();
        if let Some(first) = words.get(i).filter(|w| bare(w)) {
            path.push(first.to_string());
            i += 1;
            if parents.contains(*first)
                && let Some(second) = words.get(i).filter(|w| bare(w))
            {
                path.push(second.to_string());
                i += 1;
            }
        }
        let mut flags = Vec::new();
        while i < words.len() && !words[i].ends_with("tmux-companion") {
            let w = words[i];
            if w.starts_with("--") && w.len() > 2 {
                flags.push(w.split('=').next().unwrap_or(w).to_string());
            }
            i += 1;
        }
        if !path.is_empty() {
            out.push(Call { path, flags });
        }
    }
    out
}

/// Every binding in a `list-keys` listing, whatever its table.
///
/// The per-table parser in [`crate::keys`] does the reading, key escapes
/// included; this only finds which tables there are.
pub fn listed_bindings(listing: &str) -> Vec<Bound> {
    let mut tables: Vec<String> = Vec::new();
    for line in listing.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if let Some(t) = fields.iter().position(|f| *f == "-T")
            && let Some(table) = fields.get(t + 1)
            && !tables.iter().any(|x| x == table)
        {
            tables.push(table.to_string());
        }
    }
    let mut out = Vec::new();
    for table in tables {
        for (key, command) in crate::keys::parse_commands(listing, &table) {
            out.push(Bound {
                table: table.clone(),
                key,
                note: String::new(),
                command,
            });
        }
    }
    out
}

/// The next word of a tmux.conf line, quotes honoured, and what follows it.
fn next_word(s: &str) -> Option<(String, &str)> {
    let s = s.trim_start();
    let mut chars = s.char_indices();
    let (_, first) = chars.next()?;
    if first == '"' || first == '\'' {
        let mut word = String::new();
        let mut escaped = false;
        for (i, c) in chars {
            if escaped {
                word.push(c);
                escaped = false;
            } else if c == '\\' && first == '"' {
                escaped = true;
            } else if c == first {
                return Some((word, &s[i + 1..]));
            } else {
                word.push(c);
            }
        }
        return Some((word, ""));
    }
    let end = s.find(char::is_whitespace).unwrap_or(s.len());
    Some((s[..end].to_string(), &s[end..]))
}

/// The bindings a tmux.conf writes, continuation lines joined.
///
/// Only `bind` and `bind-key` lines at the start of a line, so a binding in a
/// comment is not one. The command is kept as written with its whitespace
/// collapsed, which is what makes two spellings of the same binding compare
/// equal.
pub fn conf_bindings(text: &str) -> Vec<Bound> {
    let mut logical: Vec<String> = Vec::new();
    let mut current = String::new();
    for line in text.lines() {
        if let Some(body) = line.strip_suffix('\\') {
            current.push_str(body);
            current.push(' ');
            continue;
        }
        current.push_str(line);
        logical.push(std::mem::take(&mut current));
    }
    if !current.is_empty() {
        logical.push(current);
    }

    let mut out = Vec::new();
    for line in logical {
        let Some((head, mut rest)) = next_word(&line) else {
            continue;
        };
        if head != "bind" && head != "bind-key" {
            continue;
        }
        let mut table = "prefix".to_string();
        let mut note = String::new();
        let key = loop {
            let Some((word, after)) = next_word(rest) else {
                break None;
            };
            rest = after;
            match word.as_str() {
                "-n" => table = "root".to_string(),
                "-r" => {}
                "-T" => {
                    if let Some((t, after)) = next_word(rest) {
                        table = t;
                        rest = after;
                    }
                }
                "-N" => {
                    if let Some((n, after)) = next_word(rest) {
                        note = n;
                        rest = after;
                    }
                }
                _ => break Some(word),
            }
        };
        let Some(key) = key else { continue };
        out.push(Bound {
            table,
            key,
            note,
            command: rest.split_whitespace().collect::<Vec<_>>().join(" "),
        });
    }
    out
}

// ── The rules ────────────────────────────────────────────────────────────────

/// The flags that tell an item from a sibling running the same command.
///
/// `panes` and `panes --agents` share a path, so `--agents` is what decides
/// which one a binding is. A flag nothing else shares, like `--pane`, decides
/// nothing, so a binding written with or without it still counts.
pub fn distinguishing(items: &[Item]) -> HashMap<Vec<String>, HashSet<String>> {
    let parents = parent_commands();
    distinguishing_with(items, &parents)
}

fn distinguishing_with(
    items: &[Item],
    parents: &HashSet<String>,
) -> HashMap<Vec<String>, HashSet<String>> {
    let mut by_path: HashMap<Vec<String>, (usize, HashSet<String>)> = HashMap::new();
    for i in items.iter().filter(|i| !i.command.is_empty()) {
        for call in invocations(&format!("tmux-companion {}", i.command), parents) {
            let entry = by_path.entry(call.path).or_default();
            entry.0 += 1;
            entry.1.extend(call.flags);
        }
    }
    by_path
        .into_iter()
        .filter(|(_, (n, _))| *n > 1)
        .map(|(path, (_, flags))| (path, flags))
        .collect()
}

/// Whether a command line runs what the item runs.
fn runs(
    item: &Item,
    command: &str,
    parents: &HashSet<String>,
    distinct: &HashMap<Vec<String>, HashSet<String>>,
) -> bool {
    let Some(want) = invocations(&format!("tmux-companion {}", item.command), parents)
        .into_iter()
        .next()
    else {
        return false;
    };
    let empty = HashSet::new();
    let deciding = distinct.get(&want.path).unwrap_or(&empty);
    let signature = |call: &Call| -> Vec<String> {
        let mut f: Vec<String> = call
            .flags
            .iter()
            .filter(|f| deciding.contains(*f))
            .cloned()
            .collect();
        f.sort();
        f.dedup();
        f
    };
    let wanted = signature(&want);
    invocations(command, parents)
        .iter()
        .any(|call| call.path == want.path && signature(call) == wanted)
}

/// A value looked up by a dotted path.
pub fn lookup<'a>(value: &'a toml::Value, path: &str) -> Option<&'a toml::Value> {
    path.split('.')
        .try_fold(value, |v, part| v.as_table().and_then(|t| t.get(part)))
}

/// Whether a setting reads as on: true, a number over zero, a word other than
/// `off`, or a list with something in it.
fn truthy(value: &toml::Value) -> bool {
    match value {
        toml::Value::Boolean(b) => *b,
        toml::Value::Integer(n) => *n > 0,
        toml::Value::Float(f) => *f > 0.0,
        toml::Value::String(s) => !s.is_empty() && s != "off",
        toml::Value::Array(a) => !a.is_empty(),
        toml::Value::Table(_) | toml::Value::Datetime(_) => true,
    }
}

/// Whether an array of tables holds one of this name.
fn holds(value: Option<&toml::Value>, name: &str) -> bool {
    value.and_then(toml::Value::as_array).is_some_and(|a| {
        a.iter()
            .any(|t| t.get("name").and_then(toml::Value::as_str) == Some(name))
    })
}

/// A config item's state.
///
/// On when what is in force says so. Otherwise a key written in the file is a
/// decision -- `enabled = false`, `nudge_after_secs = 0`, or a segment list
/// written without this one -- and one left out is a gap.
pub fn config_state(
    item: &Item,
    effective: Option<&toml::Value>,
    raw: Option<&toml::Value>,
) -> State {
    let Some(effective) = effective else {
        return State::CantTell;
    };
    let in_force = lookup(effective, &item.setting);
    let on = if item.contains.is_empty() {
        in_force.is_some_and(truthy)
    } else {
        holds(in_force, &item.contains)
    };
    if on {
        return State::On;
    }
    let written = raw.and_then(|r| lookup(r, &item.setting)).is_some();
    if written {
        State::OffByChoice
    } else {
        State::Open
    }
}

/// The key an item is bound on, when it is: `(table, key)`.
pub fn bound_on(
    item: &Item,
    bindings: &[Bound],
    parents: &HashSet<String>,
    distinct: &HashMap<Vec<String>, HashSet<String>>,
) -> Option<(String, String)> {
    // The item's own table first, so a binding somebody also has in another
    // table does not hide the one they would look for.
    let mut found: Vec<&Bound> = bindings
        .iter()
        .filter(|b| runs(item, &b.command, parents, distinct))
        .collect();
    found.sort_by_key(|b| b.table != item.table);
    found.first().map(|b| (b.table.clone(), b.key.clone()))
}

/// Every item's state, skips applied.
pub fn states(items: &[Item], inputs: &Inputs, skipped: &HashSet<String>) -> Vec<State> {
    let parents = parent_commands();
    let distinct = distinguishing_with(items, &parents);
    items
        .iter()
        .map(|i| {
            let state = state_of(i, inputs, &parents, &distinct);
            match state {
                State::Open | State::CantTell if skipped.contains(&i.id) => State::Skipped,
                s => s,
            }
        })
        .collect()
}

/// One item's state, before skips.
pub fn state_of(
    item: &Item,
    inputs: &Inputs,
    parents: &HashSet<String>,
    distinct: &HashMap<Vec<String>, HashSet<String>>,
) -> State {
    let yes = |on: bool| if on { State::On } else { State::Open };
    match item.kind {
        Kind::Binding | Kind::Hook | Kind::Option if !inputs.tmux_up => State::CantTell,
        Kind::Binding => yes(bound_on(item, &inputs.bindings, parents, distinct).is_some()),
        Kind::Hook => yes(inputs.hooks.lines().any(|l| {
            let name = l.split(['[', ' ']).next().unwrap_or("");
            name == item.hook && runs(item, l, parents, distinct)
        })),
        Kind::Option => yes(inputs
            .options
            .get(&item.option)
            .is_some_and(|v| runs(item, v, parents, distinct))),
        Kind::Config => config_state(item, inputs.effective.as_ref(), inputs.raw.as_ref()),
        Kind::Outside => outside_state(item, inputs),
    }
}

/// An outside item's state.
fn outside_state(item: &Item, inputs: &Inputs) -> State {
    let yes = |on: bool| if on { State::On } else { State::Open };
    match item.check.as_str() {
        // Any pane that has run a prompt with the marks says so; no pane
        // saying so on a server with no tmux is not an answer.
        "marks" if !inputs.tmux_up => State::CantTell,
        "marks" => yes(inputs.marks),
        // The daemon's word first, since it is what an agent actually did;
        // the settings file when there is no daemon or it has heard nothing
        // yet, since an agent not started since is not a missing hook.
        "claude-hooks" => yes(inputs.agent_reported == Some(true) || inputs.claude_settings_hooked),
        "zoxide" if !inputs.dirs_source_zoxide => State::OffByChoice,
        "zoxide" => yes(inputs.on_path.get("zoxide").copied().unwrap_or(false)),
        // Nothing asks the terminal which font it draws with. The ascii
        // preset is somebody having decided not to need one.
        "nerd-font" if inputs.ascii_glyphs => State::OffByChoice,
        "nerd-font" => State::CantTell,
        "clipboard" => match inputs.on_path.get(&inputs.clipboard_program) {
            Some(found) => yes(*found),
            None => State::CantTell,
        },
        _ => State::CantTell,
    }
}

// ── Keys ─────────────────────────────────────────────────────────────────────

/// Whether a key in a table has anything bound to it, tmux's own included.
pub fn taken<'a>(bindings: &'a [Bound], table: &str, key: &str) -> Option<&'a Bound> {
    bindings.iter().find(|b| b.table == table && b.key == key)
}

/// The key to offer: the example's when it is free in its table, else the
/// first free fallback, else none.
///
/// tmux's own bindings count as taken, since they are in `list-keys` like any
/// other: `prefix c` is new-window on a bare tmux, and offering it would be
/// offering to take it away.
pub fn recommended(item: &Item, bindings: &[Bound]) -> Option<String> {
    std::iter::once(&item.key)
        .chain(item.fallback.iter())
        .find(|k| taken(bindings, &item.table, k).is_none())
        .cloned()
}

// ── Gathering ────────────────────────────────────────────────────────────────

/// One tmux call, or `None` when it failed.
async fn tmux_out(args: &[&str]) -> Option<String> {
    let out = tokio::process::Command::new("tmux")
        .args(args)
        .output()
        .await
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Whether a program is on PATH.
pub fn on_path(program: &str) -> bool {
    if program.contains('/') {
        return std::path::Path::new(program).is_file();
    }
    std::env::var_os("PATH")
        .is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join(program).is_file()))
}

/// Whether a settings file wires the agent hooks.
pub fn settings_hooked(text: &str) -> bool {
    text.contains("tmux-companion agent")
}

/// Ask the running daemon whether an agent has reported, without starting one.
async fn agent_reported() -> Option<bool> {
    tokio::net::UnixStream::connect(crate::client::sock_path())
        .await
        .ok()?;
    let r = crate::client::send_once(&crate::proto::Request::raw(
        "__setup",
        serde_json::Value::Null,
    ))
    .await
    .ok()?;
    if r.error.is_some() {
        return None;
    }
    serde_json::from_str::<crate::proto::SetupFacts>(&r.output)
        .ok()
        .map(|f| f.agent_reported)
}

/// The config in force, as TOML, and the file as written.
pub fn config_values() -> (
    Option<crate::config::Config>,
    Option<toml::Value>,
    Option<toml::Value>,
) {
    let Ok((config, source)) = crate::config::load() else {
        return (None, None, None);
    };
    let effective = toml::Value::try_from(&config).ok();
    let raw = match source {
        crate::config::Source::File(path) => std::fs::read_to_string(path)
            .ok()
            .and_then(|t| t.parse::<toml::Table>().ok())
            .map(toml::Value::Table),
        crate::config::Source::Defaults => None,
    };
    (Some(config), effective, raw)
}

/// Ask everything the rules read.
pub async fn gather(items: &[Item]) -> Inputs {
    let (keys, hooks, marks) = tokio::join!(
        tmux_out(&["list-keys"]),
        tmux_out(&["show-hooks", "-g"]),
        tmux_out(&["list-panes", "-a", "-F", "#{@tmux-companion-marks}"]),
    );
    let mut options = HashMap::new();
    for name in items
        .iter()
        .filter(|i| i.kind == Kind::Option)
        .map(|i| i.option.as_str())
    {
        if let Some(v) = tmux_out(&["show-options", "-gv", name]).await {
            options.insert(name.to_string(), v.trim_end().to_string());
        }
    }

    let (config, effective, raw) = config_values();
    let config = config.unwrap_or_default();
    let (clipboard_program, _) = config.clipboard.command();
    let mut path = HashMap::new();
    for program in ["zoxide", clipboard_program.as_str()] {
        path.insert(program.to_string(), on_path(program));
    }
    let home = std::env::var("HOME").unwrap_or_default();
    let claude_settings_hooked =
        std::fs::read_to_string(std::path::Path::new(&home).join(".claude/settings.json"))
            .is_ok_and(|t| settings_hooked(&t));

    Inputs {
        tmux_up: keys.is_some(),
        bindings: keys.as_deref().map(listed_bindings).unwrap_or_default(),
        hooks: hooks.unwrap_or_default(),
        options,
        marks: marks.is_some_and(|m| m.lines().any(|l| l.trim() == "1")),
        effective,
        raw,
        on_path: path,
        clipboard_program,
        dirs_source_zoxide: config.project.dirs_source == "zoxide",
        ascii_glyphs: config.glyphs.preset == crate::config::Preset::Ascii,
        agent_reported: agent_reported().await,
        claude_settings_hooked,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::setup::catalog::items;

    fn item(id: &str) -> Item {
        items().into_iter().find(|i| i.id == id).unwrap()
    }

    fn parents() -> HashSet<String> {
        parent_commands()
    }

    #[test]
    fn a_call_is_found_however_the_command_is_quoted() {
        let p = parents();
        let calls = invocations(
            r#"display-popup -E -w 80% -h 70% "tmux-companion panes --agents""#,
            &p,
        );
        assert_eq!(
            calls,
            vec![Call {
                path: vec!["panes".into()],
                flags: vec!["--agents".into()]
            }]
        );
        // As list-keys prints it back, escapes and all.
        let calls = invocations(
            r#"confirm-before -p "close #{session_name}? (y/n)" "run-shell 'tmux-companion project close'""#,
            &p,
        );
        assert_eq!(calls[0].path, vec!["project", "close"]);
        // A name after a command with no subcommands is not part of the path.
        let calls = invocations("run-shell \"tmux-companion pocket logs --pane '%1'\"", &p);
        assert_eq!(calls[0].path, vec!["pocket"]);
        assert_eq!(calls[0].flags, vec!["--pane"]);
        // A full path to the binary is the binary.
        let calls = invocations("run-shell '/usr/local/bin/tmux-companion brief --hook'", &p);
        assert_eq!(calls[0].path, vec!["brief"]);
        assert!(invocations("send-keys -X next-prompt", &p).is_empty());
    }

    #[test]
    fn a_tmux_conf_binding_parses_the_way_list_keys_would_describe_it() {
        let text = "\
# bind -N \"a comment\" x kill-server
bind -N \"companion: window jump to any pane\" g \\
  display-popup -E -w 80% -h 70% \"tmux-companion panes\"
bind -n M-s display-popup -E \"tmux-companion project\"
bind-key -r -T copy-mode-vi o run-shell \"tmux-companion open\"
";
        let b = conf_bindings(text);
        assert_eq!(b.len(), 3, "{b:?}");
        assert_eq!(b[0].table, "prefix");
        assert_eq!(b[0].key, "g");
        assert_eq!(b[0].note, "companion: window jump to any pane");
        assert_eq!(
            b[0].command,
            "display-popup -E -w 80% -h 70% \"tmux-companion panes\""
        );
        assert_eq!((b[1].table.as_str(), b[1].key.as_str()), ("root", "M-s"));
        assert_eq!(
            (b[2].table.as_str(), b[2].key.as_str()),
            ("copy-mode-vi", "o")
        );
    }

    /// What `list-keys` answers with a few of the example's bindings in place.
    const LISTING: &str = r#"bind-key    -T copy-mode-vi o                 run-shell "tmux-companion open --pane '#{pane_id}'"
bind-key    -T prefix       c                 new-window
bind-key    -T prefix       g                 display-popup -E -w 80% -h 70% "tmux-companion panes"
bind-key    -T prefix       \%                split-window -h
bind-key    -T root         M-a               run-shell "tmux-companion toggle '#{session_name}'"
"#;

    #[test]
    fn a_binding_is_on_when_any_table_runs_its_command() {
        let p = parents();
        let all = items();
        let distinct = distinguishing_with(&all, &p);
        let bindings = listed_bindings(LISTING);
        assert_eq!(bindings.len(), 5);
        assert!(bindings.iter().any(|b| b.key == "%"), "escape undone");
        let on = |id: &str| bound_on(&item(id), &bindings, &p, &distinct);
        assert_eq!(on("panes"), Some(("prefix".into(), "g".into())));
        assert_eq!(on("open"), Some(("copy-mode-vi".into(), "o".into())));
        assert_eq!(on("toggle"), Some(("root".into(), "M-a".into())));
        // `panes --agents` is a different item from `panes`, and the other
        // way round.
        assert_eq!(on("panes-agents"), None);
        assert_eq!(on("toggle-last"), None);
        assert_eq!(on("inbox"), None);

        // Bound in another table than the example's is still bound.
        let elsewhere = listed_bindings(
            "bind-key -T root F5 display-popup -E \"tmux-companion panes --agents\"\n",
        );
        assert_eq!(
            bound_on(&item("panes-agents"), &elsewhere, &p, &distinct),
            Some(("root".into(), "F5".into()))
        );
        assert_eq!(bound_on(&item("panes"), &elsewhere, &p, &distinct), None);
    }

    #[test]
    fn a_hook_is_on_when_show_hooks_has_it_under_its_name() {
        let inputs = Inputs {
            tmux_up: true,
            hooks: "client-attached[41] run-shell \"tmux-companion brief --hook\"\n\
                    session-created[0] run-shell 'tmux-companion theme apply \"x\" -t \"x\"'\n"
                .into(),
            ..Inputs::default()
        };
        let got = |id: &str| states(&[item(id)], &inputs, &HashSet::new())[0];
        assert_eq!(got("brief-hook"), State::On);
        assert_eq!(got("theme-hook"), State::On);
        assert_eq!(got("start-hook"), State::Open);
    }

    #[test]
    fn the_bar_is_on_when_status_right_calls_it() {
        let mut inputs = Inputs {
            tmux_up: true,
            ..Inputs::default()
        };
        let got = |inputs: &Inputs| states(&[item("status-right")], inputs, &HashSet::new())[0];
        assert_eq!(got(&inputs), State::Open);
        inputs.options.insert(
            "status-right".into(),
            "#(tmux-companion sh-jobs #{pane_pid})#(tmux-companion status-right #{pane_current_path})".into(),
        );
        assert_eq!(got(&inputs), State::On);
    }

    #[test]
    fn without_a_tmux_server_nothing_tmux_holds_can_be_told() {
        let inputs = Inputs::default();
        for id in ["panes", "brief-hook", "status-right", "shell-init"] {
            assert_eq!(
                states(&[item(id)], &inputs, &HashSet::new())[0],
                State::CantTell,
                "{id}"
            );
        }
    }

    fn parse(text: &str) -> toml::Value {
        toml::Value::Table(text.parse::<toml::Table>().unwrap())
    }

    #[test]
    fn a_setting_written_off_is_a_decision_and_one_left_out_is_open() {
        let defaults = toml::Value::try_from(crate::config::Config::default()).unwrap();
        let online = item("online");
        assert_eq!(config_state(&online, Some(&defaults), None), State::Open);
        let raw = parse("[online]\nenabled = false\n");
        assert_eq!(
            config_state(&online, Some(&defaults), Some(&raw)),
            State::OffByChoice
        );
        let raw = parse("[online]\nenabled = true\n");
        let effective = toml::Value::try_from(
            crate::config::parse("[online]\nenabled = true\n", std::path::Path::new("x")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            config_state(&online, Some(&effective), Some(&raw)),
            State::On
        );
        // A table written without the key is not a decision about the key.
        let raw = parse("[online]\ninterval_secs = 60\n");
        assert_eq!(
            config_state(&online, Some(&defaults), Some(&raw)),
            State::Open
        );
    }

    #[test]
    fn the_journal_and_the_usage_log_are_open_until_turned_on() {
        // Both were on by default until they became opt-in, so this is the
        // shape every background setting has now: open, on once written on,
        // off by choice once written off.
        let defaults = toml::Value::try_from(crate::config::Config::default()).unwrap();
        for (id, table) in [("journal-config", "journal"), ("usage", "usage")] {
            let it = item(id);
            assert_eq!(
                config_state(&it, Some(&defaults), None),
                State::Open,
                "{id}"
            );
            for (value, want) in [("true", State::On), ("false", State::OffByChoice)] {
                let text = format!("[{table}]\nenabled = {value}\n");
                let effective = toml::Value::try_from(
                    crate::config::parse(&text, std::path::Path::new("x")).unwrap(),
                )
                .unwrap();
                let raw = parse(&text);
                assert_eq!(
                    config_state(&it, Some(&effective), Some(&raw)),
                    want,
                    "{id} = {value}"
                );
            }
        }
    }

    #[test]
    fn a_number_and_a_word_read_the_way_the_config_means_them() {
        let defaults = toml::Value::try_from(crate::config::Config::default()).unwrap();
        assert_eq!(
            config_state(&item("nudge"), Some(&defaults), None),
            State::Open
        );
        assert_eq!(
            config_state(&item("sessions-autosave"), Some(&defaults), None),
            State::Open
        );
        let raw = parse("[sessions]\nautosave = \"off\"\n");
        assert_eq!(
            config_state(&item("sessions-autosave"), Some(&defaults), Some(&raw)),
            State::OffByChoice
        );
        let raw = parse("[agents]\nnudge_after_secs = 0\n");
        assert_eq!(
            config_state(&item("nudge"), Some(&defaults), Some(&raw)),
            State::OffByChoice
        );
    }

    #[test]
    fn a_segment_is_on_when_the_list_holds_it_and_off_when_a_list_was_written_without_it() {
        let defaults = toml::Value::try_from(crate::config::Config::default()).unwrap();
        let agents = item("segment-agents");
        assert_eq!(config_state(&agents, Some(&defaults), None), State::Open);

        let text = "[[status.right.segments]]\nname = \"git\"\n";
        let effective =
            toml::Value::try_from(crate::config::parse(text, std::path::Path::new("x")).unwrap())
                .unwrap();
        assert_eq!(
            config_state(&agents, Some(&effective), Some(&parse(text))),
            State::OffByChoice
        );

        let text = "[[status.right.segments]]\nname = \"agents\"\n";
        let effective =
            toml::Value::try_from(crate::config::parse(text, std::path::Path::new("x")).unwrap())
                .unwrap();
        assert_eq!(
            config_state(&agents, Some(&effective), Some(&parse(text))),
            State::On
        );
    }

    #[test]
    fn a_config_that_does_not_parse_cannot_say() {
        assert_eq!(config_state(&item("online"), None, None), State::CantTell);
    }

    #[test]
    fn the_outside_checks_read_what_they_were_given() {
        let mut inputs = Inputs {
            tmux_up: true,
            dirs_source_zoxide: true,
            clipboard_program: "pbcopy".into(),
            ..Inputs::default()
        };
        let got = |inputs: &Inputs, id: &str| states(&[item(id)], inputs, &HashSet::new())[0];
        assert_eq!(got(&inputs, "shell-init"), State::Open);
        inputs.marks = true;
        assert_eq!(got(&inputs, "shell-init"), State::On);

        assert_eq!(got(&inputs, "claude-hooks"), State::Open);
        inputs.claude_settings_hooked = true;
        assert_eq!(got(&inputs, "claude-hooks"), State::On);
        inputs.claude_settings_hooked = false;
        inputs.agent_reported = Some(true);
        assert_eq!(got(&inputs, "claude-hooks"), State::On);

        assert_eq!(got(&inputs, "zoxide"), State::Open);
        inputs.on_path.insert("zoxide".into(), true);
        assert_eq!(got(&inputs, "zoxide"), State::On);
        inputs.dirs_source_zoxide = false;
        assert_eq!(got(&inputs, "zoxide"), State::OffByChoice);

        assert_eq!(got(&inputs, "nerd-font"), State::CantTell);
        inputs.ascii_glyphs = true;
        assert_eq!(got(&inputs, "nerd-font"), State::OffByChoice);

        assert_eq!(got(&inputs, "clipboard-tool"), State::CantTell);
        inputs.on_path.insert("pbcopy".into(), false);
        assert_eq!(got(&inputs, "clipboard-tool"), State::Open);
        inputs.on_path.insert("pbcopy".into(), true);
        assert_eq!(got(&inputs, "clipboard-tool"), State::On);
    }

    #[test]
    fn a_skip_hides_what_is_open_and_never_what_is_on() {
        let inputs = Inputs {
            tmux_up: true,
            bindings: listed_bindings(LISTING),
            ..Inputs::default()
        };
        let skipped: HashSet<String> = ["panes".to_string(), "inbox".to_string()].into();
        let got = states(&[item("panes"), item("inbox")], &inputs, &skipped);
        assert_eq!(got, vec![State::On, State::Skipped]);
    }

    #[test]
    fn the_key_offered_is_the_examples_when_free_and_a_fallback_when_not() {
        let bindings = listed_bindings(LISTING);
        // `prefix c` is tmux's own new-window, so it is taken.
        assert_eq!(
            recommended(&item("new-window"), &bindings),
            Some("N".into())
        );
        // `prefix G` is free.
        assert_eq!(
            recommended(&item("panes-agents"), &bindings),
            Some("G".into())
        );
        // Taken in another table is not taken in this one.
        let other = listed_bindings("bind-key -T root G something\n");
        assert_eq!(recommended(&item("panes-agents"), &other), Some("G".into()));
        // Everything taken: nothing to offer.
        let full = listed_bindings(
            "bind-key -T prefix G a\nbind-key -T prefix A b\nbind-key -T prefix M-G c\n",
        );
        assert_eq!(recommended(&item("panes-agents"), &full), None);
        let first_taken = listed_bindings("bind-key -T prefix G a\n");
        assert_eq!(
            recommended(&item("panes-agents"), &first_taken),
            Some("A".into())
        );
    }

    #[test]
    fn the_settings_file_counts_only_when_it_runs_the_agent_command() {
        assert!(settings_hooked(
            r#"{"hooks":{"Stop":[{"hooks":[{"command":"tmux-companion agent done"}]}]}}"#
        ));
        assert!(!settings_hooked(r#"{"model":"opus"}"#));
    }
}
