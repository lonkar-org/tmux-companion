//! A snapshot that can move house.
//!
//! A generation in the store names directories on one machine, absolute, and
//! the project map that gives each session its colour lives in another file
//! under another directory. `sessions export` writes the two into one TOML
//! with every path under `$HOME` spelled `~/...`, and `sessions import` on the
//! other machine reads it back with that machine's home, stores it as a new
//! generation, and adds the map rows it does not have. Then `sessions
//! resurrect` does what it always does, and says which directories are not
//! there.
//!
//! What is not carried: what was on each pane's screen. That is a directory
//! of files per generation and it is the half that holds whatever you printed.

use serde::{Deserialize, Serialize};

use super::{Pane, Session, Snapshot, Window};

/// The format this writes, checked on the way back in.
pub const FORMAT: u32 = 1;

/// The file.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Portable {
    /// Who wrote it and when.
    pub export: ExportHeader,
    /// The generation, with `~` where the home directory was.
    pub snapshot: Snapshot,
    /// Project name to theme name, from `_project-map.tsv`.
    #[serde(default)]
    pub project_map: Vec<MapRow>,
}

/// Who wrote it and when.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct ExportHeader {
    /// [`FORMAT`] as it was when this was written.
    pub format: u32,
    /// The machine the snapshot was taken on.
    #[serde(default)]
    pub exported_from: String,
    /// When the export was written, as `format_unix` writes it.
    #[serde(default)]
    pub exported_at: String,
    /// The generation it came from.
    #[serde(default)]
    pub stamp: String,
}

/// One row of the project map.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MapRow {
    /// The project, which is the session name.
    pub project: String,
    /// The theme's file stem.
    pub theme: String,
}

/// A path with `home` spelled `~`, or unchanged when it is not under it.
pub fn relative(path: &str, home: &str) -> String {
    let home = home.trim_end_matches('/');
    if home.is_empty() {
        return path.to_string();
    }
    if path == home {
        return "~".to_string();
    }
    match path.strip_prefix(home) {
        Some(rest) if rest.starts_with('/') => format!("~{rest}"),
        _ => path.to_string(),
    }
}

/// The inverse of [`relative`], against this machine's home.
pub fn absolute(path: &str, home: &str) -> String {
    if path == "~" {
        return home.to_string();
    }
    match path.strip_prefix("~/") {
        Some(rest) => format!("{}/{rest}", home.trim_end_matches('/')),
        None => path.to_string(),
    }
}

/// The snapshot with every path passed through `f`.
fn map_paths(snap: &Snapshot, f: &dyn Fn(&str) -> String) -> Snapshot {
    Snapshot {
        header: snap.header.clone(),
        session: snap
            .session
            .iter()
            .map(|s| Session {
                name: s.name.clone(),
                path: f(&s.path),
                window: s
                    .window
                    .iter()
                    .map(|w| Window {
                        pane: w
                            .pane
                            .iter()
                            .map(|p| Pane {
                                cwd: f(&p.cwd),
                                ..p.clone()
                            })
                            .collect(),
                        ..w.clone()
                    })
                    .collect(),
            })
            .collect(),
    }
}

/// The snapshot as it travels.
///
/// The home is tried as written and with every symlink resolved, because
/// macOS puts `/var` behind `/private/var` and tmux reports a pane's path
/// resolved while `$HOME` is not: without both spellings nothing under a
/// home like that ever matched, and the export carried absolute paths.
pub fn relativize(snap: &Snapshot, home: &str) -> Snapshot {
    let mut homes = vec![home.to_string()];
    if let Ok(real) = std::fs::canonicalize(home)
        && real.display().to_string() != home
    {
        homes.push(real.display().to_string());
    }
    map_paths(snap, &|p| {
        homes
            .iter()
            .map(|h| relative(p, h))
            .find(|r| r.starts_with('~'))
            .unwrap_or_else(|| p.to_string())
    })
}

/// The snapshot as it lands.
pub fn absolutize(snap: &Snapshot, home: &str) -> Snapshot {
    map_paths(snap, &|p| absolute(p, home))
}

/// The file's text.
pub fn render(p: &Portable) -> String {
    let body = toml::to_string_pretty(p).unwrap_or_default();
    format!(
        "# Written by tmux-companion sessions export. Paths under the home\n\
         # directory are spelled ~ so this reads on another machine; what was on\n\
         # each pane's screen is not carried. `tmux-companion sessions import`\n\
         # stores it as a new generation there.\n\n{body}"
    )
}

/// The file, read back, refusing a format this build does not know.
pub fn parse(text: &str) -> anyhow::Result<Portable> {
    let p: Portable = toml::from_str(text)?;
    if p.export.format > FORMAT {
        anyhow::bail!(
            "export format {}, this build reads {}; a newer tmux-companion wrote it",
            p.export.format,
            FORMAT
        );
    }
    if p.snapshot.session.is_empty() {
        anyhow::bail!("the export holds no sessions");
    }
    Ok(p)
}

/// The rows to add: those whose project the existing map does not name.
///
/// A project already mapped keeps its colour; the person on this machine
/// picked it, and an import is not a reason to change it.
pub fn map_additions(
    existing: &std::collections::HashMap<String, String>,
    incoming: &[MapRow],
) -> Vec<MapRow> {
    incoming
        .iter()
        .filter(|r| !existing.contains_key(&r.project))
        .filter(|r| !r.project.trim().is_empty() && !r.theme.trim().is_empty())
        .cloned()
        .collect()
}

/// The project map as rows, from its file's text.
pub fn map_rows(text: &str) -> Vec<MapRow> {
    let mut rows: Vec<MapRow> = crate::project::parse_project_map(text)
        .into_iter()
        .map(|(project, theme)| MapRow { project, theme })
        .collect();
    rows.sort_by(|a, b| a.project.cmp(&b.project));
    rows
}

/// `sessions export`: the newest generation, or the one named, as a file.
pub async fn export(stamp: Option<String>, file: Option<String>) -> anyhow::Result<()> {
    let state_dir = crate::server::state_dir()
        .ok_or_else(|| anyhow::anyhow!("no state directory: neither XDG_STATE_HOME nor HOME"))?;
    let home = std::env::var("HOME").unwrap_or_default();
    let stamp = match stamp {
        Some(s) => s,
        None => super::store::last_stamp_in(&state_dir).ok_or_else(|| {
            anyhow::anyhow!("no snapshot yet; run tmux-companion sessions save first")
        })?,
    };
    let snap = super::store::load_in(&state_dir, &stamp)?;
    let map_text =
        std::fs::read_to_string(crate::project::project_map_path(&home)).unwrap_or_default();
    let portable = Portable {
        export: ExportHeader {
            format: FORMAT,
            exported_from: snap.header.hostname.clone(),
            exported_at: crate::tasks::format_unix(crate::panes::now_secs() as i64),
            stamp: stamp.clone(),
        },
        snapshot: relativize(&snap, &home),
        project_map: map_rows(&map_text),
    };
    let text = render(&portable);
    match file.as_deref().filter(|f| *f != "-") {
        Some(path) => {
            std::fs::write(path, &text)?;
            println!(
                "wrote {path}: {} session{}, {} pane{}, {} map row{}, from generation {stamp}",
                portable.snapshot.session.len(),
                if portable.snapshot.session.len() == 1 {
                    ""
                } else {
                    "s"
                },
                portable.snapshot.pane_count(),
                if portable.snapshot.pane_count() == 1 {
                    ""
                } else {
                    "s"
                },
                portable.project_map.len(),
                if portable.project_map.len() == 1 {
                    ""
                } else {
                    "s"
                },
            );
        }
        None => print!("{text}"),
    }
    Ok(())
}

/// `sessions import`: a file from [`export`], stored here as a new generation.
pub async fn import(file: String, no_map: bool) -> anyhow::Result<()> {
    let state_dir = crate::server::state_dir()
        .ok_or_else(|| anyhow::anyhow!("no state directory: neither XDG_STATE_HOME nor HOME"))?;
    let home = std::env::var("HOME").unwrap_or_default();
    let text = std::fs::read_to_string(&file).map_err(|e| anyhow::anyhow!("{file}: {e}"))?;
    let portable = parse(&text)?;
    let mut snap = absolutize(&portable.snapshot, &home);
    snap.header.imported_from = format!(
        "{file} (from {}, generation {})",
        portable.export.exported_from, portable.export.stamp
    );
    let config = crate::cli::config_or_default();
    // A stamp is a second, and an import in the same second as a save would
    // take that generation's name and overwrite it; step forward until free.
    let mut now = crate::panes::now_secs() as i64;
    while super::store::path_in(&state_dir, &super::store::stamp_from(now)).exists() {
        now += 1;
    }
    let stamp = super::store::stamp_from(now);
    let written = super::store::store_in(
        &state_dir,
        &snap,
        &stamp,
        config.sessions.keep,
        config.sessions.keep_days,
        now,
    )?;
    println!(
        "stored {} as generation {stamp}: {} session{}, {} pane{}",
        written.display(),
        snap.session.len(),
        if snap.session.len() == 1 { "" } else { "s" },
        snap.pane_count(),
        if snap.pane_count() == 1 { "" } else { "s" },
    );

    if !no_map && !portable.project_map.is_empty() {
        let map_path = crate::project::project_map_path(&home);
        let existing_text = std::fs::read_to_string(&map_path).unwrap_or_default();
        let existing = crate::project::parse_project_map(&existing_text);
        let added = map_additions(&existing, &portable.project_map);
        if !added.is_empty() {
            use std::io::Write;
            if let Some(dir) = map_path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            let mut f = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&map_path)?;
            if !existing_text.is_empty() && !existing_text.ends_with('\n') {
                writeln!(f)?;
            }
            for r in &added {
                writeln!(f, "{}\t{}", r.project, r.theme)?;
            }
            println!(
                "added {} project colour{} to {}; the themes themselves come from `theme init`",
                added.len(),
                if added.len() == 1 { "" } else { "s" },
                map_path.display()
            );
        }
    }
    println!("now: tmux-companion sessions resurrect");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sessions::Header;

    fn snap() -> Snapshot {
        Snapshot {
            header: Header {
                hostname: "laptop".into(),
                ..Header::default()
            },
            session: vec![Session {
                name: "api".into(),
                path: "/Users/me/w/api".into(),
                window: vec![Window {
                    index: 1,
                    name: "edit".into(),
                    pane: vec![
                        Pane {
                            index: 1,
                            cwd: "/Users/me/w/api/src".into(),
                            command: "nvim".into(),
                            ..Pane::default()
                        },
                        Pane {
                            index: 2,
                            cwd: "/tmp".into(),
                            ..Pane::default()
                        },
                    ],
                    ..Window::default()
                }],
            }],
        }
    }

    #[test]
    fn paths_under_home_travel_as_tilde_and_come_back_under_the_new_home() {
        assert_eq!(relative("/Users/me/w/api", "/Users/me"), "~/w/api");
        assert_eq!(relative("/Users/me", "/Users/me"), "~");
        assert_eq!(
            relative("/Users/meow/x", "/Users/me"),
            "/Users/meow/x",
            "a prefix is not a parent"
        );
        assert_eq!(relative("/tmp", "/Users/me"), "/tmp");
        assert_eq!(absolute("~/w/api", "/home/me"), "/home/me/w/api");
        assert_eq!(absolute("~", "/home/me"), "/home/me");
        assert_eq!(absolute("/tmp", "/home/me"), "/tmp");
    }

    #[test]
    fn a_snapshot_round_trips_through_another_home() {
        let travelling = relativize(&snap(), "/Users/me");
        assert_eq!(travelling.session[0].path, "~/w/api");
        assert_eq!(travelling.session[0].window[0].pane[0].cwd, "~/w/api/src");
        assert_eq!(travelling.session[0].window[0].pane[1].cwd, "/tmp");
        let landed = absolutize(&travelling, "/home/me");
        assert_eq!(landed.session[0].path, "/home/me/w/api");
        assert_eq!(
            landed.session[0].window[0].pane[0].cwd,
            "/home/me/w/api/src"
        );
        assert_eq!(
            landed.session[0].window[0].pane[0].command, "nvim",
            "everything else is untouched"
        );
    }

    #[test]
    fn the_file_reads_back_and_a_newer_format_is_refused() {
        let p = Portable {
            export: ExportHeader {
                format: FORMAT,
                exported_from: "laptop".into(),
                exported_at: "2026-09-26 15:00:00 UTC".into(),
                stamp: "20260926T150000".into(),
            },
            snapshot: relativize(&snap(), "/Users/me"),
            project_map: vec![MapRow {
                project: "api".into(),
                theme: "ink".into(),
            }],
        };
        let text = render(&p);
        assert!(text.contains("path = \"~/w/api\""), "{text}");
        assert_eq!(parse(&text).unwrap(), p);
        let newer = text.replace("format = 1", "format = 9");
        let err = parse(&newer).unwrap_err().to_string();
        assert!(err.contains("export format 9"), "{err}");
    }

    #[test]
    fn only_unmapped_projects_take_a_colour_from_the_import() {
        let existing = std::collections::HashMap::from([("api".to_string(), "plum".to_string())]);
        let incoming = vec![
            MapRow {
                project: "api".into(),
                theme: "ink".into(),
            },
            MapRow {
                project: "web".into(),
                theme: "sand".into(),
            },
            MapRow {
                project: "".into(),
                theme: "sand".into(),
            },
        ];
        let added = map_additions(&existing, &incoming);
        assert_eq!(
            added,
            vec![MapRow {
                project: "web".into(),
                theme: "sand".into()
            }]
        );
        assert_eq!(map_rows("api\tplum\n# c\nweb\tsand\n").len(), 2);
    }
}
