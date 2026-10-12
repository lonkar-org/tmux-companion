//! Where the project picker's directory list comes from.
//!
//! zoxide is the default and was once the assumption.  It is not the only
//! directory jumper people run, and a good number run none at all, so the list
//! is a named source rather than one hardcoded command.  Four are built in
//! because they need a parser or a fixed argument list, and anything else that
//! prints one absolute path per line goes in `dirs_command` without this module
//! learning about it.
//!
//! The split is not arbitrary.  A jumper is either a binary that can be
//! executed (zoxide, ghq) or a shell function that cannot (`z`, `wd`,
//! bashmarks, enhancd) — spawning `zsh -ic 'z -l'` to reach the latter sources
//! a whole interactive rc for one list, and prints whatever that rc prints into
//! the middle of it.  The shell-function ones are reached through the data file
//! they keep instead, which is the part that is actually stable.

/// A named source of directories.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DirsSource {
    /// `zoxide query -l`, most frecent first.
    Zoxide,
    /// rupa/z, zsh-z and the other ports: the `~/.z` database.
    Z,
    /// zsh's own `cdr`: the `~/.chpwd-recent-dirs` file.
    Cdr,
    /// `ghq list -p`, every cloned repository.
    Ghq,
    /// A command that prints one absolute path per line.
    Command(Vec<String>),
    /// Live sessions and whatever gets typed, nothing else.
    None,
}

/// The names `dirs_source` accepts, for the config error and the manual.
pub const NAMES: [&str; 5] = ["zoxide", "z", "cdr", "ghq", "none"];

impl DirsSource {
    /// A `dirs_source` name, or `None` when it is not one of [`NAMES`].
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "zoxide" => Some(Self::Zoxide),
            "z" => Some(Self::Z),
            "cdr" => Some(Self::Cdr),
            "ghq" => Some(Self::Ghq),
            "none" => Some(Self::None),
            _ => None,
        }
    }

    /// What the config asks for, with the precedence settled in one place.
    ///
    /// `dirs_command` wins when it is set, because a command spelled out in
    /// full is unambiguous in a way that a name plus a command is not.  The
    /// deprecated `zoxide = false` still means "no directory list" so an
    /// existing config keeps behaving as it did.
    pub fn from_config(project: &crate::config::Project) -> Self {
        if !project.zoxide {
            return Self::None;
        }
        if !project.dirs_command.is_empty() {
            return Self::Command(project.dirs_command.clone());
        }
        Self::from_name(&project.dirs_source).unwrap_or(Self::Zoxide)
    }

    /// The command this source runs, or `None` when it reads a file instead.
    fn command(&self) -> Option<Vec<String>> {
        match self {
            Self::Zoxide => Some(vec!["zoxide".into(), "query".into(), "-l".into()]),
            Self::Ghq => Some(vec!["ghq".into(), "list".into(), "-p".into()]),
            Self::Command(c) => Some(c.clone()),
            Self::Z | Self::Cdr | Self::None => None,
        }
    }

    /// Where this source keeps its database, when it keeps one.
    ///
    /// `z` honours `$_Z_DATA` and `cdr` lives beside the rest of zsh's state,
    /// so both are worth reading from the environment rather than assuming a
    /// home directory that the user may have moved.
    fn data_file(&self, home: &str) -> Option<std::path::PathBuf> {
        match self {
            Self::Z => Some(match std::env::var_os("_Z_DATA") {
                Some(p) => std::path::PathBuf::from(p),
                None => std::path::PathBuf::from(home).join(".z"),
            }),
            Self::Cdr => {
                let dir = std::env::var_os("ZDOTDIR")
                    .map(std::path::PathBuf::from)
                    .unwrap_or_else(|| std::path::PathBuf::from(home));
                Some(dir.join(".chpwd-recent-dirs"))
            }
            _ => None,
        }
    }

    /// The file whose modification time says this source's answer changed,
    /// for a source that runs a command and keeps a database.
    ///
    /// zoxide writes `db.zo` on every `cd` it is told about, under
    /// `$_ZO_DATA_DIR` or the platform's data directory, which is what its own
    /// `dirs::data_local_dir()` resolves to: `~/Library/Application Support`
    /// on macOS, `$XDG_DATA_HOME` or `~/.local/share` elsewhere.
    pub fn watched_file(&self, home: &str) -> Option<std::path::PathBuf> {
        if *self != Self::Zoxide {
            return None;
        }
        let dir = match std::env::var_os("_ZO_DATA_DIR") {
            Some(d) => std::path::PathBuf::from(d),
            None if cfg!(target_os = "macos") => {
                std::path::PathBuf::from(home).join("Library/Application Support/zoxide")
            }
            None => match std::env::var_os("XDG_DATA_HOME") {
                Some(d) => std::path::PathBuf::from(d).join("zoxide"),
                None => std::path::PathBuf::from(home).join(".local/share/zoxide"),
            },
        };
        Some(dir.join("db.zo"))
    }

    /// The directories this source knows, most relevant first, asking the
    /// daemon first for a source that runs a command.
    ///
    /// The daemon keeps the answer until the source's database changes, or
    /// for [`COMMAND_TTL`] when there is no database to watch, so a picker
    /// opened twice without a `cd` between starts no zoxide the second time.
    /// A daemon that can't be asked, or says no, leaves it to this process.
    pub async fn list(&self, home: &str) -> Vec<String> {
        let Some(command) = self.command() else {
            return self.list_here(home).await;
        };
        let args = crate::proto::DirsArgs {
            command,
            env: SOURCE_ENV
                .iter()
                .filter_map(|k| std::env::var(k).ok().map(|v| (k.to_string(), v)))
                .collect(),
            watch: self.watched_file(home).map(|p| p.display().to_string()),
        };
        let req = crate::proto::Request::build("dirs", &args);
        if let Ok(resp) = crate::client::send(req).await
            && resp.error.is_none()
        {
            return parse_lines(&resp.output);
        }
        self.list_here(home).await
    }

    /// [`DirsSource::list`] in this process, without the daemon.
    ///
    /// Every failure is the empty list: a jumper that is not installed, a
    /// database that has never been written, a command that exits non-zero.
    /// The picker degrades to live sessions and a typed path, which is a
    /// smaller tool and still a working one, and that is better than an error
    /// in a popup somebody opened to switch projects.
    pub async fn list_here(&self, home: &str) -> Vec<String> {
        if let Some(path) = self.data_file(home) {
            let text = std::fs::read_to_string(path).unwrap_or_default();
            return match self {
                Self::Z => parse_z(&text),
                _ => parse_cdr(&text),
            };
        }
        match self.command() {
            Some(cmd) => parse_lines(&run(&cmd).await),
            None => Vec::new(),
        }
    }

    /// The command that records a visit, so a pick floats up the list next
    /// time.
    ///
    /// Only zoxide has one by default.  `z` and `cdr` are written by the shell
    /// on every `cd`, and writing their files from here would be reaching into
    /// somebody else's format for no gain; `ghq` lists clones and has no notion
    /// of a visit at all.  `visit_command` overrides whatever this returns.
    pub fn visit_command(&self, project: &crate::config::Project) -> Option<Vec<String>> {
        if !project.visit_command.is_empty() {
            return Some(project.visit_command.clone());
        }
        match self {
            Self::Zoxide => Some(vec!["zoxide".into(), "add".into()]),
            _ => None,
        }
    }
}

/// One absolute path per line: zoxide, ghq and any `dirs_command`.
pub fn parse_lines(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect()
}

/// `~/.z`: `path|rank|time`, in no particular order.
///
/// Sorted by rank alone, which is the visit count z has been decaying as it
/// goes.  z's own ordering also weights by how recently the directory was
/// seen, and that formula is z's business rather than something to reimplement
/// from memory here; rank descending puts the same handful of directories at
/// the top, and the picker is a fuzzy filter rather than a leaderboard.
pub fn parse_z(text: &str) -> Vec<String> {
    let mut rows: Vec<(f64, String)> = text
        .lines()
        .filter_map(|line| {
            let mut fields = line.split('|');
            let path = fields.next()?.trim();
            if path.is_empty() {
                return None;
            }
            let rank = fields.next().unwrap_or("0").trim().parse().unwrap_or(0.0);
            Some((rank, path.to_string()))
        })
        .collect();
    rows.sort_by(|a, b| b.0.total_cmp(&a.0));
    rows.into_iter().map(|(_, p)| p).collect()
}

/// `~/.chpwd-recent-dirs`: zsh's own quoting, most recent first.
///
/// zsh writes each path through its quoting, and which of the three forms it
/// picks depends on what is in the path, so all three are unwrapped here:
/// `$'...'`, `'...'` with `'\''` for an embedded quote, and a bare path with
/// backslash escapes.  Guessing one form and shipping it would work on every
/// path without a space in it, which is every path right up until it is not.
pub fn parse_cdr(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(unquote_zsh)
        .filter(|p| !p.is_empty())
        .collect()
}

/// One zsh-quoted word.
fn unquote_zsh(word: &str) -> String {
    let inner = word
        .strip_prefix("$'")
        .or_else(|| word.strip_prefix('\''))
        .and_then(|rest| rest.strip_suffix('\''));
    match inner {
        // Inside single quotes zsh cannot escape anything but the quote
        // itself, which it does by closing and reopening: '\''.
        Some(inner) => inner.replace("'\\''", "'"),
        // Bare: backslash escapes whatever follows it.
        None => {
            let mut out = String::with_capacity(word.len());
            let mut chars = word.chars();
            while let Some(c) = chars.next() {
                match c {
                    '\\' => out.extend(chars.next()),
                    _ => out.push(c),
                }
            }
            out
        }
    }
}

/// The variables a source's command reads, passed to the daemon with it.
pub const SOURCE_ENV: [&str; 4] = [
    "_ZO_DATA_DIR",
    "_ZO_EXCLUDE_DIRS",
    "_ZO_RESOLVE_SYMLINKS",
    "XDG_DATA_HOME",
];

/// When each directory in zoxide's database was last jumped to, in seconds
/// since the epoch, read from `db.zo` itself: `zoxide query -l` orders by
/// frecency and has no way to ask for recency.
///
/// The format is zoxide's version 3, bincode with fixed-width little-endian
/// integers: a `u32` version, a `u64` count, then per directory a `u64`
/// length, the path's bytes, an `f64` rank and a `u64` last-accessed time.
/// Any other version, or a file cut short, gives `None`, and the caller keeps
/// zoxide's own order rather than guess.
pub fn zoxide_last_jumped(db: &[u8]) -> Option<std::collections::HashMap<String, u64>> {
    fn take<'a>(b: &mut &'a [u8], n: usize) -> Option<&'a [u8]> {
        if b.len() < n {
            return None;
        }
        let (head, rest) = b.split_at(n);
        *b = rest;
        Some(head)
    }
    fn u64_le(b: &mut &[u8]) -> Option<u64> {
        Some(u64::from_le_bytes(take(b, 8)?.try_into().ok()?))
    }
    let mut b = db;
    let version = u32::from_le_bytes(take(&mut b, 4)?.try_into().ok()?);
    if version != 3 {
        return None;
    }
    let count = u64_le(&mut b)?;
    let mut out = std::collections::HashMap::new();
    for _ in 0..count {
        let len = usize::try_from(u64_le(&mut b)?).ok()?;
        let path = std::str::from_utf8(take(&mut b, len)?).ok()?.to_string();
        take(&mut b, 8)?;
        let last = u64_le(&mut b)?;
        out.insert(path, last);
    }
    Some(out)
}

/// How long the daemon keeps a source's answer when it has no file to watch:
/// `ghq` and a `dirs_command`. A minute, so a repository cloned a moment ago
/// is a short wait away rather than a restart away.
pub const COMMAND_TTL: std::time::Duration = std::time::Duration::from_secs(60);

/// How long an answer is kept even when the watched file has not changed.
///
/// zoxide orders by frecency, and the recency half of that moves with the
/// clock alone: a directory visited fifty minutes ago drops a bracket at the
/// hour without anything being written. It also leaves out directories that
/// have since been deleted, which nothing writes down either. Fifteen
/// minutes bounds both.
pub const WATCHED_MAX_AGE: std::time::Duration = std::time::Duration::from_secs(15 * 60);

/// Whether a kept answer can be given again: `age` since it was read,
/// `then` the watched file's modification time when it was read, `now` the
/// same time read now, `watched` whether there is a file to watch at all.
///
/// A watched file is the better signal: the answer is good for as long as
/// the file is unchanged, up to [`WATCHED_MAX_AGE`]. A file that can't be
/// read now, or wasn't then, says nothing, so the fixed [`COMMAND_TTL`]
/// decides, as it does for a source with no file.
pub fn still_good(
    age: std::time::Duration,
    then: Option<std::time::SystemTime>,
    now: Option<std::time::SystemTime>,
    watched: bool,
) -> bool {
    match (watched, then, now) {
        (true, Some(then), Some(now)) => then == now && age < WATCHED_MAX_AGE,
        _ => age < COMMAND_TTL,
    }
}

/// A file's modification time, or `None` when it can't be read.
pub fn modified(path: &std::path::Path) -> Option<std::time::SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// Run a command and return its standard output, empty on any failure.
async fn run(cmd: &[String]) -> String {
    run_with(cmd, &[]).await
}

/// `run` with variables set for it.
pub async fn run_with(cmd: &[String], env: &[(String, String)]) -> String {
    let Some((program, args)) = cmd.split_first() else {
        return String::new();
    };
    let out = tokio::process::Command::new(program)
        .args(args)
        .envs(env.iter().map(|(k, v)| (k, v)))
        .output()
        .await;
    match out {
        Ok(o) => String::from_utf8_lossy(&o.stdout).into_owned(),
        Err(_) => String::new(),
    }
}

/// Record a visit, if the source has a way to record one.
///
/// Failure is silence on purpose: a jumper that has been uninstalled since the
/// config was written should not stop a window opening.
pub async fn record_visit(source: &DirsSource, project: &crate::config::Project, dir: &str) {
    let Some(mut cmd) = source.visit_command(project) else {
        return;
    };
    cmd.push(dir.to_string());
    let Some((program, args)) = cmd.split_first() else {
        return;
    };
    let _ = tokio::process::Command::new(program)
        .args(args)
        .status()
        .await;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A db.zo of version 3 with two directories, built the way zoxide's
    /// bincode writes it.
    fn zo(entries: &[(&str, f64, u64)]) -> Vec<u8> {
        let mut b = 3u32.to_le_bytes().to_vec();
        b.extend((entries.len() as u64).to_le_bytes());
        for (p, rank, last) in entries {
            b.extend((p.len() as u64).to_le_bytes());
            b.extend(p.as_bytes());
            b.extend(rank.to_le_bytes());
            b.extend(last.to_le_bytes());
        }
        b
    }

    #[test]
    fn zoxide_last_jumped_reads_each_directorys_time() {
        let db = zo(&[("/a", 4.0, 100), ("/b", 1.0, 200)]);
        let got = zoxide_last_jumped(&db).unwrap();
        assert_eq!(got.get("/a"), Some(&100));
        assert_eq!(got.get("/b"), Some(&200));
    }

    #[test]
    fn zoxide_last_jumped_gives_up_on_another_version_or_a_short_file() {
        let mut db = zo(&[("/a", 4.0, 100)]);
        assert!(
            zoxide_last_jumped(&db[..db.len() - 3]).is_none(),
            "cut short"
        );
        db[0] = 4;
        assert!(zoxide_last_jumped(&db).is_none(), "version 4");
        assert!(zoxide_last_jumped(&[]).is_none());
    }
    use crate::config::Project;

    #[test]
    fn every_name_in_the_list_parses() {
        // NAMES is what the config error prints and what the manual documents,
        // so a name in it that from_name does not know is a lie in two places.
        for name in NAMES {
            assert!(DirsSource::from_name(name).is_some(), "{name}");
        }
    }

    #[test]
    fn an_unknown_name_is_not_a_source() {
        assert_eq!(DirsSource::from_name("autojump"), None);
    }

    #[test]
    fn the_default_config_is_zoxide() {
        assert_eq!(
            DirsSource::from_config(&Project::default()),
            DirsSource::Zoxide
        );
    }

    #[test]
    fn a_command_beats_a_name() {
        let project = Project {
            dirs_source: "z".into(),
            dirs_command: vec!["ghq".into(), "list".into(), "-p".into()],
            ..Project::default()
        };
        assert_eq!(
            DirsSource::from_config(&project),
            DirsSource::Command(vec!["ghq".into(), "list".into(), "-p".into()])
        );
    }

    #[test]
    fn the_deprecated_flag_still_turns_the_list_off() {
        // A config written before dirs_source existed says `zoxide = false`
        // and means "sessions only". It has to keep meaning that.
        let project = Project {
            zoxide: false,
            ..Project::default()
        };
        assert_eq!(DirsSource::from_config(&project), DirsSource::None);
    }

    #[test]
    fn the_deprecated_flag_wins_over_a_source() {
        // Contradictory, but only one reading is safe: the person who wrote
        // `zoxide = false` wanted no directory list, and honouring the name
        // instead would hand them one they had already turned off.
        let project = Project {
            zoxide: false,
            dirs_source: "ghq".into(),
            ..Project::default()
        };
        assert_eq!(DirsSource::from_config(&project), DirsSource::None);
    }

    #[test]
    fn only_zoxide_records_a_visit_by_default() {
        let project = Project::default();
        assert_eq!(
            DirsSource::Zoxide.visit_command(&project),
            Some(vec!["zoxide".into(), "add".into()])
        );
        assert_eq!(DirsSource::Z.visit_command(&project), None);
        assert_eq!(DirsSource::Cdr.visit_command(&project), None);
        assert_eq!(DirsSource::Ghq.visit_command(&project), None);
        assert_eq!(DirsSource::None.visit_command(&project), None);
    }

    #[test]
    fn a_visit_command_overrides_the_source() {
        let project = Project {
            visit_command: vec!["myjumper".into(), "add".into()],
            ..Project::default()
        };
        assert_eq!(
            DirsSource::Z.visit_command(&project),
            Some(vec!["myjumper".into(), "add".into()])
        );
    }

    #[test]
    fn plain_lines_lose_blanks_and_whitespace() {
        let out = parse_lines("/a\n\n  /b  \n\t\n/c\n");
        assert_eq!(out, vec!["/a", "/b", "/c"]);
    }

    #[test]
    fn the_z_database_sorts_by_rank() {
        let text =
            "/home/y/small|1.5|1700000000\n/home/y/big|42|1700000001\n/home/y/mid|9|1700000002\n";
        assert_eq!(
            parse_z(text),
            vec!["/home/y/big", "/home/y/mid", "/home/y/small"]
        );
    }

    #[test]
    fn a_z_row_without_a_rank_is_still_a_directory() {
        // Whatever wrote a short row, dropping the path loses a directory the
        // user has been to; ranking it last loses nothing.
        let text = "/home/y/broken\n/home/y/fine|3|1700000000\n";
        assert_eq!(parse_z(text), vec!["/home/y/fine", "/home/y/broken"]);
    }

    #[test]
    fn an_empty_z_database_is_no_directories() {
        assert!(parse_z("").is_empty());
        assert!(parse_z("\n\n").is_empty());
    }

    #[test]
    fn cdr_keeps_the_files_order() {
        // The file is most-recent-first and that is exactly the order worth
        // showing, so unlike z there is nothing to sort.
        let text = "'/home/y/one'\n'/home/y/two'\n";
        assert_eq!(parse_cdr(text), vec!["/home/y/one", "/home/y/two"]);
    }

    #[test]
    fn cdr_unwraps_all_three_of_zshs_quotings() {
        let text = "'/home/y/plain'\n$'/home/y/dollar'\n/home/y/with\\ space\n'/home/y/it'\\''s'\n";
        assert_eq!(
            parse_cdr(text),
            vec![
                "/home/y/plain",
                "/home/y/dollar",
                "/home/y/with space",
                "/home/y/it's",
            ]
        );
    }

    #[test]
    fn a_source_that_reads_a_file_runs_no_command() {
        assert!(DirsSource::Z.command().is_none());
        assert!(DirsSource::Cdr.command().is_none());
        assert!(DirsSource::None.command().is_none());
        assert!(DirsSource::Zoxide.command().is_some());
        assert!(DirsSource::Ghq.command().is_some());
    }

    #[test]
    fn z_honours_its_own_environment_variable() {
        // Safe to set here because the getter reads it once, and the test
        // asserts on the path rather than on any file existing.
        unsafe { std::env::set_var("_Z_DATA", "/tmp/elsewhere/.z") };
        assert_eq!(
            DirsSource::Z.data_file("/home/y"),
            Some(std::path::PathBuf::from("/tmp/elsewhere/.z"))
        );
        unsafe { std::env::remove_var("_Z_DATA") };
        assert_eq!(
            DirsSource::Z.data_file("/home/y"),
            Some(std::path::PathBuf::from("/home/y/.z"))
        );
    }

    #[tokio::test]
    async fn none_lists_nothing_and_runs_nothing() {
        assert!(DirsSource::None.list_here("/home/y").await.is_empty());
    }

    #[tokio::test]
    async fn a_command_that_does_not_exist_is_an_empty_list() {
        // The jumper named in the config is not installed on this machine.
        // That is a smaller picker, not an error.
        let source = DirsSource::Command(vec!["tmux-companion-no-such-binary".into()]);
        assert!(source.list_here("/home/y").await.is_empty());
    }

    #[test]
    fn an_unchanged_database_keeps_the_answer_until_the_clock_moves_it() {
        use std::time::{Duration, SystemTime};
        let t = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
        let later = t + Duration::from_secs(5);
        // Untouched, and young: kept, however long past the command TTL.
        assert!(still_good(Duration::from_secs(600), Some(t), Some(t), true));
        // A `cd` wrote the database: read again, however young.
        assert!(!still_good(
            Duration::from_millis(10),
            Some(t),
            Some(later),
            true
        ));
        // Untouched but older than frecency allows: read again.
        assert!(!still_good(WATCHED_MAX_AGE, Some(t), Some(t), true));
    }

    #[test]
    fn without_a_file_to_watch_a_fixed_time_decides() {
        use std::time::{Duration, SystemTime};
        let t = SystemTime::UNIX_EPOCH;
        assert!(still_good(Duration::from_secs(59), None, None, false));
        assert!(!still_good(COMMAND_TTL, None, None, false));
        // A watched file that has gone, or never was: the same fixed time.
        assert!(still_good(Duration::from_secs(5), Some(t), None, true));
        assert!(still_good(Duration::from_secs(5), None, Some(t), true));
        assert!(!still_good(Duration::from_secs(61), None, Some(t), true));
    }

    #[test]
    fn only_zoxide_has_a_database_to_watch() {
        let z = DirsSource::Zoxide.watched_file("/home/y").expect("a file");
        assert!(z.ends_with("zoxide/db.zo"), "{}", z.display());
        assert_eq!(DirsSource::Ghq.watched_file("/home/y"), None);
        assert_eq!(
            DirsSource::Command(vec!["x".into()]).watched_file("/home/y"),
            None
        );
        assert_eq!(DirsSource::Z.watched_file("/home/y"), None);
    }
}
