//! Where the layers disagree: a key tmux's root table binds that an app in a
//! pane binds too, and the keys in scope nobody has taken yet.
//!
//! Keys are compared through [`spell::canonical`], so tmux's `M-S-a` and an
//! app's `M-A` count as the one key a terminal sends.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use super::discover::{Discovered, Row, Rung};
use super::spell;

/// One app's claim on a key: the modes it holds it in and what it does there.
#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
pub struct Claim {
    /// The layer, `nvim`, `nano`.
    pub layer: String,
    /// The modes it is bound in, empty for every mode.
    pub modes: Vec<String>,
    /// The first description any of its rows gave.
    pub desc: String,
    /// The best rung that found it.
    pub rung: Rung,
}

/// A key tmux's root table binds and at least one app binds too.
#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
pub struct Collision {
    /// The key as tmux's root table spells it.
    pub key: String,
    /// What tmux does with it: the note, or the command when there is none.
    pub tmux: String,
    /// Every app that wants it.
    pub apps: Vec<Claim>,
    /// How the registry settles it: `hold`, `tmux` or `app`, and whether that
    /// was written or is the default.
    pub route: String,
}

/// A key the registry says one thing about and a layer does another.
#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
pub struct Drift {
    /// The key as the registry spells it.
    pub key: String,
    /// The layer, `tmux`, `nvim`.
    pub layer: String,
    /// What the registry expects to find there.
    pub expected: String,
    /// What the layer binds the key to, empty when it binds nothing.
    pub found: Vec<String>,
}

/// The whole report.
#[derive(Serialize, Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    /// Keys tmux and an app both bind.
    pub collisions: Vec<Collision>,
    /// Keys a layer binds to something other than what the registry says.
    pub drift: Vec<Drift>,
    /// Keys in the common part of the scope that no layer binds.
    pub free: Vec<String>,
    /// What discovery could not read.
    pub notes: Vec<String>,
}

/// The keys `free` is chosen from: Alt and Ctrl on a letter or digit, Alt on a
/// capital, and F1 to F12. Wider combinations exist, but a free key is a
/// suggestion for a new shared action, and these are the ones a hand reaches.
pub fn common_keys() -> Vec<String> {
    let mut keys = Vec::new();
    for c in ('a'..='z').chain('0'..='9') {
        keys.push(format!("M-{c}"));
    }
    for c in 'A'..='Z' {
        keys.push(format!("M-{c}"));
    }
    for c in 'a'..='z' {
        keys.push(format!("C-{c}"));
    }
    for n in 1..=12 {
        keys.push(format!("F{n}"));
    }
    keys
}

/// Build the report from a discovery and the registry.
pub fn report(found: &Discovered, registry: &BTreeMap<String, crate::config::KeyEntry>) -> Report {
    let entries: BTreeMap<String, &crate::config::KeyEntry> = registry
        .iter()
        .map(|(k, e)| (spell::canonical(k), e))
        .collect();
    let mut tmux: BTreeMap<String, &Row> = BTreeMap::new();
    let mut apps: BTreeMap<String, BTreeMap<String, Vec<&Row>>> = BTreeMap::new();
    let mut taken = BTreeSet::new();
    for row in &found.rows {
        let key = spell::canonical(&row.key);
        taken.insert(key.clone());
        if row.layer == "tmux" {
            tmux.entry(key).or_insert(row);
        } else {
            apps.entry(key)
                .or_default()
                .entry(row.layer.clone())
                .or_default()
                .push(row);
        }
    }

    let collisions = tmux
        .iter()
        .filter_map(|(key, t)| {
            let layers = apps.get(key)?;
            Some(Collision {
                key: t.key.clone(),
                tmux: if t.desc.is_empty() {
                    t.source.clone()
                } else {
                    t.desc.clone()
                },
                apps: layers
                    .iter()
                    .map(|(layer, rows)| claim(layer, rows))
                    .collect(),
                route: entries
                    .get(key)
                    .map_or("hold, the default".to_string(), |e| {
                        e.route.as_str().to_string()
                    }),
            })
        })
        .collect();

    let free = common_keys()
        .into_iter()
        .filter(|k| !taken.contains(&spell::canonical(k)))
        .collect();

    let drift = drift(&found.rows, registry);

    Report {
        collisions,
        drift,
        free,
        notes: found.notes.clone(),
    }
}

/// Every expectation in the registry that discovery doesn't bear out.
///
/// A case-insensitive substring of the binding's note or description, then of
/// its command: an exact match would break on every reworded description.
pub fn drift(rows: &[Row], registry: &BTreeMap<String, crate::config::KeyEntry>) -> Vec<Drift> {
    let mut out = Vec::new();
    for (key, entry) in registry {
        let want = spell::canonical(key);
        for (layer, expected) in &entry.expect {
            let bound: Vec<&Row> = rows
                .iter()
                .filter(|r| &r.layer == layer && spell::canonical(&r.key) == want)
                .collect();
            let needle = expected.to_lowercase();
            let met = bound.iter().any(|r| {
                r.desc.to_lowercase().contains(&needle) || r.source.to_lowercase().contains(&needle)
            });
            if !met {
                let mut found: Vec<String> = bound
                    .iter()
                    .map(|r| {
                        if r.desc.is_empty() {
                            r.source.clone()
                        } else {
                            r.desc.clone()
                        }
                    })
                    .collect();
                found.sort();
                found.dedup();
                out.push(Drift {
                    key: key.clone(),
                    layer: layer.clone(),
                    expected: expected.clone(),
                    found,
                });
            }
        }
    }
    out
}

/// One layer's rows for a key, folded into one claim.
fn claim(layer: &str, rows: &[&Row]) -> Claim {
    let mut modes: Vec<String> = rows.iter().map(|r| r.mode.clone()).collect();
    modes.sort();
    modes.dedup();
    // a declared claim holds in every mode, which swallows the rest
    if modes.iter().any(String::is_empty) {
        modes.clear();
    }
    Claim {
        layer: layer.to_string(),
        modes,
        desc: rows
            .iter()
            .map(|r| r.desc.as_str())
            .find(|d| !d.is_empty())
            .unwrap_or("")
            .to_string(),
        rung: rows.iter().map(|r| r.rung).min().unwrap_or(Rung::Declared),
    }
}

/// How a claim reads on one line: `nvim n,x  nvim-tree: open`.
pub fn claim_line(c: &Claim) -> String {
    let modes = if c.modes.is_empty() {
        "all".to_string()
    } else {
        c.modes.join(",")
    };
    let mut line = format!("{} {modes}", c.layer);
    if !c.desc.is_empty() {
        line.push_str("  ");
        line.push_str(&c.desc);
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(layer: &str, mode: &str, key: &str, desc: &str, rung: Rung) -> Row {
        Row {
            layer: layer.into(),
            mode: mode.into(),
            key: key.into(),
            desc: desc.into(),
            source: "cmd".into(),
            rung,
            pane: String::new(),
        }
    }

    fn found(rows: Vec<Row>) -> Discovered {
        Discovered {
            rows,
            ..Default::default()
        }
    }

    #[test]
    fn a_key_both_bind_is_a_collision_with_every_app_listed() {
        let r = report(
            &found(vec![
                row("tmux", "root", "M-a", "companion: next window", Rung::Live),
                row("nvim", "n", "M-a", "", Rung::Live),
                row("nvim", "x", "M-a", "select all", Rung::Live),
                row("nano", "", "M-a", "", Rung::Declared),
                row("nvim", "n", "M-q", "quit", Rung::Live),
            ]),
            &BTreeMap::new(),
        );
        assert_eq!(r.collisions.len(), 1);
        let c = &r.collisions[0];
        assert_eq!(c.key, "M-a");
        assert_eq!(c.tmux, "companion: next window");
        assert_eq!(c.apps.len(), 2);
        assert_eq!(c.apps[0].layer, "nano");
        assert!(c.apps[0].modes.is_empty());
        assert_eq!(c.apps[1].modes, ["n", "x"]);
        assert_eq!(c.apps[1].desc, "select all");
    }

    #[test]
    fn spellings_of_one_key_collide() {
        let r = report(
            &found(vec![
                row("tmux", "root", "M-S-a", "", Rung::Live),
                row("nvim", "n", "M-A", "", Rung::Live),
            ]),
            &BTreeMap::new(),
        );
        assert_eq!(r.collisions.len(), 1);
        assert_eq!(
            r.collisions[0].key, "M-S-a",
            "shown as tmux spells its own binding"
        );
        assert_eq!(
            r.collisions[0].tmux, "cmd",
            "no note, the command stands in"
        );
    }

    #[test]
    fn a_key_only_one_layer_binds_is_not_a_collision_and_not_free() {
        let r = report(
            &found(vec![
                row("tmux", "root", "M-s", "", Rung::Live),
                row("nvim", "n", "M-q", "", Rung::Live),
            ]),
            &BTreeMap::new(),
        );
        assert!(r.collisions.is_empty());
        assert!(!r.free.contains(&"M-s".to_string()));
        assert!(!r.free.contains(&"M-q".to_string()));
        assert!(r.free.contains(&"M-z".to_string()));
        assert!(r.free.contains(&"F12".to_string()));
    }

    #[test]
    fn the_registry_names_the_route_and_drift_is_what_does_not_match() {
        use crate::config::{KeyEntry, KeyRoute};
        let mut registry = BTreeMap::new();
        registry.insert(
            "M-1".to_string(),
            KeyEntry {
                route: KeyRoute::App,
                expect: BTreeMap::from([
                    ("tmux".to_string(), "choose-tree".to_string()),
                    ("nvim".to_string(), "nvimtreetoggle".to_string()),
                ]),
                ..Default::default()
            },
        );
        registry.insert(
            "M-q".to_string(),
            KeyEntry {
                expect: BTreeMap::from([("nvim".to_string(), "quit".to_string())]),
                ..Default::default()
            },
        );
        let r = report(
            &found(vec![
                row("tmux", "root", "M-1", "", Rung::Live),
                row("nvim", "n", "M-1", "", Rung::Live),
                row("nvim", "n", "M-q", "Quit the window", Rung::Live),
            ]),
            &registry,
        );
        assert_eq!(r.collisions[0].route, "app");
        // tmux's M-1 runs "cmd", not choose-tree; nvim's says nothing and runs "cmd"
        let drifted: Vec<(&str, &str)> = r
            .drift
            .iter()
            .map(|d| (d.key.as_str(), d.layer.as_str()))
            .collect();
        assert_eq!(drifted, [("M-1", "nvim"), ("M-1", "tmux")]);
        assert_eq!(r.drift[0].found, ["cmd"]);
    }

    #[test]
    fn drift_with_nothing_bound_says_so() {
        let registry = BTreeMap::from([(
            "M-z".to_string(),
            crate::config::KeyEntry {
                expect: BTreeMap::from([("nvim".to_string(), "zen".to_string())]),
                ..Default::default()
            },
        )]);
        let d = drift(&[], &registry);
        assert_eq!(d.len(), 1);
        assert!(d[0].found.is_empty());
    }

    #[test]
    fn the_best_rung_is_kept() {
        let rows = [
            row("nvim", "n", "M-a", "", Rung::Config),
            row("nvim", "i", "M-a", "", Rung::Live),
        ];
        let refs: Vec<&Row> = rows.iter().collect();
        assert_eq!(claim("nvim", &refs).rung, Rung::Live);
    }

    #[test]
    fn a_claim_reads_as_one_line() {
        let c = Claim {
            layer: "nvim".into(),
            modes: vec!["n".into(), "x".into()],
            desc: "tree".into(),
            rung: Rung::Live,
        };
        assert_eq!(claim_line(&c), "nvim n,x  tree");
        let all = Claim {
            modes: vec![],
            desc: String::new(),
            ..c
        };
        assert_eq!(claim_line(&all), "nvim all");
    }
}
