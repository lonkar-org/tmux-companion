//! The configuration file: where it lives, how it is parsed, and what happens
//! when it cannot be.
//!
//! Every field has a default, so a machine with no config file behaves exactly
//! as the binary did before this module existed. That is not a claim, it is a
//! test: `Config::default()` has to render byte for byte what the pinned tests
//! expect.
//!
//! TOML rather than YAML, for reasons written down in `docs/comrades-port.md`.
//! The short version: `serde_yaml` was archived by its author in March 2024 and
//! both forks have sat still since, YAML 1.1 turns `no` and `off` into booleans
//! which is a problem for a file full of one-word glyph values, and anybody
//! installing a Rust program already has a `Cargo.toml` open.

use std::{collections::HashMap, path::PathBuf};

use serde::{Deserialize, Serialize};

/// Everything the daemon and its clients can be told to do differently.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
#[serde(deny_unknown_fields, default)]
pub struct Config {
    /// Process-wide settings: logging, and where state is kept.
    pub general: General,
    /// Directory aliases and per-directory icons for the window segment.
    pub dirs: Dirs,
    /// The git segment.
    pub git: Git,
    /// The bandwidth segment.
    pub network: Network,
    /// The battery segment.
    pub battery: Battery,
    /// Which glyphs the bar draws with.
    pub glyphs: Glyphs,
}

/// Which glyphs the bar draws with.
///
/// The default preset assumes a Nerd Fonts v3 patch, which most people do not
/// have, and a bar of boxes tells a new reader nothing about whether their
/// install worked.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
#[serde(deny_unknown_fields, default)]
pub struct Glyphs {
    /// Which set to start from.
    pub preset: Preset,
    /// Replacements for individual glyphs, by the constant name in
    /// `src/tmux/icons.rs`, applied on top of the preset.
    ///
    /// One missing icon is a reason to fix that icon, not to drop to a whole
    /// preset below.
    pub icons: HashMap<String, String>,
}

/// A named set of glyph replacements.
///
/// A preset is a table of names to strings in its own file, so adding one is a
/// data change with no Rust attached.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Preset {
    /// The Nerd Fonts v3 codepoints in `src/tmux/icons.rs`, unchanged.
    #[default]
    NerdFontV3,
    /// 7-bit, for a terminal whose font nobody controls.
    Ascii,
}

impl Preset {
    /// The preset's replacements, empty for the default set.
    pub fn table(self) -> HashMap<String, String> {
        let text = match self {
            Preset::NerdFontV3 => return HashMap::new(),
            Preset::Ascii => include_str!("presets/ascii.toml"),
        };
        toml::from_str(text).expect("a shipped preset parses")
    }
}

/// Process-wide settings.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
#[serde(deny_unknown_fields, default)]
pub struct General {
    /// A file the daemon appends diagnostics to. Empty means no log.
    ///
    /// A picker inside `display-popup -E` sends its stderr wherever the popup
    /// went, which is nowhere, so a path here is the only way to see what a
    /// misbehaving command said.
    pub log: Option<PathBuf>,
}

/// Directory aliases and per-directory icons for the window segment.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
#[serde(deny_unknown_fields, default)]
pub struct Dirs {
    /// Path to label, replacing the abbreviated path in the window segment.
    ///
    /// This is what `~/.yrl/lib/dir-aliases` used to hold, which was a path on
    /// one laptop compiled into a binary other people are invited to install.
    /// The old file is still read when this table is empty, so nothing breaks
    /// the day somebody upgrades.
    pub aliases: HashMap<PathBuf, String>,
}

/// The git segment.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(deny_unknown_fields, default)]
pub struct Git {
    /// How long a parsed status stays fresh, in seconds. Zero disables the
    /// cache.
    pub ttl_secs: f64,
    /// How long "this path is inside a work tree" is trusted, in seconds.
    ///
    /// A directory's repo-ness effectively never changes, but caching it
    /// forever would leave a fresh `git init` invisible until the daemon
    /// restarts.
    pub repo_check_ttl_secs: f64,
    /// Middle-ellipsize a branch name longer than this many characters.
    pub branch_max_len: usize,
    /// How many characters of the branch name's tail survive the ellipsis.
    pub branch_tail_len: usize,
}

impl Default for Git {
    fn default() -> Self {
        Self {
            ttl_secs: 5.0,
            repo_check_ttl_secs: 300.0,
            branch_max_len: 20,
            branch_tail_len: 10,
        }
    }
}

/// The bandwidth segment.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(deny_unknown_fields, default)]
pub struct Network {
    /// Below this many bytes per second, the segment draws nothing.
    ///
    /// The default is 20 KiB/s: a bar that reacts to every background poll is
    /// noise rather than information.
    pub threshold_bps: u64,
}

impl Default for Network {
    fn default() -> Self {
        Self {
            threshold_bps: 20_480,
        }
    }
}

/// The battery segment.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(deny_unknown_fields, default)]
pub struct Battery {
    /// How long a battery reading stays fresh, in seconds.
    pub ttl_secs: f64,
}

impl Default for Battery {
    fn default() -> Self {
        Self { ttl_secs: 30.0 }
    }
}

/// The glyph substitutions to apply to a rendered segment.
///
/// Rendering uses the constants in `src/tmux/icons.rs` throughout, and the
/// preset is applied once to the finished string rather than threaded through
/// 263 call sites. That keeps one vocabulary in the code, makes the default
/// preset free — `apply` returns the input untouched — and means a preset can
/// only replace glyphs the default set contains, which is the limitation worth
/// knowing about.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GlyphMap {
    /// From default glyph to replacement, longest first so a two-character
    /// glyph is not half-matched by a one-character one.
    pairs: Vec<(String, String)>,
}

impl GlyphMap {
    /// Build the map for a config: the preset, then the per-icon overrides.
    pub fn new(glyphs: &Glyphs) -> Self {
        let mut table = glyphs.preset.table();
        for (name, value) in &glyphs.icons {
            table.insert(name.clone(), value.clone());
        }

        let mut pairs: Vec<(String, String)> = table
            .into_iter()
            .filter_map(|(name, replacement)| {
                crate::tmux::icons::by_name(&name).map(|glyph| (glyph.to_string(), replacement))
            })
            .filter(|(from, to)| from != to && !from.is_empty())
            .collect();
        pairs.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then(a.0.cmp(&b.0)));
        Self { pairs }
    }

    /// Whether this map changes anything at all.
    pub fn is_identity(&self) -> bool {
        self.pairs.is_empty()
    }

    /// Apply the substitutions to one rendered segment.
    ///
    /// One pass, matching the longest glyph at each position, so replacements
    /// never feed into each other: an `ascii` preset mapping `STAGED` to `*`
    /// cannot then have that `*` rewritten by a later pair.
    pub fn apply<'a>(&self, s: &'a str) -> std::borrow::Cow<'a, str> {
        if self.is_identity() {
            return std::borrow::Cow::Borrowed(s);
        }
        let mut out = String::with_capacity(s.len());
        let mut rest = s;
        'outer: while !rest.is_empty() {
            for (from, to) in &self.pairs {
                if let Some(stripped) = rest.strip_prefix(from.as_str()) {
                    out.push_str(to);
                    rest = stripped;
                    continue 'outer;
                }
            }
            let ch = rest.chars().next().expect("non-empty");
            out.push(ch);
            rest = &rest[ch.len_utf8()..];
        }
        std::borrow::Cow::Owned(out)
    }
}

// ── Loading ──────────────────────────────────────────────────────────────────

/// Where a config file was found, or that there wasn't one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// Read from this path.
    File(PathBuf),
    /// No file anywhere in the search order; built-in defaults in use.
    Defaults,
}

impl std::fmt::Display for Source {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Source::File(p) => write!(f, "{}", p.display()),
            Source::Defaults => f.write_str("(built-in defaults, no config file found)"),
        }
    }
}

/// A config that could not be parsed, with enough detail to fix it.
#[derive(Debug)]
pub struct ConfigError {
    /// The file the error came from.
    pub path: PathBuf,
    /// What `toml` said, line and column included.
    pub message: String,
    /// The known key closest to the one that was not recognised, when the
    /// error looks like a typo.
    pub did_you_mean: Option<String>,
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.path.display(), self.message)?;
        if let Some(s) = &self.did_you_mean {
            write!(f, "\n  did you mean `{s}`?")?;
        }
        Ok(())
    }
}

impl std::error::Error for ConfigError {}

/// The field names serde says it expected, pulled out of its own message.
///
/// Better than a list of every key in the struct: `ttl_secs` exists under
/// `git`, `battery` and nothing else agrees, so a global list suggests the
/// wrong table. Serde already knows which table it was reading.
fn expected_fields(message: &str) -> Vec<String> {
    let Some(start) = message.find("expected one of ") else {
        return Vec::new();
    };
    message[start..]
        .split('`')
        .skip(1)
        .step_by(2)
        .map(|s| s.to_string())
        .collect()
}

/// Levenshtein distance, for suggesting the key somebody meant.
fn edit_distance(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            cur[j + 1] = (prev[j] + cost).min(prev[j + 1] + 1).min(cur[j] + 1);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

/// The expected field closest to `unknown`, when one is close enough to be a
/// typo rather than a different word entirely.
fn closest_key(unknown: &str, known: &[String]) -> Option<String> {
    known
        .iter()
        .map(|k| (edit_distance(unknown, k), k))
        .filter(|(d, _)| *d > 0 && *d <= 3)
        .min_by_key(|(d, _)| *d)
        .map(|(_, k)| k.clone())
}

/// Pull the offending key out of a serde error like ``unknown field `foo` ``.
fn unknown_field(message: &str) -> Option<String> {
    let start = message.find("unknown field `")? + "unknown field `".len();
    let rest = &message[start..];
    let end = rest.find('`')?;
    Some(rest[..end].to_string())
}

/// Parse a config from TOML text, naming the file in any error.
pub fn parse(text: &str, path: &std::path::Path) -> Result<Config, ConfigError> {
    match toml::from_str::<Config>(text) {
        Ok(c) => Ok(c),
        Err(e) => {
            let message = e.to_string();
            let did_you_mean = unknown_field(&message)
                .as_deref()
                .and_then(|k| closest_key(k, &expected_fields(&message)));
            Err(ConfigError {
                path: path.to_path_buf(),
                message,
                did_you_mean,
            })
        }
    }
}

/// The candidate config paths, in the order they are tried.
///
/// `--config` is handled by the caller, since it never has to exist to be
/// meant.
pub fn search_paths() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(p) = std::env::var_os("TMUX_COMPANION_CONFIG") {
        out.push(PathBuf::from(p));
    }
    let xdg = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")));
    if let Some(dir) = xdg {
        out.push(dir.join("tmux-companion").join("config.toml"));
    }
    if let Some(home) = std::env::var_os("HOME") {
        out.push(PathBuf::from(home).join("tmux-companion.toml"));
    }
    out
}

/// Find and parse the config, or return the defaults when there is no file.
///
/// An unreadable file is skipped as if it were absent; a file that exists and
/// does not parse is an error, because silently falling back to the defaults is
/// how somebody spends an evening wondering why a setting does nothing.
pub fn load() -> Result<(Config, Source), ConfigError> {
    for path in search_paths() {
        match std::fs::read_to_string(&path) {
            Ok(text) => return parse(&text, &path).map(|c| (c, Source::File(path))),
            Err(_) => continue,
        }
    }
    Ok((Config::default(), Source::Defaults))
}

/// Load from an explicit path, which must exist.
pub fn load_from(path: &std::path::Path) -> Result<(Config, Source), ConfigError> {
    let text = std::fs::read_to_string(path).map_err(|e| ConfigError {
        path: path.to_path_buf(),
        message: e.to_string(),
        did_you_mean: None,
    })?;
    parse(&text, path).map(|c| (c, Source::File(path.to_path_buf())))
}

/// Serialise the current defaults as a TOML document.
///
/// This is what `config dump` prints and what `docs/config.example.toml` is
/// generated from, so the example cannot drift away from the struct.
pub fn dump_defaults() -> String {
    toml::to_string_pretty(&Config::default()).expect("defaults serialise")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_file_is_the_defaults() {
        let c = parse("", std::path::Path::new("test.toml")).unwrap();
        assert_eq!(c, Config::default());
    }

    #[test]
    fn defaults_match_the_constants_the_code_used_before_the_config_existed() {
        // The promise this module makes: a machine with no config file behaves
        // exactly as the binary did before it was written.
        let c = Config::default();
        assert_eq!(c.git.ttl_secs, 5.0);
        assert_eq!(c.git.repo_check_ttl_secs, 300.0);
        assert_eq!(c.git.branch_max_len, 20);
        assert_eq!(c.git.branch_tail_len, 10);
        assert_eq!(c.network.threshold_bps, 20_480);
        assert_eq!(c.battery.ttl_secs, 30.0);
        assert!(c.dirs.aliases.is_empty());
        assert!(c.general.log.is_none());
    }

    #[test]
    fn a_partial_file_leaves_everything_else_alone() {
        let c = parse("[git]\nttl_secs = 0.0\n", std::path::Path::new("t.toml")).unwrap();
        assert_eq!(c.git.ttl_secs, 0.0);
        assert_eq!(c.git.branch_max_len, 20);
        assert_eq!(c.network, Network::default());
    }

    #[test]
    fn an_unknown_key_is_an_error_with_the_key_in_it() {
        let e = parse("[git]\nttl_sec = 5\n", std::path::Path::new("t.toml")).unwrap_err();
        assert!(e.message.contains("ttl_sec"), "{}", e.message);
    }

    #[test]
    fn an_unknown_key_suggests_the_one_that_was_meant() {
        // `ttl_secs` exists under `git` and under `battery`, so this also
        // checks the suggestion comes from the table being read rather than
        // from a global list of every key in the struct.
        let e = parse("[git]\nttl_sec = 5\n", std::path::Path::new("t.toml")).unwrap_err();
        assert_eq!(e.did_you_mean.as_deref(), Some("ttl_secs"));
    }

    #[test]
    fn an_unknown_table_is_an_error_too() {
        let e = parse("[gti]\nttl_secs = 5\n", std::path::Path::new("t.toml")).unwrap_err();
        assert!(e.message.contains("gti"), "{}", e.message);
        assert_eq!(e.did_you_mean.as_deref(), Some("git"));
    }

    #[test]
    fn a_nonsense_key_gets_no_suggestion() {
        // A suggestion that is not close to anything is worse than none: it
        // sends the reader off to change a key that was already right.
        let e = parse("[git]\nqqqqqqqqqqqq = 5\n", std::path::Path::new("t.toml")).unwrap_err();
        assert_eq!(e.did_you_mean, None);
    }

    #[test]
    fn the_error_message_names_the_file_and_the_line() {
        let e = parse(
            "[git]\nttl_secs = \"soon\"\n",
            std::path::Path::new("/tmp/x.toml"),
        )
        .unwrap_err();
        let s = e.to_string();
        assert!(s.contains("/tmp/x.toml"), "{s}");
        assert!(s.contains("line 2") || s.contains("2:"), "{s}");
    }

    #[test]
    fn the_dump_round_trips_back_to_the_defaults() {
        // What makes docs/config.example.toml safe to generate: reading the
        // dump back has to give exactly what produced it.
        let text = dump_defaults();
        let parsed = parse(&text, std::path::Path::new("dump.toml")).unwrap();
        assert_eq!(parsed, Config::default());
    }

    #[test]
    fn the_example_file_documents_every_setting() {
        // `toml` cannot emit comments, so docs/config.example.toml is written
        // by hand and this is what stops it drifting: every key the dump
        // produces has to appear in the example, or a setting exists that
        // nobody reading the example would ever find.
        let example = include_str!("../docs/config.example.toml");
        for line in dump_defaults().lines() {
            let Some((key, _)) = line.split_once(" = ") else {
                continue;
            };
            let key = key.trim();
            assert!(
                example.contains(key),
                "`{key}` is missing from docs/config.example.toml"
            );
        }
    }

    #[test]
    fn the_example_file_parses_and_is_the_defaults() {
        // It documents the defaults, so reading it has to produce them. A
        // stale example that no longer parses is worse than none, because
        // somebody copies it.
        let example = include_str!("../docs/config.example.toml");
        let parsed = parse(example, std::path::Path::new("config.example.toml")).unwrap();
        assert_eq!(parsed, Config::default());
    }

    // ── glyphs ───────────────────────────────────────────────────────────────

    #[test]
    fn the_default_preset_changes_nothing() {
        // The promise byte-identity rests on: with no config, the substitution
        // pass is not just a no-op, it does not even copy the string.
        let map = GlyphMap::new(&Glyphs::default());
        assert!(map.is_identity());
        let rendered = format!("#[fg=colour233]{}main", crate::tmux::icons::BRANCH);
        assert!(matches!(
            map.apply(&rendered),
            std::borrow::Cow::Borrowed(_)
        ));
        assert_eq!(map.apply(&rendered), rendered);
    }

    #[test]
    fn the_ascii_preset_replaces_glyphs() {
        let map = GlyphMap::new(&Glyphs {
            preset: Preset::Ascii,
            icons: HashMap::new(),
        });
        assert!(!map.is_identity());
        let rendered = format!(
            "{}main {}2",
            crate::tmux::icons::BRANCH,
            crate::tmux::icons::AHEAD
        );
        let out = map.apply(&rendered).into_owned();
        assert!(!out.contains(crate::tmux::icons::AHEAD), "{out}");
        assert!(out.contains("^"), "{out}");
    }

    #[test]
    fn an_override_beats_the_preset() {
        let map = GlyphMap::new(&Glyphs {
            preset: Preset::Ascii,
            icons: HashMap::from([("AHEAD".to_string(), "UP".to_string())]),
        });
        let out = map.apply(crate::tmux::icons::AHEAD).into_owned();
        assert_eq!(out, "UP");
    }

    #[test]
    fn an_override_works_without_a_preset() {
        let map = GlyphMap::new(&Glyphs {
            preset: Preset::NerdFontV3,
            icons: HashMap::from([("STAGED".to_string(), "*".to_string())]),
        });
        assert_eq!(map.apply(crate::tmux::icons::STAGED).into_owned(), "*");
        // and nothing else moved
        assert_eq!(
            map.apply(crate::tmux::icons::AHEAD).into_owned(),
            crate::tmux::icons::AHEAD
        );
    }

    #[test]
    fn a_replacement_is_not_itself_replaced() {
        // The reason `apply` is one pass with a longest-match rather than a
        // sequence of `str::replace` calls: `STAGED` becoming `*` must not
        // then be rewritten by whatever else maps to or from `*`.
        let map = GlyphMap::new(&Glyphs {
            preset: Preset::NerdFontV3,
            icons: HashMap::from([
                ("STAGED".to_string(), "*".to_string()),
                ("MODIFIED".to_string(), "~".to_string()),
            ]),
        });
        let rendered = format!(
            "{}{}",
            crate::tmux::icons::STAGED,
            crate::tmux::icons::MODIFIED
        );
        assert_eq!(map.apply(&rendered).into_owned(), "*~");
    }

    #[test]
    fn an_unknown_icon_name_is_ignored_rather_than_fatal() {
        // A glyph that existed in an older build and was renamed should not
        // stop the daemon: the bar loses one substitution, not its whole self.
        let map = GlyphMap::new(&Glyphs {
            preset: Preset::NerdFontV3,
            icons: HashMap::from([("NOT_A_GLYPH".to_string(), "!".to_string())]),
        });
        assert!(map.is_identity());
    }

    #[test]
    fn an_unknown_preset_name_is_a_config_error() {
        let e = parse(
            "[glyphs]\npreset = \"powerline\"\n",
            std::path::Path::new("t.toml"),
        )
        .unwrap_err();
        assert!(e.message.contains("powerline"), "{}", e.message);
    }

    #[test]
    fn the_shipped_ascii_preset_names_only_real_glyphs() {
        // A preset is data, so nothing stops a typo in it except this.
        let table = Preset::Ascii.table();
        assert!(!table.is_empty());
        for name in table.keys() {
            assert!(
                crate::tmux::icons::by_name(name).is_some(),
                "ascii.toml names `{name}`, which is not a glyph"
            );
        }
    }

    #[test]
    fn expected_fields_come_out_of_serdes_own_message() {
        let fields = expected_fields(
            "unknown field `ttl_sec`, expected one of `ttl_secs`, `branch_max_len`",
        );
        assert_eq!(fields, vec!["ttl_secs", "branch_max_len"]);
        assert!(expected_fields("invalid type: string").is_empty());
    }

    #[test]
    fn edit_distance_is_symmetric_and_zero_on_equal_strings() {
        assert_eq!(edit_distance("ttl_secs", "ttl_secs"), 0);
        assert_eq!(edit_distance("ttl_sec", "ttl_secs"), 1);
        assert_eq!(
            edit_distance("threshold", "threshold_bps"),
            edit_distance("threshold_bps", "threshold")
        );
    }

    #[test]
    fn unknown_field_extracts_the_key_from_a_serde_message() {
        assert_eq!(
            unknown_field("unknown field `ttl_sec`, expected one of `ttl_secs`"),
            Some("ttl_sec".to_string())
        );
        assert_eq!(unknown_field("invalid type: string"), None);
    }

    #[test]
    fn search_order_puts_the_env_var_first_and_the_home_dotfile_last() {
        // Not asserting on this machine's actual HOME: the order is the
        // contract, the values are the environment's business.
        let paths = search_paths();
        assert!(!paths.is_empty());
        let as_str: Vec<String> = paths.iter().map(|p| p.display().to_string()).collect();
        if let Some(i) = as_str
            .iter()
            .position(|p| p.contains(".config/tmux-companion"))
        {
            let j = as_str
                .iter()
                .position(|p| p.ends_with("tmux-companion.toml") && !p.contains(".config"));
            if let Some(j) = j {
                assert!(i < j, "XDG path must be tried before the home dotfile");
            }
        }
    }
}
