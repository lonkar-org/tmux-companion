//! The client half of `sessions resurrect`: the confirm screen, the countdown
//! and the crash acknowledgement, which run in the terminal and so cannot live
//! in the daemon.

use crate::cli::{ResurrectOptions, config_or_default, plural, tmux, tmux_capture};

/// The body of [`run_sessions_resurrect`], so the error path has one home.
pub(crate) async fn resurrect(opts: ResurrectOptions) -> anyhow::Result<i32> {
    let config = config_or_default();
    let state_dir = crate::server::state_dir()
        .ok_or_else(|| anyhow::anyhow!("no state directory: neither XDG_STATE_HOME nor HOME"))?;

    let snapshot = match &opts.stamp {
        Some(s) => crate::sessions::store::load_in(&state_dir, s)?,
        None => match crate::sessions::store::load_last_in(&state_dir) {
            Ok(snap) => snap,
            // Nothing of our own, so read what tmux-resurrect left. Somebody
            // switching over has months of saves and no reason to lose them on
            // the day they try this.
            Err(mine) => {
                let home = std::env::var("HOME").unwrap_or_default();
                match crate::sessions::import::newest_in(&home) {
                    Some(snap) => {
                        println!(
                            "no snapshot of our own, reading {}",
                            snap.header.imported_from
                        );
                        snap
                    }
                    None => return Err(mine),
                }
            }
        },
    };

    // A warning and not a refusal: the format number decides whether the file
    // can be read at all, and this build reads it, but a newer build may have
    // recorded something this one does not know to restore.
    if let Some(v) = snapshot.written_by_newer_build() {
        eprintln!("snapshot written by a newer build ({v}); restore may miss what it knows");
    }

    // What is already here. A server that is not running answers nothing,
    // which is the empty list and the case a restore is for.
    let live: Vec<String> = tmux_capture(&["list-sessions", "-F", "#{session_name}"])
        .await
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect();

    // Start the server before asking it anything. `show-option -gv base-index`
    // against a server that is not running answers nothing, which parsed as
    // zero and made the first restore move a window that was never there.
    let _ = tokio::process::Command::new("tmux")
        .arg("start-server")
        .status()
        .await;

    let plan = crate::restore::plan(&config.restore, &snapshot);
    let missing = crate::sessions::restore::missing_directories(&snapshot, |d| {
        std::path::Path::new(d).is_dir()
    });
    let home = std::env::var("HOME").unwrap_or_default();

    let spec = crate::sessions::restore::RestoreSpec {
        snapshot: &snapshot,
        plan: &plan,
        live: &live,
        only: &opts.only,
        exclude: &opts.exclude,
        merge: opts.merge,
        home: &home,
        base_index: tmux_number("base-index").await.unwrap_or(0),
        pane_base: tmux_number("pane-base-index").await.unwrap_or(0),
        missing: &missing,
    };

    let built = match crate::sessions::restore::rebuild(&spec) {
        Ok(b) => b,
        Err(refusal) => {
            eprintln!("{refusal}");
            return Ok(refusal.code());
        }
    };

    // Only the sessions this restore would actually build get a say in whether
    // it stops to ask. A pane in a session that is already live, or one
    // `--only` left out, is nobody's decision here.
    let building: Vec<crate::restore::Planned> = plan
        .iter()
        .filter(|p| built.sessions.contains(&p.session))
        .cloned()
        .collect();
    let unsettled: Vec<&crate::restore::Planned> =
        building.iter().filter(|p| !p.settled()).collect();

    println!(
        "{} {} session{}, {} pane{}  from {}, {}",
        if opts.dry_run {
            "would restore"
        } else {
            "restoring"
        },
        built.sessions.len(),
        plural(built.sessions.len()),
        building.len(),
        plural(building.len()),
        snapshot.header.captured_at,
        if snapshot.header.clean {
            "taken at shutdown"
        } else {
            "taken while running"
        }
    );
    if !missing.is_empty() {
        println!(
            "  {} director{} not on this machine, opening at {home}:",
            missing.len(),
            if missing.len() == 1 { "y" } else { "ies" }
        );
        for dir in &missing {
            println!("    {dir}");
        }
    }
    for line in &built.skipped {
        println!("  {line}");
    }
    if !unsettled.is_empty() {
        println!(
            "  {} pane{} nobody has said to run, left at a prompt:",
            unsettled.len(),
            plural(unsettled.len())
        );
        for p in &unsettled {
            println!("    {}:{}.{}  {}", p.session, p.window, p.pane, p.saved);
        }
        if !opts.yes && !opts.dry_run {
            println!("  --yes runs everything the table claimed and opens the rest at a prompt");
        }
    }

    // Ask, when the restore does not know something and somebody is watching.
    // Never on a count of panes: a count would stop every ordinary restore here
    // and stay quiet on the small one holding something unrecognised.
    let watched = std::io::IsTerminal::is_terminal(&std::io::stdin());
    let crashed = after_a_crash();
    let mut approved: Vec<usize> = Vec::new();
    if opts.yes {
        approved = crate::restore::unsettled_rows(&building);
    } else if !opts.dry_run && watched && crate::restore::needs_a_look(&building, crashed) {
        let rows = crate::sessions::summary::rows(&building);
        let headline = crate::sessions::summary::headline(
            built.sessions.len(),
            snapshot.pane_count(),
            crate::sessions::summary::agents(&building, &config.agents.programs),
            &snapshot.header.captured_at,
            snapshot.header.clean,
        );
        let countdown = std::time::Duration::from_secs(config.sessions.confirm_secs);
        match crate::sessions::summary::confirm(&headline, rows, countdown)? {
            crate::sessions::summary::Outcome::Go(rows) => approved = rows,
            crate::sessions::summary::Outcome::Cancelled => {
                println!("cancelled, nothing restored");
                return Ok(0);
            }
        }
    }

    // Rebuild against the plan somebody actually agreed to. Without this an
    // unsettled row would still run, because its decision is already `Run` for
    // a command the table claims but the capture had to guess at.
    let agreed =
        crate::restore::withhold_unapproved(&plan, &approved_in_plan(&plan, &building, &approved));
    let spec = crate::sessions::restore::RestoreSpec {
        plan: &agreed,
        ..spec
    };
    let built = crate::sessions::restore::rebuild(&spec).unwrap_or(built);

    use crate::sessions::restore::Step;
    if opts.dry_run {
        for step in &built.commands {
            match step {
                Step::Tmux(cmd) => println!("tmux {}", cmd.join(" ")),
                Step::WaitForPrompt(at) => println!("# wait for a prompt in {at}"),
            }
        }
        return Ok(0);
    }

    // The restore is going ahead, so the crash has been acted on: whoever was
    // watching saw the summary, or `--yes` or a script said not to ask. Left
    // in place, the marker would open the summary on every restore until the
    // next crash replaced it. A dry run and a cancel both return above and
    // leave it, because neither has dealt with anything.
    if crashed {
        crate::sessions::timer::acknowledge_crash();
    }

    for step in &built.commands {
        match step {
            Step::Tmux(cmd) => {
                let borrowed: Vec<&str> = cmd.iter().map(String::as_str).collect();
                tmux(&borrowed).await;
            }
            Step::WaitForPrompt(at) => wait_for_prompt(at).await,
        }
    }

    // Attach when somebody is watching and is not already inside tmux. A boot
    // script has no terminal and wants the server left running.
    let inside = std::env::var_os("TMUX").is_some();
    let watched = std::io::IsTerminal::is_terminal(&std::io::stdin());
    if opts.detach || inside || !watched {
        println!("  restored, detached. `tmux attach` when you want it");
        return Ok(0);
    }
    let target = snapshot
        .attach_target()
        .filter(|t| built.sessions.iter().any(|s| s == t))
        .or_else(|| built.sessions.first().map(String::as_str));
    if let Some(target) = target {
        let _ = tokio::process::Command::new("tmux")
            .args(["attach", "-t", &format!("={target}")])
            .status()
            .await;
    }
    Ok(0)
}

/// Whether the last run of the daemon ended badly, and no restore has acted
/// on it yet.
///
/// The daemon writes a `running` file at startup and removes it on a clean
/// stop; the next daemon to start finds one naming a dead pid and moves it
/// to `crashed`, which is what this reads, so a daemon that is alive right
/// now never counts. A snapshot's own `clean` flag cannot answer this: a
/// timer's capture writes `false` because the daemon does not know yet, so
/// believing it would open the summary on every restore from an automatic
/// save. `resurrect` removes the marker once it has gone ahead.
pub(crate) fn after_a_crash() -> bool {
    crate::server::state_dir().is_some_and(|d| crate::sessions::timer::crashed_in(&d))
}

/// Translate approvals given against the buildable rows back into positions in
/// the whole plan.
///
/// The screen only ever shows what this restore would build, so its indices are
/// into that shorter list. Applying them to the full plan without translating
/// would approve whichever pane happened to sit at the same position, which is
/// the sort of off-by-one that runs the wrong command in somebody's repository.
fn approved_in_plan(
    plan: &[crate::restore::Planned],
    building: &[crate::restore::Planned],
    approved: &[usize],
) -> Vec<usize> {
    approved
        .iter()
        .filter_map(|i| building.get(*i))
        .filter_map(|wanted| {
            plan.iter().position(|p| {
                p.session == wanted.session && p.window == wanted.window && p.pane == wanted.pane
            })
        })
        .collect()
}

/// Wait until a pane has drawn a prompt, or until the ceiling.
///
/// tmux marks a prompt line when the shell says where one begins, which is what
/// `tmux-companion shell-init` makes it do. A shell that emits the mark answers
/// in milliseconds; one that does not costs the full wait once per pane and
/// then gets typed into anyway, which is what every version of this did before
/// the mark existed.
///
/// The alternative is a fixed sleep, which is too short on a slow morning and
/// wasted every other time.
async fn wait_for_prompt(at: &str) {
    use crate::sessions::restore::{PROMPT_POLL, PROMPT_WAIT, has_drawn_a_prompt};
    let until = std::time::Instant::now() + PROMPT_WAIT;
    loop {
        let seen = tmux_capture(&["capture-pane", "-p", "-F", "-S", "-5", "-t", at]).await;
        if has_drawn_a_prompt(&seen) {
            return;
        }
        if std::time::Instant::now() >= until {
            return;
        }
        tokio::time::sleep(PROMPT_POLL).await;
    }
}

/// A numeric server option, or nothing when tmux did not answer with one.
async fn tmux_number(option: &str) -> Option<u32> {
    tmux_capture(&["show-option", "-gv", option])
        .await
        .trim()
        .parse()
        .ok()
}
