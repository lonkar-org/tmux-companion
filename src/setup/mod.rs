//! `setup`: everything the tool offers, whether each is on for this user, and
//! a way to add what is missing.
//!
//! The list is [`catalog`](crate::setup::catalog), data compiled in. Whether an item is on is
//! [`detect`](crate::setup::detect), read fresh from tmux and the config every time the list opens,
//! so a binding somebody wrote by hand shows as on without `setup` having
//! been involved. Adding one copies its lines to the clipboard first and then
//! offers to write them, into a fenced block in tmux.conf ([`block`](crate::setup::block)) or into
//! config.toml with its comments kept ([`merge`](crate::setup::merge)); the answer defaults to no.
//!
//! The picker runs in the client, because it owns a terminal and the daemon
//! has none. The daemon is asked one thing, whether an agent has reported,
//! and nothing is written through it.

pub mod block;
pub mod catalog;
pub mod detect;
pub mod merge;

use std::collections::{HashMap, HashSet};
use std::io::Write as _;
use std::path::{Path, PathBuf};

use catalog::{Item, Kind, Target};
use detect::{Inputs, State};

/// The skip file's name in the state directory.
pub const SKIP_FILE: &str = "setup-skipped.tsv";

/// The file the daemon stamps with the build it first ran and when.
pub const BUILD_FILE: &str = "build-seen";

/// How long after an install or an upgrade the brief mentions what is open.
pub const FRESH_DAYS: u64 = 7;

// ── Skips ────────────────────────────────────────────────────────────────────

/// The ids in a skip file: one per line, blank lines and `#` lines ignored.
///
/// An id the catalog no longer has is kept rather than dropped, so a skip
/// outlives a release that renames nothing and one that briefly lost an item.
pub fn parse_skips(text: &str) -> HashSet<String> {
    text.lines()
        .map(|l| l.split('\t').next().unwrap_or("").trim())
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_string)
        .collect()
}

/// A skip file for a set of ids, sorted so two writes of one set are the
/// same bytes.
pub fn render_skips(skips: &HashSet<String>) -> String {
    let mut ids: Vec<&String> = skips.iter().collect();
    ids.sort();
    ids.into_iter().map(|id| format!("{id}\n")).collect()
}

/// The set with an id skipped, or put back when it was.
pub fn toggled(mut skips: HashSet<String>, id: &str) -> HashSet<String> {
    if !skips.remove(id) {
        skips.insert(id.to_string());
    }
    skips
}

fn skips_path() -> Option<PathBuf> {
    crate::server::state_dir().map(|d| d.join(SKIP_FILE))
}

/// The skips on disk, none when there is no file.
pub fn load_skips() -> HashSet<String> {
    skips_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|t| parse_skips(&t))
        .unwrap_or_default()
}

fn save_skips(skips: &HashSet<String>) -> anyhow::Result<()> {
    let Some(path) = skips_path() else {
        anyhow::bail!("no state directory: neither XDG_STATE_HOME nor HOME is set");
    };
    block::write_through(&path, &render_skips(skips))?;
    Ok(())
}

// ── The build stamp ──────────────────────────────────────────────────────────

/// What the stamp file should say now, or `None` when it already says it.
///
/// The first time a daemon runs a build, the file gets the build and the time;
/// every later start of the same build leaves it alone, so the time is when
/// this build arrived rather than when the daemon last restarted.
pub fn stamp_text(existing: Option<&str>, build: &str, now: u64) -> Option<String> {
    if existing.and_then(|t| first_seen(t, build)).is_some() {
        return None;
    }
    Some(format!("{build}\t{now}\n"))
}

/// When a build was first run, from the stamp file, if the file is about it.
pub fn first_seen(text: &str, build: &str) -> Option<u64> {
    let (stamped, secs) = text.lines().next()?.split_once('\t')?;
    (stamped == build)
        .then(|| secs.trim().parse().ok())
        .flatten()
}

/// Whether a build first seen at `since` is still new at `now`.
pub fn is_fresh(since: Option<u64>, now: u64) -> bool {
    since.is_some_and(|t| now.saturating_sub(t) < FRESH_DAYS * 86_400)
}

/// Stamp the state directory with this build, the first time it runs.
///
/// Called by the daemon as it comes up. The daemon is the one process every
/// upgrade restarts -- a client from a newer build replaces an older daemon on
/// its first call -- so its first start is the upgrade, near enough. The
/// binary's own mtime was the other candidate and is the build time rather
/// than the install time under Homebrew, which pours a bottle built days
/// earlier.
pub fn record_build() {
    let Some(dir) = crate::server::state_dir() else {
        return;
    };
    let path = dir.join(BUILD_FILE);
    let existing = std::fs::read_to_string(&path).ok();
    if let Some(text) = stamp_text(
        existing.as_deref(),
        &crate::proto::build_id(),
        crate::panes::now_secs(),
    ) {
        let _ = std::fs::create_dir_all(&dir);
        let _ = block::write_through(&path, &text);
    }
}

/// Whether this build is within [`FRESH_DAYS`] of first running.
pub fn build_is_fresh() -> bool {
    let since = crate::server::state_dir()
        .and_then(|d| std::fs::read_to_string(d.join(BUILD_FILE)).ok())
        .and_then(|t| first_seen(&t, &crate::proto::build_id()));
    is_fresh(since, crate::panes::now_secs())
}

// ── Counting ─────────────────────────────────────────────────────────────────

/// How many items are open, and how many there are.
pub fn open_count(states: &[State]) -> (usize, usize) {
    (
        states.iter().filter(|s| **s == State::Open).count(),
        states.len(),
    )
}

/// Ask, and count: what `doctor` and the brief show.
pub async fn count() -> (usize, usize) {
    let items = catalog::items();
    let inputs = detect::gather(&items).await;
    open_count(&detect::states(&items, &inputs, &load_skips()))
}

/// The doctor's line.
pub fn doctor_line(open: usize, total: usize) -> String {
    // @Yogesh(word): doctor's setup line, after the label
    format!("{open} of {total} open")
}

// ── The rows ─────────────────────────────────────────────────────────────────

/// The order rows are listed in: open first, then what cannot be told, then
/// what is decided either way, and what was skipped last. Catalog order
/// inside each.
pub fn order(states: &[State]) -> Vec<usize> {
    let rank = |s: State| match s {
        State::Open => 0,
        State::CantTell => 1,
        State::OffByChoice | State::On => 2,
        State::Skipped => 3,
    };
    let mut idx: Vec<usize> = (0..states.len()).collect();
    idx.sort_by_key(|i| (rank(states[*i]), *i));
    idx
}

/// A key as a row shows it: table, then key.
pub fn key_label(table: &str, key: &str) -> String {
    format!("{table} {key}")
}

/// The key each binding item is on, or would be offered on.
///
/// Bound: where it is bound. Otherwise the one chosen this session, else the
/// one [`detect::recommended`] offers. `None` for everything that is not a
/// binding, and for a binding with nowhere free to go.
pub fn keys_for(
    items: &[Item],
    inputs: &Inputs,
    chosen: &HashMap<String, String>,
) -> Vec<Option<(String, String)>> {
    let parents = detect::parent_commands();
    let distinct = detect::distinguishing(items);
    items
        .iter()
        .map(|i| {
            if i.kind != Kind::Binding {
                return None;
            }
            if let Some(found) = detect::bound_on(i, &inputs.bindings, &parents, &distinct) {
                return Some(found);
            }
            chosen
                .get(&i.id)
                .cloned()
                .or_else(|| detect::recommended(i, &inputs.bindings))
                .map(|k| (i.table.clone(), k))
        })
        .collect()
}

/// `--print`: one row per item, tab-separated: id, group, state, key, line.
pub fn tsv(items: &[Item], states: &[State], keys: &[Option<(String, String)>]) -> String {
    let mut out = String::new();
    for (i, item) in items.iter().enumerate() {
        let key = keys[i]
            .as_ref()
            .map(|(t, k)| key_label(t, k))
            .unwrap_or_default();
        out.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\n",
            item.id,
            item.group.name(),
            states[i].word(),
            key,
            item.line
        ));
    }
    out
}

/// The preview for a row: what it is for, the lines, and where they go.
pub fn preview(item: &Item, key: Option<&str>, file: &str) -> String {
    // @Yogesh(word): the label in front of the file a row's lines go in, in the preview
    let goes = "goes in";
    format!(
        "{}\n\n{}\n\n{goes} {file}\n",
        item.line,
        item.snippet_for(key)
    )
}

// ── Where things are written ─────────────────────────────────────────────────

/// The tmux.conf to write, from tmux's `#{config_files}`.
///
/// tmux lists every file it loaded, `/etc/tmux.conf` and anything sourced
/// included, in the order it read them. The first one under the home
/// directory is the one somebody edits; failing that the first one; failing
/// that, nothing, and the caller falls back to where tmux would look.
pub fn pick_conf(config_files: &str, home: &str) -> Option<PathBuf> {
    let files: Vec<&str> = config_files
        .split(',')
        .map(str::trim)
        .filter(|f| !f.is_empty())
        .collect();
    let home = home.trim_end_matches('/');
    files
        .iter()
        .find(|f| !home.is_empty() && f.starts_with(&format!("{home}/")))
        .or(files.first())
        .map(PathBuf::from)
}

/// The tmux.conf this server loaded, or the one tmux would load.
pub async fn tmux_conf() -> PathBuf {
    let files = crate::cli::tmux_capture(&["display-message", "-p", "#{config_files}"]).await;
    let home = std::env::var("HOME").unwrap_or_default();
    pick_conf(files.trim(), &home).unwrap_or_else(crate::keys::tmux_conf_path)
}

/// The config.toml in force, or where `config init` would put one.
pub fn config_file() -> anyhow::Result<PathBuf> {
    match crate::config::load() {
        Ok((_, crate::config::Source::File(p))) => Ok(p),
        Ok((_, crate::config::Source::Defaults)) => crate::config::search_paths()
            .into_iter()
            .next()
            .ok_or_else(|| {
                anyhow::anyhow!("nowhere to write: neither XDG_CONFIG_HOME nor HOME is set")
            }),
        // @Yogesh(word): refusing to write a config.toml that does not parse
        Err(e) => anyhow::bail!("{e}\nconfig.toml does not parse; fix it before adding to it"),
    }
}

/// Put a file back the way it was: its old text, or gone when it was new.
fn restore(path: &Path, old: Option<&str>) {
    match old {
        Some(text) => {
            let _ = block::write_through(path, text);
        }
        None => {
            let _ = std::fs::remove_file(block::resolve(path));
        }
    }
}

/// Add an item to tmux.conf's block and source the file.
///
/// A file tmux refuses is put back as it was, and tmux's words are the error.
pub async fn add_to_tmux_conf(
    path: &Path,
    item: &Item,
    key: Option<&str>,
    catalog: &[Item],
) -> anyhow::Result<PathBuf> {
    let old = std::fs::read_to_string(path).ok();
    let entries = block::with(
        block::entries(old.as_deref().unwrap_or("")),
        block::Entry {
            id: item.id.clone(),
            key: key.map(str::to_string),
            body: Vec::new(),
        },
    );
    let text = block::apply(old.as_deref().unwrap_or(""), &entries, catalog);
    let landed = block::write_through(path, &text)?;
    let out = tokio::process::Command::new("tmux")
        .arg("source-file")
        .arg(&landed)
        .output()
        .await;
    let why = match out {
        Ok(o) if o.status.success() => return Ok(landed),
        Ok(o) => format!(
            "{}{}",
            String::from_utf8_lossy(&o.stdout),
            String::from_utf8_lossy(&o.stderr)
        ),
        Err(e) => e.to_string(),
    };
    restore(path, old.as_deref());
    // @Yogesh(word): tmux refused the file; it was put back
    anyhow::bail!(
        "tmux refused {}, so it is back as it was: {}",
        landed.display(),
        why.trim()
    )
}

/// Merge an item into config.toml, check it, and restart the daemon on it.
///
/// A file the check refuses is put back as it was, and the check's words are
/// the error.
pub async fn add_to_config(path: &Path, item: &Item) -> anyhow::Result<String> {
    let old = std::fs::read_to_string(path).ok();
    let seed_text;
    let seed = if item.seed {
        let config = crate::config::load().map(|(c, _)| c).unwrap_or_default();
        seed_text = merge::segments_seed(&config);
        Some((item.setting.as_str(), seed_text.as_str()))
    } else {
        None
    };
    let text = merge::merged(old.as_deref().unwrap_or(""), &item.snippet, seed)?;
    block::write_through(path, &text)?;
    if let Err(e) = crate::config::load_from(path) {
        restore(path, old.as_deref());
        // @Yogesh(word): config check refused the merged file; it was put back
        anyhow::bail!("{e}\nso {} is back as it was", path.display());
    }
    crate::cli::restart_daemon().await
}

// ── The picker ───────────────────────────────────────────────────────────────

/// The ctrl key that skips a row, or puts it back.
pub const SKIP_KEY: char = 'x';
/// The ctrl key that sets a binding's key by hand.
pub const KEY_KEY: char = 'e';

/// Ask a yes-or-no question on the terminal; anything but yes is no.
fn confirm(question: &str) -> bool {
    print!("{question} [y/N] ");
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    let _ = std::io::stdin().read_line(&mut line);
    matches!(line.trim().to_lowercase().as_str(), "y" | "yes")
}

/// Ask for a line on the terminal.
fn ask(question: &str) -> String {
    print!("{question} ");
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    let _ = std::io::stdin().read_line(&mut line);
    line.trim().to_string()
}

/// What a key would replace, for the question asked before taking it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyCheck {
    /// Nothing is bound there.
    Free,
    /// This item is already bound there.
    Same,
    /// Something else is, and whether it is tmux's own binding.
    Replaces {
        /// The command bound there now.
        command: String,
        /// Whether that is one of tmux's own bindings.
        tmux_default: bool,
    },
    /// Not a key tmux could take.
    Invalid,
}

/// Check a key typed for an item against every binding and tmux's defaults.
pub fn check_key(
    item: &Item,
    key: &str,
    bindings: &[detect::Bound],
    defaults: &[detect::Bound],
) -> KeyCheck {
    if key.is_empty() || key.chars().any(char::is_whitespace) {
        return KeyCheck::Invalid;
    }
    let Some(there) = detect::taken(bindings, &item.table, key) else {
        return KeyCheck::Free;
    };
    let parents = detect::parent_commands();
    let distinct = detect::distinguishing(&catalog::items());
    let one = [there.clone()];
    if detect::bound_on(item, &one, &parents, &distinct).is_some() {
        return KeyCheck::Same;
    }
    KeyCheck::Replaces {
        command: there.command.clone(),
        tmux_default: defaults
            .iter()
            .any(|d| d.table == there.table && d.key == there.key && d.command == there.command),
    }
}

/// tmux's own bindings, from a throwaway server started on no config.
///
/// The server has no session, so it exits as soon as `list-keys` has
/// answered. Empty when tmux will not start one, and then every collision is
/// asked about as though it were somebody's own.
pub async fn tmux_defaults() -> Vec<detect::Bound> {
    let socket = format!("tc-setup-defaults-{}", std::process::id());
    let out = tokio::process::Command::new("tmux")
        .args([
            "-L",
            &socket,
            "-f",
            "/dev/null",
            "start-server",
            ";",
            "list-keys",
        ])
        .env_remove("TMUX")
        .output()
        .await;
    match out {
        Ok(o) => detect::listed_bindings(&String::from_utf8_lossy(&o.stdout)),
        Err(_) => Vec::new(),
    }
}

/// Ask for a key for a binding item, and check it before taking it.
async fn choose_key(item: &Item, inputs: &Inputs) -> Option<String> {
    // @Yogesh(word): asking for a key for one item, in its table
    let key = ask(&format!(
        "key for {} in the {} table (enter cancels):",
        item.id, item.table
    ));
    if key.is_empty() {
        return None;
    }
    let defaults = tmux_defaults().await;
    match check_key(item, &key, &inputs.bindings, &defaults) {
        KeyCheck::Free | KeyCheck::Same => Some(key),
        KeyCheck::Invalid => {
            // @Yogesh(word): a key tmux could not take
            println!("`{key}` is not a key tmux can bind");
            None
        }
        KeyCheck::Replaces {
            command,
            tmux_default,
        } => {
            // @Yogesh(word): what binding a chosen key would replace
            println!(
                "{} is {}: {command}",
                key_label(&item.table, &key),
                if tmux_default { "tmux's own" } else { "bound" }
            );
            // @Yogesh(word): confirm replacing it
            confirm("replace it?").then_some(key)
        }
    }
}

/// Copy text to the clipboard, saying why not when it cannot.
async fn copy(text: &str) -> Result<(), String> {
    crate::cli::copy_to_clipboard(text)
        .await
        .map_err(|e| e.to_string())
}

/// The chrome the setup picker is drawn with.
fn chrome(config: &crate::config::Config) -> crate::picker::Chrome {
    crate::picker::Chrome {
        // @Yogesh(word): the setup picker's label on its border
        title: "[ Setup ]".into(),
        // @Yogesh(word): the setup picker's hint line naming its keys
        footer: "enter copies and offers to add   ctrl-x skip   ctrl-e set the key   esc leaves"
            .into(),
        // @Yogesh(word): the label over the setup picker's preview
        preview_title: "[ What it adds ]".into(),
        ..crate::picker::Chrome::default()
    }
    .configured(&config.picker, crate::config::Picker::Setup)
}

/// `setup`: the checklist, or with `--print` its rows.
pub async fn run(print: bool) -> anyhow::Result<()> {
    let items = catalog::items();
    let config = crate::cli::config_or_default();
    let mut chosen: HashMap<String, String> = HashMap::new();
    let mut said: Option<String> = None;
    let mut at: Option<usize> = None;

    loop {
        let inputs = detect::gather(&items).await;
        let states = detect::states(&items, &inputs, &load_skips());
        let keys = keys_for(&items, &inputs, &chosen);
        if print {
            print!("{}", tsv(&items, &states, &keys));
            return Ok(());
        }

        let conf = tmux_conf().await;
        let toml = config_file().ok();
        let rows = order(&states);
        let picker_items: Vec<crate::picker::Item> = rows
            .iter()
            .map(|&i| {
                let item = &items[i];
                let key = keys[i].as_ref().map(|(_, k)| k.as_str());
                let file = match item.target() {
                    Some(Target::TmuxConf) => conf.display().to_string(),
                    Some(Target::Config) => toml
                        .as_ref()
                        .map(|p| p.display().to_string())
                        .unwrap_or_else(|| item.goes.clone()),
                    None => item.goes.clone(),
                };
                let label = keys[i]
                    .as_ref()
                    .map(|(t, k)| key_label(t, k))
                    .unwrap_or_default();
                crate::picker::Item::with_preview(item.id.clone(), preview(item, key, &file))
                    .in_columns(vec![
                        states[i].word().to_string(),
                        item.id.clone(),
                        label,
                        item.line.clone(),
                    ])
                    .in_colour((states[i] == State::Skipped).then(|| "colour244".to_string()))
            })
            .collect();

        let mut look = chrome(&config);
        if let Some(message) = said.take() {
            look.footer = message;
        }
        let start = at.and_then(|i| rows.iter().position(|r| *r == i));
        let picked =
            crate::picker::run_keyed(picker_items, "", &look, &[SKIP_KEY, KEY_KEY], start)?;
        let (row, key_pressed) = match picked {
            crate::picker::Keyed::Cancelled => return Ok(()),
            crate::picker::Keyed::Chosen(r) => (r, None),
            crate::picker::Keyed::Key(c, r) => (r, Some(c)),
        };
        let index = rows[row];
        let item = &items[index];
        at = Some(index);

        match key_pressed {
            Some(SKIP_KEY) => {
                let skips = toggled(load_skips(), &item.id);
                said = Some(match save_skips(&skips) {
                    // @Yogesh(word): a row skipped or put back
                    Ok(()) if skips.contains(&item.id) => format!("{} skipped", item.id),
                    Ok(()) => format!("{} back in the list", item.id),
                    Err(e) => e.to_string(),
                });
            }
            Some(_) if item.kind != Kind::Binding => {
                // @Yogesh(word): ctrl-e on a row that has no key
                said = Some(format!("{} has no key to set", item.id));
            }
            Some(_) => {
                if let Some(k) = choose_key(item, &inputs).await {
                    // @Yogesh(word): a key chosen for a row
                    said = Some(format!(
                        "{} will go on {}",
                        item.id,
                        key_label(&item.table, &k)
                    ));
                    chosen.insert(item.id.clone(), k);
                }
            }
            None if states[index] == State::On => {
                // Writing it again would bind a second key or add a second
                // hook, so an item that is on is copied and left alone.
                let shown = keys[index].as_ref().map(|(_, k)| k.as_str());
                said = Some(match copy(&item.snippet_for(shown)).await {
                    // @Yogesh(word): enter on a row that is already on
                    Ok(()) => format!("{} is already on; copied", item.id),
                    Err(e) => format!("could not copy: {e}"),
                });
            }
            None => {
                said = Some(add(item, &items, &inputs, &mut chosen, &conf, toml.as_deref()).await)
            }
        }
    }
}

/// Enter on a row: copy it, then offer to write it.
async fn add(
    item: &Item,
    catalog: &[Item],
    inputs: &Inputs,
    chosen: &mut HashMap<String, String>,
    conf: &Path,
    toml: Option<&Path>,
) -> String {
    let key = if item.kind == Kind::Binding {
        let offered = chosen
            .get(&item.id)
            .cloned()
            .or_else(|| detect::recommended(item, &inputs.bindings));
        match offered {
            Some(k) => Some(k),
            None => match choose_key(item, inputs).await {
                Some(k) => {
                    chosen.insert(item.id.clone(), k.clone());
                    Some(k)
                }
                // @Yogesh(word): a binding with no free key and none typed
                None => return format!("{} needs a key; ctrl-e sets one", item.id),
            },
        }
    } else {
        None
    };
    let snippet = item.snippet_for(key.as_deref());
    let copied = copy(&snippet).await;

    let Some(target) = item.target() else {
        return match copied {
            // @Yogesh(word): copied an item that is never written, and where it goes
            Ok(()) => format!("copied; it goes in {}", item.goes),
            // @Yogesh(word): the copy failed
            Err(e) => format!("could not copy: {e}"),
        };
    };
    let file = match target {
        Target::TmuxConf => conf.to_path_buf(),
        Target::Config => match toml {
            Some(p) => p.to_path_buf(),
            None => {
                return config_file()
                    .err()
                    .map(|e| e.to_string())
                    .unwrap_or_default();
            }
        },
    };
    if let Err(e) = &copied {
        println!("could not copy: {e}");
    }
    // @Yogesh(word): offering to write the lines into the file as well
    if !confirm(&format!("also add it to {}?", file.display())) {
        return match copied {
            Ok(()) => "copied".to_string(),
            Err(e) => format!("could not copy: {e}"),
        };
    }
    let written = match target {
        Target::TmuxConf => add_to_tmux_conf(&file, item, key.as_deref(), catalog)
            .await
            // @Yogesh(word): added to tmux.conf and sourced
            .map(|landed| format!("added to {} and sourced", landed.display())),
        Target::Config => add_to_config(&file, item)
            .await
            // @Yogesh(word): added to config.toml and the daemon restarted
            .map(|restarted| format!("added to {}; {restarted}", file.display())),
    };
    match written {
        Ok(m) => m,
        Err(e) => {
            // The picker's footer holds one line, and tmux's complaint and
            // the check's can run to several, so the whole of it is printed
            // and waited on before the list comes back.
            println!("{e}");
            // @Yogesh(word): the prompt that holds a long error on screen until enter
            let _ = ask("(enter)");
            e.to_string().lines().next().unwrap_or("").to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_skip_file_round_trips_and_keeps_ids_it_does_not_know() {
        let skips = parse_skips("zoxide\n\n# a comment\nsomething-from-later\nnerd-font\n");
        assert_eq!(skips.len(), 3);
        assert!(skips.contains("something-from-later"));
        let text = render_skips(&skips);
        assert_eq!(text, "nerd-font\nsomething-from-later\nzoxide\n");
        assert_eq!(parse_skips(&text), skips);
        let back = toggled(skips.clone(), "zoxide");
        assert!(!back.contains("zoxide"));
        assert!(toggled(back, "zoxide").contains("zoxide"));
    }

    #[test]
    fn the_stamp_is_written_once_per_build() {
        assert_eq!(
            stamp_text(None, "0.5.2+abc", 100),
            Some("0.5.2+abc\t100\n".into())
        );
        let same = "0.5.2+abc\t100\n";
        assert_eq!(stamp_text(Some(same), "0.5.2+abc", 999), None);
        assert_eq!(
            stamp_text(Some(same), "0.5.3+def", 999),
            Some("0.5.3+def\t999\n".into())
        );
        assert_eq!(stamp_text(Some("garbage"), "b", 5), Some("b\t5\n".into()));
        assert_eq!(first_seen(same, "0.5.2+abc"), Some(100));
        assert_eq!(first_seen(same, "other"), None);
    }

    #[test]
    fn a_build_is_fresh_for_seven_days_and_unknown_is_not_fresh() {
        let day = 86_400;
        assert!(is_fresh(Some(0), 6 * day));
        assert!(!is_fresh(Some(0), 7 * day));
        assert!(!is_fresh(None, 0));
        // A clock behind the stamp is fresh rather than a wrap-around.
        assert!(is_fresh(Some(100), 50));
    }

    #[test]
    fn open_rows_come_first_and_skipped_ones_last() {
        let s = [
            State::On,
            State::Skipped,
            State::Open,
            State::OffByChoice,
            State::CantTell,
            State::Open,
        ];
        assert_eq!(order(&s), vec![2, 5, 4, 0, 3, 1]);
        assert_eq!(open_count(&s), (2, 6));
    }

    #[test]
    fn the_conf_is_the_first_one_under_home() {
        assert_eq!(
            pick_conf(
                "/etc/tmux.conf,/home/me/.tmux.conf,/home/me/.tmux/extra.conf",
                "/home/me"
            ),
            Some(PathBuf::from("/home/me/.tmux.conf"))
        );
        assert_eq!(
            pick_conf("/etc/tmux.conf", "/home/me"),
            Some(PathBuf::from("/etc/tmux.conf"))
        );
        assert_eq!(pick_conf("", "/home/me"), None);
        assert_eq!(pick_conf(" , ", "/home/me"), None);
        // A home that is a prefix of another directory is not that directory.
        assert_eq!(
            pick_conf("/home/meg/.tmux.conf,/home/me/.tmux.conf", "/home/me"),
            Some(PathBuf::from("/home/me/.tmux.conf"))
        );
    }

    #[test]
    fn print_has_five_columns_and_a_key_only_for_bindings() {
        let items = catalog::items();
        let inputs = Inputs {
            tmux_up: true,
            bindings: detect::listed_bindings(
                "bind-key -T prefix g display-popup \"tmux-companion panes\"\n",
            ),
            ..Inputs::default()
        };
        let states = detect::states(&items, &inputs, &HashSet::new());
        let keys = keys_for(&items, &inputs, &HashMap::new());
        let text = tsv(&items, &states, &keys);
        assert_eq!(text.lines().count(), items.len());
        for line in text.lines() {
            assert_eq!(line.split('\t').count(), 5, "{line}");
        }
        let panes = text.lines().find(|l| l.starts_with("panes\t")).unwrap();
        assert_eq!(
            panes,
            format!(
                "panes\tKeys\ton\tprefix g\t{}",
                items.iter().find(|i| i.id == "panes").unwrap().line
            )
        );
        let online = text.lines().find(|l| l.starts_with("online\t")).unwrap();
        assert!(online.contains("\tConfig\t"), "{online}");
        assert_eq!(online.split('\t').nth(3), Some(""));
    }

    #[test]
    fn a_key_chosen_by_hand_wins_over_the_one_offered() {
        let items: Vec<Item> = catalog::items()
            .into_iter()
            .filter(|i| i.id == "inbox")
            .collect();
        let inputs = Inputs {
            tmux_up: true,
            ..Inputs::default()
        };
        let chosen: HashMap<String, String> = [("inbox".to_string(), "F9".to_string())].into();
        assert_eq!(
            keys_for(&items, &inputs, &HashMap::new())[0],
            Some(("prefix".into(), "M-g".into()))
        );
        assert_eq!(
            keys_for(&items, &inputs, &chosen)[0],
            Some(("prefix".into(), "F9".into()))
        );
    }

    #[test]
    fn a_typed_key_says_what_it_would_replace_and_whose_it_is() {
        let items = catalog::items();
        let panes = items.iter().find(|i| i.id == "panes").unwrap();
        let live = detect::listed_bindings(
            "bind-key -T prefix c new-window\n\
             bind-key -T prefix v my-own-thing\n\
             bind-key -T prefix g display-popup \"tmux-companion panes\"\n",
        );
        let defaults = detect::listed_bindings("bind-key -T prefix c new-window\n");
        assert_eq!(check_key(panes, "j", &live, &defaults), KeyCheck::Free);
        assert_eq!(check_key(panes, "g", &live, &defaults), KeyCheck::Same);
        assert_eq!(
            check_key(panes, "c", &live, &defaults),
            KeyCheck::Replaces {
                command: "new-window".into(),
                tmux_default: true
            }
        );
        assert_eq!(
            check_key(panes, "v", &live, &defaults),
            KeyCheck::Replaces {
                command: "my-own-thing".into(),
                tmux_default: false
            }
        );
        assert_eq!(check_key(panes, "", &live, &defaults), KeyCheck::Invalid);
        assert_eq!(check_key(panes, "a b", &live, &defaults), KeyCheck::Invalid);
    }

    #[test]
    fn the_preview_has_the_line_the_lines_and_the_file() {
        let items = catalog::items();
        let panes = items.iter().find(|i| i.id == "panes").unwrap();
        let p = preview(panes, Some("g"), "/home/me/.tmux.conf");
        assert!(p.starts_with(&panes.line), "{p}");
        assert!(p.contains("\" g display-popup"), "{p}");
        assert!(p.trim_end().ends_with("/home/me/.tmux.conf"), "{p}");
    }

    #[tokio::test]
    async fn a_config_the_check_refuses_is_put_back() {
        // The merged file is checked with the same code as `config check`,
        // and one that fails goes back to what it was. `[online] probe`
        // without a port is refused once `enabled` is true.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let before = "# mine\n[online]\nprobe = \"nowhere\"\n";
        std::fs::write(&path, before).unwrap();
        let items = catalog::items();
        let online = items.iter().find(|i| i.id == "online").unwrap();
        let err = add_to_config(&path, online).await.unwrap_err();
        assert!(err.to_string().contains("probe"), "{err}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
    }
}
