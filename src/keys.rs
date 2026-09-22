//! Key bindings, parsed out of `tmux list-keys` into rows a picker can search.
//!
//! This is the data half of `keys.zsh`, the which-key tmux cannot quite do.
//! tmux has no idle hook on a key table, so the popup is a binding of its own
//! rather than something that appears after a pause.
//!
//! Two listings per table, joined on the key, because each one knows half of
//! what a row needs: `list-keys -N` has the note and `list-keys` has the
//! command. `list-keys <key>` is deliberately not used to look a command up
//! afterwards, since it answers for some keys and not others, which cost a
//! debugging round the first time.

use std::collections::HashMap;

/// The tables worth listing.
///
/// `copy-mode-vi` and `copy-mode-emacs` are absent on purpose. Their `-N`
/// listing answers with nothing from a normal client and with one `g` note
/// under `run-shell`, and tmux then prints that single line to the client's
/// message area, which was the flash on every config reload. The `my-keys`
/// rows already read `copy-mode  g <key>`, so nothing is lost.
pub const TABLES: [&str; 3] = ["prefix", "root", "my-keys"];

/// One binding, with everything the picker shows and runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyRow {
    /// Which table it is bound in.
    pub table: String,
    /// The key itself, as tmux spells it.
    pub key: String,
    /// How the chord is written for a reader: `prefix ?`, `M-s`, and so on.
    pub shown: String,
    /// The note from `list-keys -N`.
    pub note: String,
    /// The command `run-shell -C` would run.
    pub command: String,
}

/// Parse a `list-keys -N -T <table>` listing into key-to-note.
///
/// Two shapes come back. The prefix table is padded into columns, so the note
/// starts after a run of two or more spaces and the key is the last word of the
/// chord before it. Every other table is single-spaced, where the key is the
/// second field and the note is everything from the third on.
pub fn parse_notes(listing: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for line in listing.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if let Some(pos) = find_double_space(line) {
            let chord = &line[..pos];
            let note = line[pos..].trim_start();
            if let Some(key) = chord.split_whitespace().next_back()
                && !note.is_empty()
            {
                out.insert(key.to_string(), note.to_string());
            }
            continue;
        }
        let mut fields = line.split_whitespace();
        let (_, key) = (fields.next(), fields.next());
        let note: Vec<&str> = fields.collect();
        if let Some(key) = key
            && !note.is_empty()
        {
            out.insert(key.to_string(), note.join(" "));
        }
    }
    out
}

/// The byte offset of the first run of two or more spaces, if there is one.
fn find_double_space(line: &str) -> Option<usize> {
    line.as_bytes()
        .windows(2)
        .position(|w| w == b"  ")
        .map(|i| i + 1)
}

/// Parse a `list-keys -T <table>` listing into key-to-command, keeping only the
/// rows for `table`.
///
/// The listing is `bind-key [-r] -T <table> <key> <command...>`, and the `-T`
/// is found rather than assumed to be second, because the repeat flag moves it.
pub fn parse_commands(listing: &str, table: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for line in listing.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        let Some(t) = fields.iter().position(|f| *f == "-T") else {
            continue;
        };
        if fields.get(t + 1) != Some(&table) {
            continue;
        }
        let Some(key) = fields.get(t + 2) else {
            continue;
        };
        let command = fields[t + 3..].join(" ");
        if command.is_empty() {
            continue;
        }
        out.push((key.to_string(), command));
    }
    out
}

/// How a chord is written for a reader.
pub fn shown_for(table: &str, key: &str) -> String {
    match table {
        "prefix" => format!("prefix {key}"),
        "root" => key.to_string(),
        "my-keys" => format!("copy-mode  g {key}"),
        _ => format!("copy-mode  {key}"),
    }
}

/// Join one table's two listings into rows.
///
/// A key with a command and no note is dropped: tmux ships notes for about a
/// hundred of its own defaults and a row nobody described is a row nobody can
/// search for.
pub fn rows_for_table(table: &str, notes: &str, commands: &str) -> Vec<KeyRow> {
    let notes = parse_notes(notes);
    parse_commands(commands, table)
        .into_iter()
        .filter_map(|(key, command)| {
            let note = notes.get(&key)?.clone();
            Some(KeyRow {
                shown: shown_for(table, &key),
                table: table.to_string(),
                key,
                note,
                command,
            })
        })
        .collect()
}

/// Sort by note and drop the second row for a table and key.
///
/// Sorting by the note rather than the key is what makes the list read as a
/// list of things you can do rather than a list of keystrokes.
pub fn sort_and_dedupe(mut rows: Vec<KeyRow>) -> Vec<KeyRow> {
    rows.sort_by(|a, b| a.note.cmp(&b.note));
    let mut seen = std::collections::HashSet::new();
    rows.retain(|r| seen.insert((r.table.clone(), r.key.clone())));
    rows
}

/// The rows a query selects, by substring over the note and the chord.
///
/// The default query is `custom: ` because every binding written in tmux.conf
/// carries a note starting that way, and tmux's hundred noted defaults would
/// otherwise bury the thirty-six that are somebody's own.
pub fn filter<'a>(rows: &'a [KeyRow], query: &str) -> Vec<&'a KeyRow> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return rows.iter().collect();
    }
    rows.iter()
        .filter(|r| r.note.to_lowercase().contains(&q) || r.shown.to_lowercase().contains(&q))
        .collect()
}

// ── Collecting from tmux ─────────────────────────────────────────────────────

/// Ask tmux for every table's bindings.
///
/// Six `tmux list-keys` calls, two per table, which cost about 70 ms of process
/// spawn every time the popup opened in the zsh version. They happen once per
/// config change now rather than once per keypress, which is the whole reason
/// the daemon holds these.
pub async fn collect() -> anyhow::Result<Vec<KeyRow>> {
    let mut all = Vec::new();
    for table in TABLES {
        let notes = list_keys(&["-N", "-T", table]).await;
        let commands = list_keys(&["-T", table]).await;
        all.extend(rows_for_table(table, &notes, &commands));
    }
    Ok(sort_and_dedupe(all))
}

/// One `tmux list-keys` call, empty on failure.
///
/// A table tmux does not know is not an error worth failing the whole listing
/// for: the answer is that it contributes no rows.
async fn list_keys(args: &[&str]) -> String {
    let out = tokio::process::Command::new("tmux")
        .arg("list-keys")
        .args(args)
        .output()
        .await;
    match out {
        Ok(o) => String::from_utf8_lossy(&o.stdout).into_owned(),
        Err(_) => String::new(),
    }
}

/// When the tmux config was last written, for deciding whether rows are stale.
///
/// The zsh version compared the config's mtime against a cache file's. The
/// daemon holds the rows in memory instead, so this is compared against the
/// mtime recorded when they were built, and the cache file, the `--build` flag
/// and the `--refresh` flag all go away with it.
pub fn config_mtime(path: &std::path::Path) -> Option<std::time::SystemTime> {
    std::fs::metadata(path).ok()?.modified().ok()
}

/// The tmux config this daemon watches.
pub fn tmux_conf_path() -> std::path::PathBuf {
    if let Some(p) = std::env::var_os("TMUX_COMPANION_TMUX_CONF") {
        return std::path::PathBuf::from(p);
    }
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
    match home {
        Some(h) => h.join(".config/tmux/tmux.conf"),
        None => std::path::PathBuf::from("tmux.conf"),
    }
}

/// Append a pick to the usage log.
///
/// The cheat sheet orders each box by this, so the keys somebody actually
/// reaches for float to the top of their group. Failure is ignored on purpose:
/// a status bar that stops working because a log file could not be written
/// would be a poor trade.
pub fn record_use(path: &std::path::Path, table: &str, key: &str) {
    use std::io::Write;

    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(f, "{table}\t{key}");
    }
}

/// How many times each binding has been picked.
pub fn usage_counts(log: &str) -> HashMap<(String, String), usize> {
    let mut out: HashMap<(String, String), usize> = HashMap::new();
    for line in log.lines() {
        if let Some((table, key)) = line.split_once('\t') {
            *out.entry((table.to_string(), key.to_string())).or_default() += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Real `list-keys -N -T prefix` output: column-padded.
    const NOTES_PREFIX: &str = "\
C-b Space   Select next layout
C-b !       Break pane to a new window
C-b \"       Split window vertically
C-b ?       custom: search key bindings
";

    /// Real `list-keys -T prefix` output.
    const CMDS_PREFIX: &str = "\
bind-key    -T prefix Space   next-layout
bind-key    -T prefix !       break-pane
bind-key    -T prefix \"      split-window
bind-key -r -T prefix ?       run-shell -b keys.zsh
bind-key    -T prefix M-x     display-popup -E something
";

    #[test]
    fn a_padded_note_listing_gives_the_last_word_of_the_chord() {
        let notes = parse_notes(NOTES_PREFIX);
        assert_eq!(
            notes.get("Space").map(String::as_str),
            Some("Select next layout")
        );
        assert_eq!(
            notes.get("?").map(String::as_str),
            Some("custom: search key bindings")
        );
    }

    #[test]
    fn a_single_spaced_note_listing_takes_the_second_field_as_the_key() {
        // What every table other than prefix answers with.
        let notes = parse_notes("C-b M-s Choose a session\nC-b q Display pane numbers\n");
        assert_eq!(
            notes.get("M-s").map(String::as_str),
            Some("Choose a session")
        );
        assert_eq!(
            notes.get("q").map(String::as_str),
            Some("Display pane numbers")
        );
    }

    #[test]
    fn a_note_listing_line_without_a_note_is_skipped() {
        let notes = parse_notes("C-b x\n\n");
        assert!(notes.is_empty(), "{notes:?}");
    }

    #[test]
    fn commands_are_found_whatever_position_the_table_flag_is_in() {
        // `-r` shifts everything right, which is why -T is searched for.
        let cmds = parse_commands(CMDS_PREFIX, "prefix");
        let by_key: HashMap<_, _> = cmds.into_iter().collect();
        assert_eq!(by_key.get("Space").map(String::as_str), Some("next-layout"));
        assert_eq!(
            by_key.get("?").map(String::as_str),
            Some("run-shell -b keys.zsh")
        );
    }

    #[test]
    fn commands_for_another_table_are_left_out() {
        let listing = "bind-key -T root MouseDown1Pane select-pane\n\
                       bind-key -T prefix c new-window\n";
        assert_eq!(parse_commands(listing, "root").len(), 1);
        assert_eq!(parse_commands(listing, "prefix").len(), 1);
    }

    #[test]
    fn a_binding_with_no_note_does_not_become_a_row() {
        // M-x has a command in the fixture and no note.
        let rows = rows_for_table("prefix", NOTES_PREFIX, CMDS_PREFIX);
        assert!(rows.iter().all(|r| r.key != "M-x"), "{rows:?}");
        assert_eq!(rows.len(), 4);
    }

    #[test]
    fn a_row_carries_the_command_so_nothing_has_to_be_looked_up_later() {
        let rows = rows_for_table("prefix", NOTES_PREFIX, CMDS_PREFIX);
        let row = rows
            .iter()
            .find(|r| r.key == "?")
            .expect("the keys binding");
        assert_eq!(row.command, "run-shell -b keys.zsh");
        assert_eq!(row.shown, "prefix ?");
        assert_eq!(row.note, "custom: search key bindings");
    }

    #[test]
    fn each_table_writes_its_chord_the_way_a_reader_reads_it() {
        assert_eq!(shown_for("prefix", "?"), "prefix ?");
        assert_eq!(shown_for("root", "M-s"), "M-s");
        assert_eq!(shown_for("my-keys", "u"), "copy-mode  g u");
        assert_eq!(shown_for("copy-mode-vi", "v"), "copy-mode  v");
    }

    fn row(table: &str, key: &str, note: &str) -> KeyRow {
        KeyRow {
            table: table.into(),
            key: key.into(),
            shown: shown_for(table, key),
            note: note.into(),
            command: "cmd".into(),
        }
    }

    #[test]
    fn rows_are_ordered_by_what_they_do_not_by_which_key() {
        let rows = sort_and_dedupe(vec![
            row("prefix", "z", "custom: zoom"),
            row("prefix", "a", "custom: attach"),
        ]);
        assert_eq!(rows[0].note, "custom: attach");
    }

    #[test]
    fn the_same_key_in_the_same_table_appears_once() {
        let rows = sort_and_dedupe(vec![
            row("prefix", "a", "custom: attach"),
            row("prefix", "a", "custom: attach again"),
        ]);
        assert_eq!(rows.len(), 1);
    }

    #[test]
    fn the_same_key_in_two_tables_is_two_rows() {
        let rows = sort_and_dedupe(vec![
            row("prefix", "a", "custom: one"),
            row("root", "a", "custom: two"),
        ]);
        assert_eq!(rows.len(), 2);
    }

    #[test]
    fn the_default_query_keeps_only_the_bindings_somebody_wrote() {
        let rows = vec![
            row("prefix", "a", "custom: attach"),
            row("prefix", "b", "Break pane to a new window"),
        ];
        let found = filter(&rows, "custom: ");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].key, "a");
    }

    #[test]
    fn an_empty_query_shows_tmuxs_own_bindings_too() {
        let rows = vec![
            row("prefix", "a", "custom: attach"),
            row("prefix", "b", "Break pane to a new window"),
        ];
        assert_eq!(filter(&rows, "").len(), 2);
    }

    #[test]
    fn usage_counts_add_up_per_table_and_key() {
        let counts = usage_counts("prefix\t?\nprefix\t?\nroot\tM-s\n");
        assert_eq!(counts.get(&("prefix".into(), "?".into())), Some(&2));
        assert_eq!(counts.get(&("root".into(), "M-s".into())), Some(&1));
        assert_eq!(counts.len(), 2);
    }

    #[test]
    fn a_malformed_usage_line_is_ignored_rather_than_counted() {
        let counts = usage_counts("prefix\t?\ngarbage\n\n");
        assert_eq!(counts.len(), 1);
    }

    #[test]
    fn recording_a_use_appends_rather_than_replacing() {
        let dir = std::env::temp_dir().join(format!("tc-usage-{}", std::process::id()));
        let path = dir.join("keys-usage.tsv");
        let _ = std::fs::remove_dir_all(&dir);

        record_use(&path, "prefix", "?");
        record_use(&path, "prefix", "?");
        record_use(&path, "root", "M-s");

        let text = std::fs::read_to_string(&path).expect("log written");
        let counts = usage_counts(&text);
        assert_eq!(counts.get(&("prefix".into(), "?".into())), Some(&2));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn recording_into_an_unwritable_path_is_silent() {
        // A status bar that stops working because a log file could not be
        // written would be a poor trade.
        record_use(std::path::Path::new("/proc/nonexistent/keys.tsv"), "a", "b");
    }

    #[test]
    fn the_config_path_can_be_pointed_somewhere_else() {
        // SAFETY: single-threaded test that restores what it changed.
        let before = std::env::var_os("TMUX_COMPANION_TMUX_CONF");
        unsafe { std::env::set_var("TMUX_COMPANION_TMUX_CONF", "/tmp/somewhere.conf") };
        assert_eq!(
            tmux_conf_path(),
            std::path::PathBuf::from("/tmp/somewhere.conf")
        );
        match before {
            Some(v) => unsafe { std::env::set_var("TMUX_COMPANION_TMUX_CONF", v) },
            None => unsafe { std::env::remove_var("TMUX_COMPANION_TMUX_CONF") },
        }
    }

    #[test]
    fn a_query_matches_the_chord_as_well_as_the_note() {
        let rows = vec![row("root", "M-s", "Choose a session")];
        assert_eq!(filter(&rows, "M-s").len(), 1);
        assert_eq!(filter(&rows, "m-s").len(), 1, "case should not matter");
    }
}
