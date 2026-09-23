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

    /// The directories this source knows, most relevant first.
    ///
    /// Every failure is the empty list: a jumper that is not installed, a
    /// database that has never been written, a command that exits non-zero.
    /// The picker degrades to live sessions and a typed path, which is a
    /// smaller tool and still a working one, and that is better than an error
    /// in a popup somebody opened to switch projects.
    pub async fn list(&self, home: &str) -> Vec<String> {
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

/// Run a command and return its standard output, empty on any failure.
async fn run(cmd: &[String]) -> String {
    let Some((program, args)) = cmd.split_first() else {
        return String::new();
    };
    let out = tokio::process::Command::new(program)
        .args(args)
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
        assert!(DirsSource::None.list("/home/y").await.is_empty());
    }

    #[tokio::test]
    async fn a_command_that_does_not_exist_is_an_empty_list() {
        // The jumper named in the config is not installed on this machine.
        // That is a smaller picker, not an error.
        let source = DirsSource::Command(vec!["tmux-companion-no-such-binary".into()]);
        assert!(source.list("/home/y").await.is_empty());
    }
}
