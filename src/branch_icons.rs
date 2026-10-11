//! Which glyph a branch prefix gets: the config's entries, the built-in
//! table, and for any other `word/` a lookup among the Nerd Font glyph names,
//! kept in a file so the same prefix draws the same glyph next time.
//!
//! The lookup runs once per distinct prefix word for a daemon's life and its
//! result is appended to the table every render already walks, so a render
//! pays for nothing it did not pay for before. `docs/dev/design-branch-icons.md`
//! has the rule and the reasons.

use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::Arc,
};

use serde::{Deserialize, Serialize};

use crate::config::{BranchType, Git, Preset};

/// Nerd Fonts' Material Design glyph names, sorted, one `name\tcodepoint`
/// per line after a `#` header. Written by `scripts/nerdfont-names.sh`.
const NF_MD: &str = include_str!("presets/nf-md-names.tsv");

/// The file the picks are kept in, under the daemon's state directory.
pub const PICKS_FILE: &str = "branch-icons.toml";

/// Words that are not glyph names but have an obvious one, tried before the
/// word itself.
const SYNONYMS: &[(&str, &str)] = &[
    ("cleanup", "broom"),
    ("demo", "presentation"),
    ("deploy", "rocket_launch"),
    ("infra", "server"),
    ("migration", "database_sync"),
    ("ops", "server"),
    ("user", "account"),
    ("users", "account"),
];

/// `NF_MD` without its header.
fn md_data() -> &'static str {
    let mut rest = NF_MD;
    while rest.starts_with('#') {
        match rest.find('\n') {
            Some(i) => rest = &rest[i + 1..],
            None => return "",
        }
    }
    rest
}

/// The codepoint of `nf-md-<name>`, by binary search over the embedded list.
pub fn md_codepoint(name: &str) -> Option<char> {
    let data = md_data();
    let bytes = data.as_bytes();
    // `lo` and `hi` are always line starts (or the end), so every probe backs
    // up from the middle to the start of the line it landed in.
    let (mut lo, mut hi) = (0usize, bytes.len());
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        let start = bytes[lo..mid]
            .iter()
            .rposition(|&b| b == b'\n')
            .map_or(lo, |i| lo + i + 1);
        let end = bytes[start..]
            .iter()
            .position(|&b| b == b'\n')
            .map_or(bytes.len(), |i| start + i);
        let (entry, code) = data[start..end].split_once('\t')?;
        match entry.cmp(name) {
            std::cmp::Ordering::Equal => {
                return u32::from_str_radix(code, 16).ok().and_then(char::from_u32);
            }
            std::cmp::Ordering::Less => lo = end + 1,
            std::cmp::Ordering::Greater => hi = start,
        }
    }
    None
}

/// The word in front of the first `/`, when the branch has one the lookup
/// would consider: 2 to 24 letters, digits, `-` or `_`, at least one of them
/// a letter, and something after the slash.
pub fn prefix_word(branch: &str) -> Option<&str> {
    let (word, rest) = branch.split_once('/')?;
    let ok = !rest.is_empty()
        && (2..=24).contains(&word.len())
        && word
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        && word.bytes().any(|b| b.is_ascii_alphabetic());
    ok.then_some(word)
}

/// The glyph names a word is tried as, in order. See the design note for
/// why each step is there.
pub fn candidates(word: &str) -> Vec<String> {
    let w = word.to_ascii_lowercase().replace('-', "_");
    let mut out: Vec<String> = Vec::new();
    let mut push = |c: String| {
        if c.len() >= 2 && !out.contains(&c) {
            out.push(c);
        }
    };
    if let Some((_, name)) = SYNONYMS.iter().find(|(k, _)| *k == w) {
        push((*name).to_string());
    }
    push(w.clone());
    if let Some(stem) = w.strip_suffix("ies") {
        push(format!("{stem}y"));
    }
    if let Some(stem) = w.strip_suffix("es") {
        push(stem.to_string());
    }
    if !w.ends_with("ss")
        && let Some(stem) = w.strip_suffix('s')
    {
        push(stem.to_string());
    }
    if let Some(stem) = w.strip_suffix("ed") {
        push(format!("{stem}ing"));
        push(stem.to_string());
        push(format!("{stem}e"));
    }
    if let Some(stem) = w.strip_suffix("ing") {
        push(stem.to_string());
        push(format!("{stem}e"));
    }
    push(format!("{w}s"));
    out
}

/// Whether `glyph` is one the bar already draws for something else, which a
/// branch prefix should not borrow: the md alert is the health mark.
fn on_the_bar(glyph: char) -> bool {
    crate::tmux::icons::ALL
        .iter()
        .any(|(_, g)| g.starts_with(glyph))
}

/// One prefix the lookup placed.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Pick {
    /// The prefix as the table matches it, lowercased, slash included.
    pub prefix: String,
    /// The glyph's Nerd Font name, `nf-md-parking`.
    pub name: String,
    /// Its codepoint in hex, which is what is drawn: a later Nerd Fonts that
    /// renames the glyph does not move it.
    pub codepoint: String,
}

impl Pick {
    /// The glyph, when the codepoint is one.
    pub fn glyph(&self) -> Option<char> {
        u32::from_str_radix(&self.codepoint, 16)
            .ok()
            .and_then(char::from_u32)
    }

    /// The table entry this pick draws as: the glyph and the bar's usual
    /// trailing space.
    fn entry(&self) -> Option<BranchType> {
        Some(BranchType {
            icon: format!("{} ", self.glyph()?),
            prefixes: vec![self.prefix.clone()],
        })
    }

    /// The `[[git.branch_types]]` entry that makes this pick the user's own,
    /// with the codepoint in TOML's 8-digit escape so it reads without the
    /// font.
    pub fn as_config(&self) -> String {
        format!(
            "[[git.branch_types]]\nicon = {}\nprefixes = [\"{}\"]",
            self.icon_toml(),
            self.prefix
        )
    }

    /// The icon as a TOML string with the codepoint escaped: `"\U000F00E2 "`.
    pub fn icon_toml(&self) -> String {
        format!("\"\\U{:0>8} \"", self.codepoint.to_ascii_uppercase())
    }
}

/// Look `word` up among the glyph names. `None` when no candidate is a
/// glyph the bar does not already use.
pub fn lookup(word: &str) -> Option<Pick> {
    candidates(word).into_iter().find_map(|name| {
        let glyph = md_codepoint(&name)?;
        (!on_the_bar(glyph)).then(|| Pick {
            prefix: format!("{}/", word.to_ascii_lowercase()),
            name: format!("nf-md-{name}"),
            codepoint: format!("{:x}", glyph as u32),
        })
    })
}

/// The table a render walks: the config's entries, then every built-in entry
/// the config does not repeat. First match wins, so an entry in the config
/// beats the built-in for the prefixes it names and leaves the rest alone.
pub fn effective(config: &[BranchType]) -> Vec<BranchType> {
    let mut table = config.to_vec();
    for builtin in crate::config::default_branch_types() {
        if !config.contains(&builtin) {
            table.push(builtin);
        }
    }
    table
}

/// The picks file's shape.
#[derive(Serialize, Deserialize, Debug, Default)]
#[serde(deny_unknown_fields)]
struct PicksFile {
    #[serde(default)]
    pick: Vec<Pick>,
}

const HEADER: &str = "\
# Branch-prefix glyphs tmux-companion picked by looking the prefix up among the
# Nerd Font glyph names, kept so a restart or an upgrade draws the same one.
# Delete a [[pick]], or the file, and restart the daemon to pick again. To keep
# one, `tmux-companion doctor` prints the [[git.branch_types]] entry for it.
";

/// The picks in `file`; none when it is missing or does not parse, since a
/// lost pick is only made again.
pub fn read_picks(file: &Path) -> Vec<Pick> {
    std::fs::read_to_string(file)
        .ok()
        .and_then(|text| toml::from_str::<PicksFile>(&text).ok())
        .map(|f| f.pick)
        .unwrap_or_default()
}

/// The text [`write_picks`] puts in the file.
pub fn render_picks(picks: &[Pick]) -> String {
    let body = toml::to_string(&PicksFile {
        pick: picks.to_vec(),
    })
    .unwrap_or_default();
    format!("{HEADER}\n{body}")
}

/// Write the picks through a temporary file and a rename, mode 0600, so a
/// reader never sees half a file.
pub fn write_picks(file: &Path, text: &str) -> std::io::Result<()> {
    use std::{io::Write, os::unix::fs::OpenOptionsExt};
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = file.with_extension(format!("toml.{}.tmp", std::process::id()));
    let written = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&tmp)
        .and_then(|mut f| f.write_all(text.as_bytes()))
        .and_then(|()| std::fs::rename(&tmp, file));
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    written
}

/// Where the daemon keeps its picks, when there is a state directory.
pub fn picks_path() -> Option<PathBuf> {
    crate::server::state_dir().map(|d| d.join(PICKS_FILE))
}

/// The table, the lookup and what it picked, for one daemon.
///
/// Built once from the config. Every method is synchronous and does no I/O,
/// so it can live on `ServerState`; what a new pick needs written comes back
/// to the caller to write once the lock is gone.
#[derive(Debug, Clone)]
pub struct BranchIcons {
    table: Arc<Vec<BranchType>>,
    matching: bool,
    considered: HashSet<String>,
    picks: Vec<Pick>,
    store: Option<PathBuf>,
}

impl BranchIcons {
    /// The table for a config, with no picks and nowhere to keep them.
    pub fn new(git: &Git, preset: Preset) -> Self {
        Self {
            table: Arc::new(effective(&git.branch_types)),
            // A looked-up glyph is a codepoint the ascii preset has no
            // replacement for, so under it the catch-all, which has one,
            // stands in.
            matching: git.branch_icon_matching && preset != Preset::Ascii,
            considered: HashSet::new(),
            picks: Vec::new(),
            store: None,
        }
    }

    /// Take the picks a previous daemon made, and where to keep new ones.
    ///
    /// A stored pick goes after the table, so a config or built-in entry for
    /// the same prefix still wins. `store` is `None` for a caller that reads
    /// the picks and must not write them, the no-daemon path.
    pub fn attach(&mut self, picks: Vec<Pick>, store: Option<PathBuf>) {
        if !self.matching {
            return;
        }
        let mut table = (*self.table).clone();
        for pick in &picks {
            let word = pick.prefix.trim_end_matches('/').to_string();
            if self.considered.insert(word)
                && let Some(entry) = pick.entry()
            {
                table.push(entry);
            }
        }
        self.table = Arc::new(table);
        self.picks = picks;
        self.store = store;
    }

    /// The table as it stands.
    pub fn table(&self) -> Arc<Vec<BranchType>> {
        Arc::clone(&self.table)
    }

    /// The table to draw `branch` with, looking its prefix up the first time
    /// it is seen. The second value is a file and its new text when the
    /// lookup made a pick that should be kept.
    pub fn table_for(&mut self, branch: &str) -> (Arc<Vec<BranchType>>, Option<(PathBuf, String)>) {
        let Some(word) = self.matching.then(|| prefix_word(branch)).flatten() else {
            return (self.table(), None);
        };
        let key = word.to_ascii_lowercase();
        if self.considered.contains(&key) {
            return (self.table(), None);
        }
        self.considered.insert(key);
        if self.table.iter().any(|e| e.strip(branch).is_some()) {
            return (self.table(), None);
        }
        let Some(pick) = lookup(word) else {
            return (self.table(), None);
        };
        let Some(entry) = pick.entry() else {
            return (self.table(), None);
        };
        Arc::make_mut(&mut self.table).push(entry);
        self.picks.push(pick);
        let write = self
            .store
            .clone()
            .map(|file| (file, render_picks(&self.picks)));
        (self.table(), write)
    }
}

/// What `doctor` says about the picks in `file`: how many and where, then
/// one line per pick with its glyph name and the `icon` value that keeps it
/// in a `[[git.branch_types]]` entry of your own.
pub fn doctor_lines(file: &Path) -> Vec<String> {
    let picks = read_picks(file);
    if picks.is_empty() {
        return vec![format!("no glyph-name picks yet ({})", file.display())];
    }
    let mut out = vec![format!(
        "{} picked by glyph name, kept in {}; delete it and restart to pick again",
        picks.len(),
        file.display()
    )];
    for p in &picks {
        out.push(format!(
            "{} is {}, icon = {}",
            p.prefix,
            p.name,
            p.icon_toml()
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tmux::icons;

    fn git() -> Git {
        Git::default()
    }

    #[test]
    fn the_embedded_list_is_sorted_and_whole() {
        // The binary search is only right on a sorted list, and a list that
        // lost its tail would make every late name miss.
        let names: Vec<&str> = md_data()
            .lines()
            .map(|l| l.split_once('\t').expect("a tab").0)
            .collect();
        assert!(names.len() > 6000, "{}", names.len());
        assert!(names.windows(2).all(|w| w[0] < w[1]));
    }

    #[test]
    fn every_name_is_found_by_the_search() {
        for line in md_data().lines() {
            let (name, code) = line.split_once('\t').unwrap();
            let want = char::from_u32(u32::from_str_radix(code, 16).unwrap());
            assert_eq!(md_codepoint(name), want, "{name}");
        }
        assert_eq!(md_codepoint("not_a_glyph_name"), None);
        assert_eq!(md_codepoint(""), None);
        assert_eq!(md_codepoint("zzzz"), None);
        assert_eq!(md_codepoint("a"), None);
    }

    #[test]
    fn a_prefix_word_is_a_short_word_before_a_slash() {
        assert_eq!(prefix_word("parked/old"), Some("parked"));
        assert_eq!(prefix_word("Alice-B/x"), Some("Alice-B"));
        assert_eq!(prefix_word("a/b/c"), None, "one letter is too short");
        assert_eq!(prefix_word("ab/b/c"), Some("ab"));
        assert_eq!(prefix_word("main"), None);
        assert_eq!(prefix_word("feat/"), None, "nothing after the slash");
        assert_eq!(prefix_word("123/x"), None, "no letter");
        assert_eq!(prefix_word("has space/x"), None);
        assert_eq!(prefix_word(&format!("{}/x", "w".repeat(25))), None);
    }

    #[test]
    fn candidates_come_in_a_fixed_order() {
        assert_eq!(
            candidates("parked"),
            ["parked", "parking", "park", "parke", "parkeds"]
        );
        assert_eq!(
            candidates("Stories"),
            ["stories", "story", "stori", "storie", "storiess"]
        );
        assert_eq!(candidates("users")[0], "account");
        assert_eq!(candidates("my-thing")[0], "my_thing");
        assert_eq!(candidates("class"), ["class", "classs"]);
    }

    #[test]
    fn common_prefixes_resolve_the_same_way_every_time() {
        // What the design note promises, pinned so a new list or a changed
        // rule shows up here rather than on somebody's bar.
        let name = |w: &str| lookup(w).map(|p| p.name);
        // parking is PARKED's glyph, so the lookup leaves it to the table.
        assert_eq!(name("parked"), None);
        assert_eq!(name("locked").as_deref(), Some("nf-md-lock"));
        assert_eq!(name("cleanup").as_deref(), Some("nf-md-broom"));
        assert_eq!(name("deploy").as_deref(), Some("nf-md-rocket_launch"));
        assert_eq!(name("infra").as_deref(), Some("nf-md-server"));
        assert_eq!(name("api").as_deref(), Some("nf-md-api"));
        assert_eq!(name("web").as_deref(), Some("nf-md-web"));
        assert_eq!(name("database").as_deref(), Some("nf-md-database"));
        assert_eq!(name("clouds").as_deref(), Some("nf-md-cloud"));
        assert_eq!(name("update").as_deref(), Some("nf-md-update"));
        assert_eq!(name("hack"), None);
        assert_eq!(name("alice"), None);
        assert_eq!(name("db"), None);
    }

    #[test]
    fn a_glyph_the_bar_already_draws_is_never_picked() {
        // md-bell_alert_outline is the health mark, md-bell_off_outline
        // quiet hours and md-robot the agent count; a branch named after
        // one must not look like it, nor like a built-in branch type
        // (hammer/ is not build/).
        assert_eq!(lookup("bell-alert-outline"), None);
        assert_eq!(lookup("bell-off-outline"), None);
        assert_eq!(lookup("robot"), None);
        assert_eq!(lookup("hammer"), None);
        let pick = lookup("Cleanup").unwrap();
        assert_eq!(pick.glyph(), Some('\u{f00e2}'));
        assert_eq!(pick.prefix, "cleanup/");
    }

    #[test]
    fn the_branch_glyphs_are_their_own() {
        // Every glyph the built-in table draws is a codepoint no other
        // constant uses, so a branch type never reads as a file count or a
        // warning.
        let table = crate::config::default_branch_types();
        for entry in &table {
            let glyph = crate::config::expand_glyphs(&entry.icon);
            let owners: Vec<&str> = icons::ALL
                .iter()
                .filter(|(_, g)| *g == glyph)
                .map(|(n, _)| *n)
                .collect();
            assert_eq!(owners.len(), 1, "{} is shared by {owners:?}", entry.icon);
        }
        let prefixed: Vec<_> = icons::ALL
            .iter()
            .filter(|(_, g)| *g == icons::PREFIXED)
            .collect();
        assert_eq!(prefixed.len(), 1);
    }

    #[test]
    fn every_branch_glyph_has_an_ascii_form() {
        let ascii = Preset::Ascii.table();
        for entry in crate::config::default_branch_types() {
            let name = entry.icon.trim_matches(|c| c == '{' || c == '}');
            assert!(ascii.contains_key(name), "ascii.toml has no {name}");
        }
        assert!(ascii.contains_key("PREFIXED"));
    }

    #[test]
    fn the_config_overrides_and_the_builtins_stay() {
        let mine = BranchType {
            icon: "P ".into(),
            prefixes: vec!["parked/".into()],
        };
        let table = effective(std::slice::from_ref(&mine));
        assert_eq!(table[0], mine);
        assert_eq!(table.len(), 1 + crate::config::default_branch_types().len());
        // The default list repeated in full is not doubled.
        assert_eq!(
            effective(&crate::config::default_branch_types()),
            crate::config::default_branch_types()
        );
    }

    #[test]
    fn a_copy_of_the_old_six_is_the_old_six() {
        // The old advice was to copy the six and add to them; that file must
        // draw those six exactly as before.
        let six: Vec<BranchType> = crate::config::default_branch_types()[..6].to_vec();
        let table = effective(&six);
        assert_eq!(&table[..6], &six[..]);
    }

    #[test]
    fn a_new_prefix_is_looked_up_once_and_handed_back_to_write() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(PICKS_FILE);
        let mut icons = BranchIcons::new(&git(), Preset::NerdFontV3);
        icons.attach(Vec::new(), Some(file.clone()));
        let before = icons.table().len();

        let (table, write) = icons.table_for("cleanup/old-stuff");
        assert_eq!(table.len(), before + 1);
        let (path, text) = write.expect("a pick to keep");
        assert_eq!(path, file);
        assert!(text.contains("nf-md-broom"), "{text}");

        // The second sighting is a hash probe and nothing else.
        let (again, write) = icons.table_for("cleanup/other");
        assert!(Arc::ptr_eq(&table, &again));
        assert!(write.is_none());

        // A word the table claims, and one nothing places, write nothing.
        assert!(icons.table_for("docs/readme").1.is_none());
        assert!(icons.table_for("alice/topic").1.is_none());
        assert!(icons.table_for("main").1.is_none());
    }

    #[test]
    fn picks_survive_a_restart_even_when_the_list_moves() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(PICKS_FILE);
        let mut first = BranchIcons::new(&git(), Preset::NerdFontV3);
        first.attach(read_picks(&file), Some(file.clone()));
        let (_, write) = first.table_for("cleanup/x");
        let (path, text) = write.unwrap();
        write_picks(&path, &text).unwrap();

        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&file).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);

        // A stored codepoint the current list no longer has is drawn as
        // stored: that is the point of keeping it.
        let mut picks = read_picks(&file);
        assert_eq!(picks.len(), 1);
        picks[0].codepoint = "f1aef".into();
        write_picks(&file, &render_picks(&picks)).unwrap();

        let mut second = BranchIcons::new(&git(), Preset::NerdFontV3);
        second.attach(read_picks(&file), Some(file.clone()));
        let (table, write) = second.table_for("cleanup/y");
        assert!(write.is_none(), "already known, not looked up again");
        let entry = table
            .iter()
            .find(|e| e.strip("cleanup/y").is_some())
            .unwrap();
        assert_eq!(entry.icon, "\u{f1aef} ");
    }

    #[test]
    fn a_config_entry_beats_a_stored_pick() {
        let mut git = git();
        git.branch_types.insert(
            0,
            BranchType {
                icon: "C ".into(),
                prefixes: vec!["cleanup/".into()],
            },
        );
        let mut icons = BranchIcons::new(&git, Preset::NerdFontV3);
        icons.attach(vec![lookup("cleanup").unwrap()], None);
        let (table, _) = icons.table_for("cleanup/x");
        let first = table
            .iter()
            .find(|e| e.strip("cleanup/x").is_some())
            .unwrap();
        assert_eq!(first.icon, "C ");
    }

    #[test]
    fn matching_off_or_ascii_reads_and_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(PICKS_FILE);
        let mut off = git();
        off.branch_icon_matching = false;
        for mut icons in [
            BranchIcons::new(&off, Preset::NerdFontV3),
            BranchIcons::new(&git(), Preset::Ascii),
        ] {
            let before = icons.table().len();
            icons.attach(vec![lookup("cleanup").unwrap()], Some(file.clone()));
            let (table, write) = icons.table_for("deploy/x");
            assert_eq!(table.len(), before);
            assert!(write.is_none());
        }
        assert!(!file.exists());
    }

    #[test]
    fn doctor_says_where_the_picks_are_and_how_to_keep_one() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(PICKS_FILE);
        assert!(doctor_lines(&file)[0].starts_with("no glyph-name picks yet"));
        write_picks(&file, &render_picks(&[lookup("cleanup").unwrap()])).unwrap();
        let lines = doctor_lines(&file);
        assert!(lines[0].starts_with("1 picked by glyph name"), "{lines:?}");
        assert_eq!(lines[1], "cleanup/ is nf-md-broom, icon = \"\\U000F00E2 \"");
        // No glyph in what doctor prints: it is read aloud and pasted into
        // issues.
        assert!(lines.iter().all(|l| l.is_ascii()), "{lines:?}");
    }

    #[test]
    fn a_broken_picks_file_is_no_picks() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(PICKS_FILE);
        std::fs::write(&file, "this is [not toml").unwrap();
        assert!(read_picks(&file).is_empty());
        assert!(read_picks(&dir.path().join("missing.toml")).is_empty());
    }

    #[test]
    fn a_pick_promotes_to_a_config_entry_that_parses() {
        let pick = lookup("cleanup").unwrap();
        let config = crate::config::parse(&pick.as_config(), Path::new("t.toml")).unwrap();
        let entry = &config.git.branch_types[0];
        assert_eq!(entry.icon, "\u{f00e2} ");
        assert_eq!(entry.prefixes, ["cleanup/"]);
    }

    #[test]
    fn the_lookup_is_cheap_enough_to_run_once_per_word() {
        // Not a benchmark: a ceiling that a linear scan or a per-call parse
        // would blow through, and that a binary search sits far under.
        let start = std::time::Instant::now();
        for _ in 0..1000 {
            std::hint::black_box(lookup(std::hint::black_box("cleanup")));
        }
        let per = start.elapsed() / 1000;
        assert!(
            per < std::time::Duration::from_millis(1),
            "{per:?} per lookup"
        );
    }
}
