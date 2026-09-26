//! The client half of `theme`: the subcommands that read a themes directory,
//! run the picker and source a file into tmux, which talk to the terminal and
//! so run in the client rather than the daemon.

use crate::cli::{ThemeAction, config_or_default};

/// `theme gen`.
pub fn run(action: ThemeAction) -> anyhow::Result<()> {
    let (apply, shades, themes, background): (bool, Option<String>, _, _) = match action {
        ThemeAction::Gen {
            apply,
            shades,
            themes,
            background,
        } => (apply, shades, themes, background),
        ThemeAction::Pick {
            target,
            register,
            themes,
            print,
        } => return theme_pick(target, register, &themes_dir_or(themes), print),
        ThemeAction::Apply {
            session,
            target,
            themes,
            all,
        } => {
            let dir = themes_dir_or(themes);
            if all {
                // Sourcing tmux.conf resets the global options a theme sets,
                // so a reload leaves every session painted with whatever the
                // file says rather than with its own colour. One pass over
                // the session list puts them all back.
                let listed = std::process::Command::new("tmux")
                    .args(["list-sessions", "-F", "#{session_name}"])
                    .output()
                    .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
                    .unwrap_or_default();
                for name in listed.lines().map(str::trim).filter(|l| !l.is_empty()) {
                    theme_apply(name, Some(name.to_string()), &dir)?;
                }
                return Ok(());
            }
            let Some(session) = session else {
                anyhow::bail!("theme apply needs a session, or --all");
            };
            return theme_apply(&session, target, &dir);
        }
        ThemeAction::Init { themes } => return theme_init(&themes_dir_or(themes)),
        ThemeAction::Add {
            bg,
            fg,
            name,
            force,
            themes,
        } => return theme_add(&bg, fg, name, force, &themes_dir_or(themes)),
        ThemeAction::ListColours { print } => return theme_list_colours(print),
    };
    // Named rather than numbered, and resolved once: an unknown rung is an
    // error here rather than a silent fall back to the default, because a
    // typo that quietly generates 148 themes instead of 18 is a directory
    // somebody has to clean up by hand.
    let level_name = shades.clone().unwrap_or_default();
    let level = match &shades {
        None => None,
        Some(name) => match crate::theme::ShadeLevel::from_name(name) {
            Some(level) => Some(level),
            None => anyhow::bail!(
                "unknown --shades level `{name}`, expected one of {}",
                crate::theme::SHADE_LEVELS.join(", ")
            ),
        },
    };

    let dir = themes_dir_or(themes);
    let (bg, source) = match background.as_deref().map(crate::theme::parse_hex) {
        Some(Some(c)) => (c, "--background".to_string()),
        Some(None) => anyhow::bail!("--background wants #rrggbb"),
        None => crate::theme::terminal_background(),
    };

    let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(&dir)
        .map_err(|e| anyhow::anyhow!("{}: {e}", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "tmux"))
        .filter(|p| {
            !p.file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with('_'))
        })
        .collect();
    files.sort();

    let mut themes_parsed = Vec::new();
    for path in &files {
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        if let Some(t) = crate::theme::parse_theme(path, &text) {
            themes_parsed.push(t);
        }
    }

    println!("{} themes", themes_parsed.len());
    println!("background {:?} from {}", bg, source);

    let mut changed = 0usize;
    let mut needs_light = Vec::new();
    let mut lifted = Vec::new();
    for theme in &themes_parsed {
        let (fg, ratio) = crate::theme::readable_on(theme.index);
        let (border, bratio) = crate::theme::border_for(theme.index, bg);
        let stem = theme
            .path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        if fg == crate::theme::TEXT_LIGHT {
            needs_light.push((stem.clone(), theme.index, ratio));
        }
        if border != theme.index {
            lifted.push((stem, theme.index, border, bratio));
        }
        if let Some(text) = crate::theme::with_computed_colours(theme, bg) {
            changed += 1;
            if apply {
                std::fs::write(&theme.path, text)?;
            }
        }
    }

    // Sorted by stem rather than left in directory order: `blue` reads before
    // `blue-dark` in a report and after it in a directory listing, and the
    // report is the thing a person reads.
    needs_light.sort_by(|a, b| a.0.cmp(&b.0));
    lifted.sort_by(|a, b| a.0.cmp(&b.0));

    println!(
        "\n-- text colour: {} of {} themes need {}, and were painting dark text on a dark block",
        needs_light.len(),
        themes_parsed.len(),
        crate::theme::TEXT_LIGHT
    );
    for (stem, index, ratio) in &needs_light {
        println!("   {stem:<22} colour{index:<4} light contrast {ratio:.1}");
    }

    println!(
        "\n-- borders: {} of {} themes need a lighter active pane border to clear {:.1}:1 on {:?}",
        lifted.len(),
        themes_parsed.len(),
        crate::theme::BORDER_MIN,
        bg
    );
    for (stem, index, border, ratio) in &lifted {
        println!("   {stem:<22} colour{index:<4} -> colour{border:<4} {ratio:.1}");
    }

    println!(
        "\n{} @theme-color-on-main and @theme-color-border into {changed} files",
        if apply { "wrote" } else { "would write" }
    );

    if let Some(level) = level {
        // Every cube colour that can carry readable text, not two siblings of
        // whatever happened to be in the directory. The old behaviour answered
        // "vary what I have"; the question people ask a theme generator is
        // "show me what there is", and the answer should not depend on which
        // files are already there.
        let taken: std::collections::HashSet<u8> = themes_parsed.iter().map(|t| t.index).collect();
        let mut made = Vec::new();
        for (stem, index) in crate::theme::themes_for(level) {
            if taken.contains(&index) {
                continue;
            }
            let target = dir.join(format!("{stem}.tmux"));
            if apply && !target.exists() {
                std::fs::write(
                    &target,
                    crate::theme::cube_theme_file(&stem, index, bg, &dir.display().to_string()),
                )?;
            }
            made.push((stem, index));
        }
        println!(
            "\n-- shades ({}): {} {} themes{}",
            level_name,
            if apply { "wrote" } else { "would write" },
            made.len(),
            match level.min_contrast() {
                Some(min) => format!(", every cube colour whose text clears {min}:1"),
                None => ", the bundled six and their lighter and darker siblings".to_string(),
            }
        );
        for (stem, index) in &made {
            println!("   {stem:<22} colour{index}");
        }
    }

    Ok(())
}

/// `~` to the home directory, because a default path in `--help` reads better
/// with a tilde in it than with somebody's username.
/// The themes directory a `--themes` flag asked for, or the one this machine
/// actually uses.
///
/// Resolved rather than defaulted in clap, because the answer depends on
/// whether this machine keeps its tmux config under XDG or at `~/.tmux.conf`,
/// and a default string printed in `--help` would be a lie on half of them.
fn themes_dir_or(flag: Option<String>) -> std::path::PathBuf {
    match flag {
        Some(p) => expand_tilde(&p),
        None => crate::theme::default_themes_dir(),
    }
}

fn expand_tilde(path: &str) -> std::path::PathBuf {
    match path.strip_prefix("~/") {
        Some(rest) => match std::env::var_os("HOME") {
            Some(home) => std::path::PathBuf::from(home).join(rest),
            None => std::path::PathBuf::from(path),
        },
        None => std::path::PathBuf::from(path),
    }
}

/// `theme pick`: choose one, then apply it or remember it.
fn theme_pick(
    target: Option<String>,
    register: Option<String>,
    dir: &std::path::Path,
    print: bool,
) -> anyhow::Result<()> {
    let rows = crate::theme::rows(dir);
    if rows.is_empty() {
        anyhow::bail!("no themes in {}", dir.display());
    }

    if print {
        for row in &rows {
            println!(
                "{}\t{}\t{}",
                row.path.file_name().unwrap_or_default().to_string_lossy(),
                row.name,
                row.colour
            );
        }
        return Ok(());
    }

    let items: Vec<crate::picker::Item> = rows
        .iter()
        .map(|r| {
            crate::picker::Item::with_preview(r.search_text(), theme_preview(r))
                // The colour as a block, not as a tint on the text: a theme
                // list where every row is painted its own colour is a list
                // where half the rows cannot be read.
                .with_swatch(Some(r.colour.clone()))
                .in_columns(r.columns())
        })
        .collect();

    let config = config_or_default();
    let chrome = crate::picker::Chrome {
        title: match &register {
            Some(s) => format!("[ Theme for {s} ]"),
            None => "[ Theme ]".to_string(),
        },
        footer: "enter applies it   esc cancels".into(),
        preview_title: "[ Colours ]".into(),
        ..Default::default()
    }
    .configured(&config.picker, crate::config::Picker::Theme);

    let Some(index) = crate::picker::run(items, "", &chrome)? else {
        return Ok(());
    };
    let row = &rows[index];

    if let Some(session) = register {
        // Remembered and not applied: the session does not exist yet, so
        // applying here would paint whichever session happens to be current.
        let stem = row
            .path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let map = dir.join("_project-map.tsv");
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(map)?;
        writeln!(f, "{session}\t{stem}")?;
        println!("{}", row.path.display());
        return Ok(());
    }

    let target = target.or_else(attached_session_sync);
    source_theme(&row.path, target.as_deref());
    Ok(())
}

/// `theme init`: write the starter themes and the machinery that applies them.
///
/// Refuses to overwrite. Somebody running this twice, or running it beside
/// themes they already wrote, should get the files they are missing and keep
/// everything they have; the alternative is a command that can quietly undo an
/// afternoon's work.
fn theme_init(dir: &std::path::Path) -> anyhow::Result<()> {
    use crate::theme::{BASE_THEMES, apply_file, base_theme_file, readable_on, reset_file};

    std::fs::create_dir_all(dir)?;

    let mut written = Vec::new();
    let mut kept = Vec::new();
    let mut write = |name: String, body: String| -> anyhow::Result<()> {
        let path = dir.join(&name);
        if path.exists() {
            kept.push(name);
            return Ok(());
        }
        std::fs::write(&path, body)?;
        written.push(name);
        Ok(())
    };

    write("_reset.tmux".to_string(), reset_file())?;
    write("_apply.tmux".to_string(), apply_file())?;
    for (stem, label, index) in BASE_THEMES {
        write(
            format!("{stem}.tmux"),
            base_theme_file(stem, label, index, &dir.display().to_string()),
        )?;
    }

    println!("{}", dir.display());
    for name in &written {
        let index = BASE_THEMES
            .iter()
            .find(|(s, _, _)| format!("{s}.tmux") == *name)
            .map(|(_, _, i)| *i);
        match index {
            Some(i) => {
                let (fg, ratio) = readable_on(i);
                println!("  wrote  {name:<16} colour{i} with {fg} at {ratio:.1}:1");
            }
            None => println!("  wrote  {name}"),
        }
    }
    for name in &kept {
        println!("  kept   {name:<16} already there, left alone");
    }
    if written.is_empty() {
        println!("\nNothing to do: every file was already there.");
        return Ok(());
    }
    println!(
        "\nNext: tmux-companion theme gen --apply --shades\n\
         That mints a lighter and a darker sibling of each colour and measures\n\
         every border against this terminal's background."
    );
    Ok(())
}

/// `theme add`: write a theme from one colour, or two.
fn theme_add(
    bg: &str,
    fg: Option<String>,
    name: Option<String>,
    force: bool,
    dir: &std::path::Path,
) -> anyhow::Result<()> {
    use crate::theme::{
        TEXT_MIN_AA, added_theme_file, contrast, readable_on_rgb, resolve_colour, stem_for, swatch,
    };

    let Some(bg_rgb) = resolve_colour(bg) else {
        anyhow::bail!(
            "{bg} is not a colour tmux takes.\n\
             Try a name, colour0 to colour255, or #rrggbb; \
             `tmux-companion theme list-colours` prints every one."
        );
    };

    // One colour is enough, and the second is the interesting decision: pick it
    // yourself and the contrast is yours to answer for, leave it out and the
    // better of black and white is chosen for you.
    let (fg_value, ratio) = match &fg {
        Some(chosen) => {
            let Some(fg_rgb) = resolve_colour(chosen) else {
                anyhow::bail!("{chosen} is not a colour tmux takes");
            };
            (chosen.clone(), contrast(bg_rgb, fg_rgb))
        }
        None => {
            let (computed, r) = readable_on_rgb(bg_rgb);
            (computed.to_string(), r)
        }
    };

    if ratio < TEXT_MIN_AA && !force {
        anyhow::bail!(
            "{} on {} is {ratio:.2}:1, under WCAG AA at {TEXT_MIN_AA}:1.\n\
             Leave --fg out to have it chosen, pick a different pair, or pass \
             --force to write it anyway.",
            fg_value,
            bg
        );
    }

    let label = name.unwrap_or_else(|| bg.to_string());
    let stem = stem_for(&label);
    if stem.is_empty() {
        anyhow::bail!("{label} leaves nothing to name a file after");
    }
    let path = dir.join(format!("{stem}.tmux"));
    if path.exists() {
        anyhow::bail!(
            "{} is already there; delete it or pick another name",
            path.display()
        );
    }
    std::fs::create_dir_all(dir)?;
    std::fs::write(
        &path,
        added_theme_file(&label, bg, &fg_value, &dir.display().to_string()),
    )?;

    println!(
        "{} {label}  {fg_value} on {bg} at {ratio:.1}:1{}",
        swatch(bg),
        if ratio < TEXT_MIN_AA {
            "  (under AA)"
        } else {
            ""
        }
    );
    println!("  {}", path.display());
    println!("\nNext: tmux-companion theme gen --apply    adds the border");
    Ok(())
}

/// `theme list-colours`: every value tmux takes, painted.
fn theme_list_colours(print: bool) -> anyhow::Result<()> {
    use crate::theme::{all_colours, readable_on_rgb};

    for (value, name, (r, g, b)) in all_colours() {
        if print {
            println!("{value}");
            continue;
        }
        let (fg, ratio) = readable_on_rgb((r, g, b));
        // The block is painted in the colour and the label written on it, so
        // the row shows both what it looks like and what reads on top.
        let on = if fg == "colour16" { "30" } else { "97" };
        println!(
            "\x1b[48;2;{r};{g};{b}m\x1b[{on}m {value:<12} \x1b[0m  #{r:02x}{g:02x}{b:02x}               {fg} at {ratio:.1}:1{}",
            if name.is_empty() {
                String::new()
            } else {
                format!("   ({name})")
            }
        );
    }
    Ok(())
}

/// `theme apply`: the session-created hook's half, with no picker.
///
/// Silent when there is no theme to apply. This runs once per session created,
/// so anything it prints lands on somebody's terminal at the moment they open a
/// session, and "no theme yet" is the state every install starts in.
fn theme_apply(session: &str, target: Option<String>, dir: &std::path::Path) -> anyhow::Result<()> {
    let map = std::fs::read_to_string(dir.join("_project-map.tsv"))
        .map(|t| crate::project::parse_project_map(&t))
        .unwrap_or_default();
    let config = config_or_default();
    // An empty target is what `#{session_id}` expands to under tmux 3.5, which
    // does not resolve it inside a `run-shell` the way 3.7 does. Without this
    // the theme was sourced with no target at all, so it landed on whichever
    // session happened to be current -- which, at `session-created` time, is
    // not the session being created.
    let target = target
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| session.to_string());
    if let Some(path) = crate::theme::theme_for_session(session, &map, &config.theme, dir) {
        source_theme(&path, Some(&target));
    }
    Ok(())
}

/// What a theme looks like, and then what it is made of.
///
/// The card first, because a list of `@theme-color-*` values answers none of
/// the question anybody opens a theme preview to ask: whether the text on that
/// background can be read, what a message looks like, what a copy-mode
/// selection looks like. The settings follow it for the person who is editing
/// the file rather than choosing from it.
fn theme_preview(row: &crate::theme::ThemeRow) -> String {
    let text = std::fs::read_to_string(&row.path).unwrap_or_default();
    let settings = crate::theme::parse_settings(&text);

    // Wider than any preview pane, so the sample bars reach its edge whatever
    // that turns out to be and the pane clips the rest. The card is built
    // before the pane exists, so this cannot be a measurement; 60 was a guess
    // and on a pane narrower than that the overhang wrapped onto the next
    // line as a stray block of colour.
    let card = crate::theme::preview_card(&settings, 400);

    let mut keys: Vec<&String> = settings.keys().collect();
    keys.sort();
    let listed = keys
        .iter()
        .map(|k| {
            let v = &settings[*k];
            format!("  {} {:<26} {v}", crate::theme::swatch(v), k)
        })
        .collect::<Vec<String>>()
        .join("\n");
    format!("{card}\n{listed}\n")
}

/// `tmux source-file`, honouring a target.
///
/// The target matters: `_apply.tmux` sets window options, and a window option
/// lands on one window, so without a target the theme paints whichever window
/// happened to be current and every other window in the session keeps the
/// global default. That is why copy-mode selection could be readable in one
/// window and not the next.
/// The attached client's session, for the synchronous callers.
///
/// `theme pick` draws in a popup like every other picker, and a popup is not a
/// client, so a theme sourced with no target painted whichever session the
/// server touched last. Switch project, press the key, watch the session you
/// just left change colour.
fn attached_session_sync() -> Option<String> {
    let out = std::process::Command::new("tmux")
        .args(["list-clients", "-F", "#{client_session}"])
        .output()
        .ok()?;
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .map(|s| format!("{s}:"))
}

fn source_theme(path: &std::path::Path, target: Option<&str>) {
    let path = path.display().to_string();
    let mut args: Vec<&str> = vec!["source-file"];
    if let Some(t) = target {
        args.push("-t");
        args.push(t);
    }
    args.push(&path);
    let _ = std::process::Command::new("tmux").args(args).status();
}
