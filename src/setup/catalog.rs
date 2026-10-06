//! The catalog: every item `setup` lists, read out of `items.toml`.
//!
//! The file is compiled in, so the list cannot go missing from an install, and
//! parsed when the command starts, so a mistake in it is a test failure rather
//! than a panic somebody meets in a popup. The tests at the bottom hold it to
//! clap and to `docs/tmux.conf.full.example`: a snippet that runs a command
//! the binary does not have, or a binding that says something different from
//! the one the example writes, fails the suite.

use serde::Deserialize;

/// The file, as compiled in.
pub const ITEMS: &str = include_str!("items.toml");

/// Where an item sits in the list.
///
/// The first four mirror what the cheat sheet groups by, with the bar split out
/// of the config because it is the thing on screen all day.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum Group {
    /// Key bindings.
    Keys,
    /// tmux hooks, and the one mouse binding that behaves like one.
    Hooks,
    /// What the status bar draws.
    Bar,
    /// Settings in config.toml.
    Config,
    /// Things outside tmux: the shell, the agent, the font, the PATH.
    #[serde(rename = "Outside tmux")]
    Outside,
}

impl Group {
    /// The name as the catalog writes it.
    pub fn name(self) -> &'static str {
        match self {
            Group::Keys => "Keys",
            Group::Hooks => "Hooks",
            Group::Bar => "Bar",
            Group::Config => "Config",
            Group::Outside => "Outside tmux",
        }
    }
}

/// What an item is, which decides how it is detected and where it is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    /// A key binding in tmux.conf.
    Binding,
    /// A `set-hook` in tmux.conf.
    Hook,
    /// A tmux option in tmux.conf.
    Option,
    /// A setting in config.toml.
    Config,
    /// Something outside tmux: copied, never written.
    Outside,
}

/// One item.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Item {
    /// Stable name, used in the skip file and in the managed block.
    pub id: String,
    /// Where it sits in the list.
    pub group: Group,
    /// How it is detected and where it goes.
    pub kind: Kind,
    /// What it is for, as the question it answers.
    pub line: String,
    /// The tmux-companion words a binding, hook or option runs.
    #[serde(default)]
    pub command: String,
    /// The key the full example binds.
    #[serde(default)]
    pub key: String,
    /// The key table: prefix, root or copy-mode-vi.
    #[serde(default)]
    pub table: String,
    /// Keys to offer when `key` is taken.
    #[serde(default)]
    pub fallback: Vec<String>,
    /// The tmux hook name.
    #[serde(default)]
    pub hook: String,
    /// The tmux option.
    #[serde(default)]
    pub option: String,
    /// The dotted path in config.toml.
    #[serde(default)]
    pub setting: String,
    /// The name an array of tables has to hold.
    #[serde(default)]
    pub contains: String,
    /// Write what is in force before adding to an array that is not in the file.
    #[serde(default)]
    pub seed: bool,
    /// Which outside check decides it.
    #[serde(default)]
    pub check: String,
    /// Whether it may be written at all, rather than only copied.
    #[serde(default = "yes")]
    pub write: bool,
    /// The lines to copy and write; `{key}` is the key chosen.
    pub snippet: String,
    /// The file it goes in.
    pub goes: String,
}

fn yes() -> bool {
    true
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    item: Vec<Item>,
}

/// Parse a catalog.
pub fn parse(text: &str) -> anyhow::Result<Vec<Item>> {
    let file: File = toml::from_str(text)?;
    Ok(file.item)
}

/// The compiled-in catalog.
///
/// A parse failure here is a bug in this repository rather than on the
/// machine running it, and the tests catch it before a release does.
pub fn items() -> Vec<Item> {
    parse(ITEMS).expect("src/setup/items.toml parses; the catalog tests say why not")
}

impl Item {
    /// Whether it can be written to a file, as opposed to only copied.
    pub fn writable(&self) -> bool {
        self.write && self.kind != Kind::Outside
    }

    /// The file it is written to, when it is written at all.
    pub fn target(&self) -> Option<Target> {
        if !self.writable() {
            return None;
        }
        Some(match self.kind {
            Kind::Config => Target::Config,
            _ => Target::TmuxConf,
        })
    }

    /// The snippet with a key in place of `{key}`, trimmed of blank edges.
    pub fn snippet_for(&self, key: Option<&str>) -> String {
        self.snippet_in(key, popups_take_b())
    }

    /// The snippet for a tmux that does or does not take `display-popup -B`.
    ///
    /// The picker draws its own border, so the snippets ask tmux not to draw
    /// a second one round it. `-B` is tmux 3.3; a 3.2 refuses the flag and the
    /// popup never opens, so there it comes out and the border is doubled.
    pub fn snippet_in(&self, key: Option<&str>, borderless: bool) -> String {
        let key = key.map(quote_key).unwrap_or_else(|| "{key}".to_string());
        let s = self.snippet.trim().replace("{key}", &key);
        if borderless {
            s
        } else {
            s.replace("display-popup -B ", "display-popup ")
        }
    }

    /// The command's words: the subcommand path and the flags.
    pub fn words(&self) -> Vec<&str> {
        self.command.split_whitespace().collect()
    }
}

/// Whether the tmux on PATH takes `display-popup -B`, asked once.
///
/// `tmux -V` rather than `#{version}`, so it answers with no server running.
/// A tmux that cannot be asked gets the flag: every tmux still packaged
/// anywhere current is 3.3 or later except Ubuntu 22.04's 3.2a, and that one
/// answers `-V` like any other.
pub fn popups_take_b() -> bool {
    static ANSWER: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ANSWER.get_or_init(|| {
        crate::tmux::command_sync()
            .arg("-V")
            .output()
            .ok()
            .map(|o| at_least(&String::from_utf8_lossy(&o.stdout), (3, 3)))
            .unwrap_or(true)
    })
}

/// Whether `tmux -V` output names at least this version: `tmux 3.3a`,
/// `tmux next-3.6`, `tmux 3.2`. Unreadable output is taken as new enough.
pub fn at_least(version: &str, want: (u32, u32)) -> bool {
    let v = version.trim().rsplit(' ').next().unwrap_or("");
    let v = v.rsplit('-').next().unwrap_or(v);
    let mut parts = v.split('.');
    let major = parts.next().and_then(|p| p.parse::<u32>().ok());
    let minor = parts.next().map(|p| {
        p.chars()
            .take_while(char::is_ascii_digit)
            .collect::<String>()
            .parse::<u32>()
            .unwrap_or(0)
    });
    match (major, minor) {
        (Some(ma), Some(mi)) => (ma, mi) >= want,
        _ => true,
    }
}

/// Which file an item is written to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// The tmux config tmux loaded.
    TmuxConf,
    /// tmux-companion's config.toml.
    Config,
}

/// A key as tmux.conf has to spell it.
///
/// Most keys go in bare. The characters tmux's parser gives a meaning to --
/// `;` ends a command, `#` starts a comment or a format, the quotes quote and
/// the braces open a block -- go in quotes, which is how `list-keys` itself
/// would write most of them back; a lone `'` goes in double quotes.
pub fn quote_key(key: &str) -> String {
    const SPECIAL: &[char] = &[';', '#', '"', '\'', '{', '}', '~', '$', '%', ' '];
    if !key.contains(SPECIAL) {
        return key.to_string();
    }
    if key.contains('\'') {
        format!("\"{}\"", key.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        format!("'{key}'")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::setup::detect::{self, Bound};

    #[test]
    fn the_catalog_parses_and_every_id_is_unique() {
        let items = items();
        assert!(items.len() > 30, "{} items", items.len());
        let mut ids: Vec<&str> = items.iter().map(|i| i.id.as_str()).collect();
        ids.sort_unstable();
        let before = ids.len();
        ids.dedup();
        assert_eq!(before, ids.len(), "an id is used twice");
    }

    #[test]
    fn every_item_has_the_fields_its_kind_needs() {
        for i in items() {
            assert!(!i.line.trim().is_empty(), "{} has no line", i.id);
            assert!(!i.snippet.trim().is_empty(), "{} has no snippet", i.id);
            match i.kind {
                Kind::Binding => {
                    assert!(!i.key.is_empty() && !i.command.is_empty(), "{}", i.id);
                    assert!(
                        ["prefix", "root", "copy-mode-vi"].contains(&i.table.as_str()),
                        "{}: table {}",
                        i.id,
                        i.table
                    );
                    assert!(
                        i.snippet.contains("{key}"),
                        "{} snippet has no {{key}}",
                        i.id
                    );
                }
                Kind::Hook => assert!(!i.hook.is_empty() && !i.command.is_empty(), "{}", i.id),
                Kind::Option => assert!(!i.option.is_empty() && !i.command.is_empty(), "{}", i.id),
                Kind::Config => {
                    assert!(!i.setting.is_empty(), "{}", i.id);
                    assert_eq!(i.goes, "config.toml", "{}", i.id);
                    // The fragment is what gets merged, so it has to be TOML.
                    i.snippet
                        .parse::<toml_edit::DocumentMut>()
                        .unwrap_or_else(|e| panic!("{}: {e}", i.id));
                }
                Kind::Outside => assert!(
                    ["marks", "claude-hooks", "zoxide", "nerd-font", "clipboard"]
                        .contains(&i.check.as_str()),
                    "{}: check {}",
                    i.id,
                    i.check
                ),
            }
        }
    }

    #[test]
    fn every_config_setting_is_a_real_path_in_the_config() {
        // A typo in `setting` would read as "not set" forever, and the item
        // would stay open whatever somebody wrote.
        let effective =
            toml::Value::try_from(crate::config::Config::default()).expect("serialises");
        for i in items().iter().filter(|i| i.kind == Kind::Config) {
            let parent = i.setting.rsplit_once('.').map_or("", |(p, _)| p);
            let found = detect::lookup(&effective, &i.setting).is_some()
                // An array that is empty by default serialises to nothing, so
                // its parent table is what has to exist.
                || parent.is_empty()
                || detect::lookup(&effective, parent).is_some();
            assert!(found, "{}: `{}` is not in the config", i.id, i.setting);
        }
    }

    #[test]
    fn every_command_the_catalog_runs_is_one_clap_has() {
        use clap::CommandFactory;
        let cli = crate::cli::Cli::command();
        let parents = detect::parent_commands();
        let mut checked = 0;
        for i in items() {
            // The command field, and every invocation in the snippet: both are
            // claims that a command exists.
            let mut calls = detect::invocations(&i.snippet, &parents);
            if !i.command.is_empty() {
                calls.extend(detect::invocations(
                    &format!("tmux-companion {}", i.command),
                    &parents,
                ));
            }
            for call in calls {
                let mut cmd = &cli;
                for word in &call.path {
                    cmd = cmd.find_subcommand(word).unwrap_or_else(|| {
                        panic!("{}: no command `{}`", i.id, call.path.join(" "))
                    });
                }
                for flag in &call.flags {
                    let name = flag.trim_start_matches("--");
                    assert!(
                        cmd.get_arguments().any(|a| a.get_long() == Some(name)),
                        "{}: `{}` has no {flag}",
                        i.id,
                        call.path.join(" ")
                    );
                }
                checked += 1;
            }
        }
        assert!(checked > 40, "only {checked} commands checked");
    }

    /// The full example's bindings, parsed the same way as a snippet.
    fn example_bindings() -> Vec<Bound> {
        let text = include_str!("../../docs/tmux.conf.full.example");
        detect::conf_bindings(text)
    }

    #[test]
    fn every_binding_is_the_one_the_full_example_writes() {
        // The two files say the same thing about the same key, so a binding
        // changed in one of them and not the other fails here.
        let example = example_bindings();
        for i in items().iter().filter(|i| i.kind == Kind::Binding) {
            let ours = detect::conf_bindings(&i.snippet_for(Some(&i.key)));
            assert_eq!(ours.len(), 1, "{}: snippet is not one binding", i.id);
            let ours = &ours[0];
            let theirs = example
                .iter()
                .find(|b| b.table == i.table && b.key == i.key)
                .unwrap_or_else(|| {
                    panic!(
                        "{}: the full example binds nothing on {} {}",
                        i.id, i.table, i.key
                    )
                });
            assert_eq!(ours, theirs, "{} differs from the full example", i.id);
        }
    }

    #[test]
    fn every_companion_binding_in_the_full_example_is_in_the_catalog() {
        // The other direction: a key added to the example and not here would
        // be a feature `setup` never mentions.
        let parents = detect::parent_commands();
        let items = items();
        for b in example_bindings() {
            if detect::invocations(&b.command, &parents).is_empty() {
                continue;
            }
            assert!(
                items
                    .iter()
                    .any(|i| i.kind == Kind::Binding && i.table == b.table && i.key == b.key),
                "the full example binds {} {} to `{}` and the catalog has no item for it",
                b.table,
                b.key,
                b.command
            );
        }
    }

    #[test]
    fn a_key_tmux_would_misread_is_quoted() {
        assert_eq!(quote_key("g"), "g");
        assert_eq!(quote_key("M-s"), "M-s");
        assert_eq!(quote_key("`"), "`");
        assert_eq!(quote_key(";"), "';'");
        assert_eq!(quote_key("#"), "'#'");
        assert_eq!(quote_key("'"), "\"'\"");
        assert_eq!(quote_key("\""), "'\"'");
    }

    #[test]
    fn a_snippet_takes_the_key_it_is_given() {
        let i = items().into_iter().find(|i| i.id == "panes").unwrap();
        assert!(i.snippet_for(Some("j")).contains("\" j display-popup"));
        assert!(i.snippet_for(None).contains("{key}"));
    }

    #[test]
    fn a_picker_popup_drops_its_border_where_tmux_can_and_keeps_it_where_it_cannot() {
        let items = items();
        let panes = items.iter().find(|i| i.id == "panes").unwrap();
        assert!(
            panes
                .snippet_in(Some("g"), true)
                .contains("display-popup -B -E")
        );
        let old = panes.snippet_in(Some("g"), false);
        assert!(old.contains("display-popup -E"), "{old}");
        assert!(!old.contains("-B"), "{old}");
        // The brief draws the pickers' frame itself, so it drops tmux's
        // border the same way.
        let brief = items.iter().find(|i| i.id == "brief").unwrap();
        assert!(
            brief
                .snippet_in(Some("b"), true)
                .contains("display-popup -B -E")
        );
        assert!(!brief.snippet_in(Some("b"), false).contains("-B"));
    }

    #[test]
    fn a_version_is_read_the_way_tmux_prints_it() {
        assert!(at_least("tmux 3.3", (3, 3)));
        assert!(at_least("tmux 3.3a", (3, 3)));
        assert!(at_least("tmux 3.7c\n", (3, 3)));
        assert!(at_least("tmux 3.10", (3, 3)));
        assert!(at_least("tmux next-3.6", (3, 3)));
        assert!(!at_least("tmux 3.2a", (3, 3)));
        assert!(!at_least("tmux 2.9", (3, 3)));
        assert!(at_least("", (3, 3)));
    }

    #[test]
    fn only_what_is_inside_tmux_is_written() {
        for i in items() {
            match i.kind {
                Kind::Outside => assert_eq!(i.target(), None, "{}", i.id),
                Kind::Config if i.write => assert_eq!(i.target(), Some(Target::Config)),
                Kind::Config => assert_eq!(i.target(), None),
                _ => assert_eq!(i.target(), Some(Target::TmuxConf), "{}", i.id),
            }
        }
    }
}
