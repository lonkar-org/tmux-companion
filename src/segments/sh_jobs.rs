//! Jobs stopped under a pane: which ones, and what to draw for each.
//!
//! This was `vim-bg`, which asked one question with one answer baked in: is
//! there a suspended `nvim` under this pane. Somebody who suspends `vim`, or
//! `claude`, or a `cargo watch` got nothing, and the name of the subcommand
//! told them the tool was not written for them.
//!
//! The cost has not changed with the rename and is the reason the segment
//! stays off the combined status side: `sysinfo` enumerates the whole process
//! table to find the children of one pid, which measured 16.25 ms of server
//! CPU per call.

use crate::config::ShJobs;

/// The name of a job found under a pane, as the process table reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
    /// The process name.
    pub name: String,
    /// Whether it is stopped rather than running in the background.
    pub stopped: bool,
}

/// Match the jobs under a pane against the configured table and render them.
///
/// Pure, so the matching and the formatting are testable without a process
/// table: the only I/O is finding the jobs in the first place.
pub fn format_jobs(jobs: &[Job], config: &ShJobs) -> String {
    let mut out = String::new();
    let mut drawn = 0usize;

    for job in jobs {
        if drawn >= config.max {
            break;
        }
        if !config.states.matches(job.stopped) {
            continue;
        }
        let Some(entry) = config.job.iter().find(|e| e.matches(&job.name)) else {
            continue;
        };
        out.push_str(&entry.render());
        drawn += 1;
    }
    out
}

/// Every job under `pane_pid`, stopped or running in the background.
///
/// The `spawn_blocking` is not decoration: refreshing the process table is a
/// syscall-heavy scan, and running it on a worker thread keeps it off the
/// runtime that is answering other segments concurrently.
pub async fn jobs_under(pane_pid: u32) -> anyhow::Result<Vec<Job>> {
    Ok(tokio::task::spawn_blocking(move || {
        use sysinfo::{Pid, ProcessesToUpdate, System};
        let mut sys = System::new();
        sys.refresh_processes(ProcessesToUpdate::All, true);
        let parent = Pid::from_u32(pane_pid);
        sys.processes()
            .values()
            .filter(|p| p.parent() == Some(parent))
            .map(|p| Job {
                name: p.name().to_string_lossy().into_owned(),
                stopped: p.status() == sysinfo::ProcessStatus::Stop,
            })
            .collect()
    })
    .await?)
}

/// Whether a stopped `nvim` is a descendant of this pane.
///
/// Kept because the git segment asks this exact question when a pane pid is
/// passed by hand, and because changing what the git segment draws is a
/// separate decision from renaming this one.
pub async fn has_suspended_nvim(pane_pid: u32) -> anyhow::Result<bool> {
    Ok(jobs_under(pane_pid)
        .await?
        .iter()
        .any(|j| j.stopped && j.name.contains("nvim")))
}

/// Render the segment for one pane.
pub async fn render(pane_pid: u32, config: &ShJobs) -> anyhow::Result<String> {
    Ok(format_jobs(&jobs_under(pane_pid).await?, config))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{JobEntry, JobStates};

    fn stopped(name: &str) -> Job {
        Job {
            name: name.into(),
            stopped: true,
        }
    }

    fn running(name: &str) -> Job {
        Job {
            name: name.into(),
            stopped: false,
        }
    }

    #[test]
    fn the_default_table_draws_the_old_vim_marker() {
        // The rename is a rename: a suspended nvim has to render exactly what
        // `vim-bg` rendered, or somebody's bar changes for no reason they
        // asked for.
        let out = format_jobs(&[stopped("nvim")], &ShJobs::default());
        assert_eq!(out, crate::config::ShJobs::VIM_OUTPUT);
    }

    #[test]
    fn a_running_job_is_not_a_suspended_one() {
        let out = format_jobs(&[running("nvim")], &ShJobs::default());
        assert_eq!(out, "", "the default table wants stopped jobs only");
    }

    #[test]
    fn a_job_nobody_configured_draws_nothing() {
        let out = format_jobs(&[stopped("emacs")], &ShJobs::default());
        assert_eq!(out, "");
    }

    #[test]
    fn a_configured_job_draws_its_icon() {
        let config = ShJobs {
            job: vec![JobEntry {
                match_: "^claude$".into(),
                icon: "AI".into(),
                color: String::new(),
                window_name: None,
            }],
            ..ShJobs::default()
        };
        assert_eq!(format_jobs(&[stopped("claude")], &config), "AI");
    }

    #[test]
    fn matching_is_a_regex_not_a_substring() {
        let config = ShJobs {
            job: vec![JobEntry {
                match_: "^vim$".into(),
                icon: "V".into(),
                color: String::new(),
                window_name: None,
            }],
            ..ShJobs::default()
        };
        assert_eq!(format_jobs(&[stopped("vim")], &config), "V");
        assert_eq!(
            format_jobs(&[stopped("nvim")], &config),
            "",
            "^vim$ must not match nvim"
        );
    }

    #[test]
    fn the_first_matching_entry_wins() {
        let config = ShJobs {
            job: vec![
                JobEntry {
                    match_: "vim".into(),
                    icon: "first".into(),
                    color: String::new(),
                    window_name: None,
                },
                JobEntry {
                    match_: "nvim".into(),
                    icon: "second".into(),
                    color: String::new(),
                    window_name: None,
                },
            ],
            ..ShJobs::default()
        };
        assert_eq!(format_jobs(&[stopped("nvim")], &config), "first");
    }

    #[test]
    fn max_bounds_how_much_a_busy_pane_can_put_on_the_bar() {
        let config = ShJobs {
            job: vec![JobEntry {
                match_: ".".into(),
                icon: "J".into(),
                color: String::new(),
                window_name: None,
            }],
            max: 2,
            ..ShJobs::default()
        };
        let jobs = [stopped("a"), stopped("b"), stopped("c")];
        assert_eq!(format_jobs(&jobs, &config), "JJ");
    }

    #[test]
    fn states_can_include_background_jobs() {
        let config = ShJobs {
            job: vec![JobEntry {
                match_: "watch".into(),
                icon: "W".into(),
                color: String::new(),
                window_name: None,
            }],
            states: JobStates::Any,
            ..ShJobs::default()
        };
        assert_eq!(format_jobs(&[running("cargo-watch")], &config), "W");
    }

    #[test]
    fn an_invalid_regex_matches_nothing_rather_than_panicking() {
        // The pattern comes from a config file, so it can be anything.
        let config = ShJobs {
            job: vec![JobEntry {
                match_: "[unclosed".into(),
                icon: "X".into(),
                color: String::new(),
                window_name: None,
            }],
            ..ShJobs::default()
        };
        assert_eq!(format_jobs(&[stopped("unclosed")], &config), "");
    }

    #[test]
    fn a_colour_wraps_the_icon_in_tmux_markup() {
        let config = ShJobs {
            job: vec![JobEntry {
                match_: "nvim".into(),
                icon: "V".into(),
                color: "#539035".into(),
                window_name: None,
            }],
            ..ShJobs::default()
        };
        assert_eq!(format_jobs(&[stopped("nvim")], &config), "#[fg=#539035]V");
    }
}
