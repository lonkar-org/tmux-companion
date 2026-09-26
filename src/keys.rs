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
/// `copy-mode-vi` was left out on the grounds that its `-N` listing answered
/// with nothing useful and made tmux flash a line in the message area. That is
/// not what tmux 3.7 does: it answers with exactly the notes somebody wrote,
/// and leaving it out meant every `-T copy-mode-vi` binding in the shipped
/// example config -- the prompt jumps, `o` to open the selection, `y` to yank
/// -- was missing from the key search and left the cheat sheet's copy-mode box
/// empty.
///
/// `copy-mode-emacs` stays out: a config binds one or the other, and listing
/// both would show every copy binding twice.
pub const TABLES: [&str; 4] = ["prefix", "root", "my-keys", "copy-mode-vi"];

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
        out.push((unescape_key(key), command));
    }
    out
}

/// The key as `list-keys -N` writes it.
///
/// `list-keys` escapes the nine characters that would otherwise be ambiguous
/// in a command it is printing -- `\;` `\'` `\"` `\{` `\}` `\#` `\%` `\~`
/// `\$` -- and `list-keys -N` does not. A row is kept only when its key is
/// found in the notes, so without this every one of those nine was silently
/// dropped: `prefix %` and `prefix "`, the two split bindings tmux ships,
/// were missing from the key search and from the cheat sheet.
fn unescape_key(key: &str) -> String {
    match key.strip_prefix('\\') {
        Some(rest) if rest.chars().count() == 1 => rest.to_string(),
        _ => key.to_string(),
    }
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
///
/// `TMUX_COMPANION_TMUX_CONF` when it is set, and otherwise the file tmux
/// itself would read, found the way [`tmux_conf_in`] finds it.
pub fn tmux_conf_path() -> std::path::PathBuf {
    if let Some(p) = std::env::var_os("TMUX_COMPANION_TMUX_CONF") {
        return std::path::PathBuf::from(p);
    }
    let home = std::env::var("HOME").unwrap_or_default();
    let xdg = std::env::var("XDG_CONFIG_HOME").ok();
    tmux_conf_in(&home, xdg.as_deref())
}

/// The tmux config on this machine, in the order tmux looks for it.
///
/// tmux reads `$XDG_CONFIG_HOME/tmux/tmux.conf` before `~/.tmux.conf`, so
/// the first of those that exists is the one being edited. Until this
/// looked, somebody on the classic dotfile had the daemon watching a path
/// that was never written: the key rows never went stale and `[autoreload]`
/// watched nothing. With neither file present the XDG path is returned, so a
/// config written later is picked up where tmux would look first.
pub fn tmux_conf_in(home: &str, xdg_config: Option<&str>) -> std::path::PathBuf {
    let xdg = match xdg_config {
        Some(x) if !x.is_empty() => std::path::PathBuf::from(x),
        _ => std::path::PathBuf::from(home).join(".config"),
    };
    let under_xdg = xdg.join("tmux/tmux.conf");
    if under_xdg.is_file() {
        return under_xdg;
    }
    let classic = std::path::PathBuf::from(home).join(".tmux.conf");
    if classic.is_file() {
        return classic;
    }
    under_xdg
}

/// Lines past which the usage log is rewritten as one line per binding.
///
/// Every pick appends a line and nothing removed one, so a log grew for as
/// long as the machine lasted. Five thousand is months of picks, read in
/// under a millisecond, and a rewrite that often costs nothing anybody sees.
pub const COMPACT_AFTER: usize = 5000;

/// The first line of a compacted log, which is how a reader knows the third
/// column is a count.
pub const COMPACT_HEADER: &str = "#v1";

/// Append a pick to the usage log, rewriting it compactly once it is long.
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
    // Read back after the append, because the append is what may have taken
    // it over the line.
    if let Ok(text) = std::fs::read_to_string(path)
        && text.lines().count() > COMPACT_AFTER
    {
        let _ = crate::saved::write_atomically(path, &compact(&text));
    }
}

/// The log rewritten as one `table\tkey\tcount` line per binding, under
/// [`COMPACT_HEADER`].
///
/// Sorted, so two compactions of the same picks write the same bytes and a
/// diff of the file says something.
pub fn compact(log: &str) -> String {
    let mut rows: Vec<((String, String), usize)> = usage_counts(log).into_iter().collect();
    rows.sort();
    let mut out = format!("{COMPACT_HEADER}\n");
    for ((table, key), count) in rows {
        out.push_str(&format!("{table}\t{key}\t{count}\n"));
    }
    out
}

/// How many times each binding has been picked.
///
/// Reads both shapes of the file: the raw log, one `table\tkey` line per
/// pick, and the compacted one, where a third column carries the count.
/// Picks appended after a compaction have no third column and count as one,
/// which is what lets [`record_use`] keep appending to a compacted file.
pub fn usage_counts(log: &str) -> HashMap<(String, String), usize> {
    let compacted = log.lines().next() == Some(COMPACT_HEADER);
    let mut out: HashMap<(String, String), usize> = HashMap::new();
    for line in log.lines() {
        if line.starts_with('#') {
            continue;
        }
        let Some((table, rest)) = line.split_once('\t') else {
            continue;
        };
        // Only a compacted file has counts, and only a last column that reads
        // as a number is one; anything else is part of the key.
        let (key, count) = match rest.rsplit_once('\t') {
            Some((key, n)) if compacted => match n.parse::<usize>() {
                Ok(n) => (key, n),
                Err(_) => (rest, 1),
            },
            _ => (rest, 1),
        };
        *out.entry((table.to_string(), key.to_string())).or_default() += count;
    }
    out
}

/// The rows the log has never recorded a press for, in the order given.
///
/// The match is on the table and key exactly as `record_use` wrote them, which
/// is the row's own spelling with nothing folded: a log line saying `C-A` is
/// not a press of `C-a`, and `^a` is not one either. The log never held those
/// spellings, because nothing writes to it but a pick of a row, so treating
/// them as the same binding would only hide a row that was never pressed.
///
/// The log carries no dates, so "never" means no press since the log began,
/// and the caller says so next to the count.
pub fn unused<'a>(
    rows: impl IntoIterator<Item = &'a KeyRow>,
    counts: &HashMap<(String, String), usize>,
) -> Vec<&'a KeyRow> {
    rows.into_iter()
        .filter(|r| {
            counts
                .get(&(r.table.clone(), r.key.clone()))
                .copied()
                .unwrap_or(0)
                == 0
        })
        .collect()
}

/// The line that says what an unused listing is a listing of.
///
/// `presses` is the log's total, or `None` when there is no log to count,
/// which is worth saying in words: a list of every binding under a heading
/// that says "never used" is the truth on a fresh machine, and looks like a
/// bug unless the line beneath it explains why.
pub fn unused_summary(never: usize, total: usize, presses: Option<usize>) -> String {
    let log = match presses {
        Some(n) => format!("the log holds {n} press{}", if n == 1 { "" } else { "es" }),
        None => "no usage log yet".to_string(),
    };
    format!("{never} of {total} bindings never pressed; {log}")
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
    fn unused_keeps_the_rows_with_no_press_in_the_order_given() {
        let rows = vec![
            row("prefix", "z", "custom: zoom"),
            row("prefix", "a", "custom: attach"),
            row("root", "M-s", "custom: sessions"),
        ];
        let counts = usage_counts("prefix\ta\nprefix\ta\n");
        let never: Vec<&str> = unused(&rows, &counts)
            .iter()
            .map(|r| r.key.as_str())
            .collect();
        assert_eq!(never, vec!["z", "M-s"]);
    }

    #[test]
    fn unused_with_no_log_is_every_row() {
        let rows = vec![row("prefix", "a", "custom: attach")];
        assert_eq!(unused(&rows, &HashMap::new()).len(), 1);
    }

    #[test]
    fn a_press_in_another_table_does_not_count() {
        // The same key bound in two tables is two bindings, and the log says
        // which one was picked.
        let rows = vec![
            row("prefix", "a", "custom: one"),
            row("root", "a", "custom: two"),
        ];
        let counts = usage_counts("root\ta\n");
        let never = unused(&rows, &counts);
        assert_eq!(never.len(), 1);
        assert_eq!(never[0].table, "prefix");
    }

    #[test]
    fn the_log_is_matched_on_the_rows_own_spelling_and_nothing_looser() {
        // `record_use` writes the row's key verbatim, so the log has never
        // held `C-A` or `^a` for a row spelled `C-a`. A line like that is not
        // evidence the binding was pressed, and folding it in would hide the
        // one row this listing exists to show.
        let rows = vec![row("root", "C-a", "custom: last window")];
        for spelling in ["C-A", "^a", "c-a"] {
            let counts = usage_counts(&format!("root\t{spelling}\n"));
            assert_eq!(
                unused(&rows, &counts).len(),
                1,
                "{spelling} counted as a press"
            );
        }
        let counts = usage_counts("root\tC-a\n");
        assert!(unused(&rows, &counts).is_empty());
    }

    #[test]
    fn the_summary_says_how_much_evidence_there_is() {
        assert_eq!(
            unused_summary(3, 36, Some(120)),
            "3 of 36 bindings never pressed; the log holds 120 presses"
        );
        assert_eq!(
            unused_summary(35, 36, Some(1)),
            "35 of 36 bindings never pressed; the log holds 1 press"
        );
        assert_eq!(
            unused_summary(36, 36, None),
            "36 of 36 bindings never pressed; no usage log yet"
        );
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
    fn a_compacted_log_counts_the_same_as_the_raw_one_it_replaced() {
        let raw = "prefix\t?\nprefix\t?\nroot\tM-s\nprefix\tSpace\n";
        let text = compact(raw);
        assert!(text.starts_with("#v1\n"), "{text}");
        assert_eq!(text.lines().count(), 4, "{text}");
        assert!(text.contains("prefix\t?\t2\n"), "{text}");
        assert_eq!(usage_counts(&text), usage_counts(raw));
        // Compacting a compacted log changes nothing.
        assert_eq!(compact(&text), text);
    }

    #[test]
    fn picks_appended_after_a_compaction_still_count() {
        // `record_use` keeps appending two-column lines to a compacted file,
        // so a reader has to take a count where there is one and a one where
        // there is not.
        let text = "#v1\nprefix\t?\t7\nroot\tM-s\t1\nprefix\t?\nroot\tM-x\n";
        let counts = usage_counts(text);
        assert_eq!(counts.get(&("prefix".into(), "?".into())), Some(&8));
        assert_eq!(counts.get(&("root".into(), "M-s".into())), Some(&1));
        assert_eq!(counts.get(&("root".into(), "M-x".into())), Some(&1));
    }

    #[test]
    fn a_raw_log_never_reads_its_last_column_as_a_count() {
        // Without the header a third column is part of the key, however
        // numeric it looks.
        let counts = usage_counts("prefix\t9\t9\n");
        assert_eq!(counts.get(&("prefix".into(), "9\t9".into())), Some(&1));
    }

    #[test]
    fn a_log_past_the_limit_is_rewritten_with_its_counts_kept() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("keys-usage.tsv");
        let mut long = String::new();
        for _ in 0..COMPACT_AFTER {
            long.push_str("prefix\t?\n");
        }
        std::fs::write(&path, &long).expect("seed");

        record_use(&path, "root", "M-s");
        let text = std::fs::read_to_string(&path).expect("read");
        assert!(
            text.starts_with("#v1\n"),
            "{}",
            text.lines().next().unwrap_or("")
        );
        assert_eq!(text.lines().count(), 3, "{text}");
        let counts = usage_counts(&text);
        assert_eq!(
            counts.get(&("prefix".into(), "?".into())),
            Some(&COMPACT_AFTER)
        );
        assert_eq!(counts.get(&("root".into(), "M-s".into())), Some(&1));

        // And the next pick appends to the compacted file rather than
        // compacting again.
        record_use(&path, "root", "M-s");
        let text = std::fs::read_to_string(&path).expect("read");
        assert_eq!(text.lines().count(), 4, "{text}");
        assert_eq!(
            usage_counts(&text).get(&("root".into(), "M-s".into())),
            Some(&2)
        );
    }

    #[test]
    fn the_tmux_config_is_found_where_tmux_looks_for_it() {
        let t = tempfile::tempdir().expect("tempdir");
        let home = t.path().display().to_string();

        // Neither file: the XDG path, where a config written later will be.
        assert_eq!(
            tmux_conf_in(&home, None),
            t.path().join(".config/tmux/tmux.conf")
        );

        // Only the classic dotfile: that one. This was the case that watched
        // a path nothing ever wrote to.
        std::fs::write(t.path().join(".tmux.conf"), "").expect("write");
        assert_eq!(tmux_conf_in(&home, None), t.path().join(".tmux.conf"));

        // Both: XDG wins, as it does in tmux.
        std::fs::create_dir_all(t.path().join(".config/tmux")).expect("dir");
        std::fs::write(t.path().join(".config/tmux/tmux.conf"), "").expect("write");
        assert_eq!(
            tmux_conf_in(&home, None),
            t.path().join(".config/tmux/tmux.conf")
        );

        // An XDG_CONFIG_HOME somewhere else is looked in first.
        let elsewhere = t.path().join("cfg");
        std::fs::create_dir_all(elsewhere.join("tmux")).expect("dir");
        std::fs::write(elsewhere.join("tmux/tmux.conf"), "").expect("write");
        assert_eq!(
            tmux_conf_in(&home, Some(&elsewhere.display().to_string())),
            elsewhere.join("tmux/tmux.conf")
        );
        // An empty one is the same as none.
        assert_eq!(
            tmux_conf_in(&home, Some("")),
            t.path().join(".config/tmux/tmux.conf")
        );
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

#[cfg(test)]
mod escaped_key_tests {
    use super::*;

    #[test]
    fn the_keys_list_keys_escapes_still_find_their_notes() {
        // `list-keys` escapes these nine; `list-keys -N` does not. Joining the
        // two listings by key therefore dropped every one of them, including
        // both of tmux's own split bindings.
        let commands = concat!(
            "bind-key    -T prefix \\%      split-window -h\n",
            "bind-key    -T prefix \\\"      split-window -v\n",
            "bind-key    -T prefix c       new-window\n",
        );
        let notes = concat!(
            "C-b %       Split window horizontally\n",
            "C-b \"       Split window vertically\n",
            "C-b c       Create a new window\n",
        );
        let rows = rows_for_table("prefix", notes, commands);
        let keys: Vec<&str> = rows.iter().map(|r| r.key.as_str()).collect();
        assert!(keys.contains(&"%"), "no % binding: {keys:?}");
        assert!(keys.contains(&"\""), "no \" binding: {keys:?}");
        assert_eq!(rows.len(), 3, "{keys:?}");
    }

    #[test]
    fn a_backslash_that_is_the_key_itself_is_left_alone() {
        assert_eq!(unescape_key("\\%"), "%");
        assert_eq!(unescape_key("C-b"), "C-b");
        // A two-character escape is a key; anything longer is not one.
        assert_eq!(unescape_key("\\abc"), "\\abc");
    }
}
