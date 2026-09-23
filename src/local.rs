//! Running a segment in this process, with no daemon and no socket.
//!
//! The daemon exists because a status bar asks the same question once a second
//! and forking `git status` every time is the bill this tool was written to
//! stop paying. Nothing else about the segments needs it, so `--no-daemon`
//! computes the answer here and exits: one process, one answer, no socket, no
//! background state and nothing left running afterwards.
//!
//! That is the right shape for a shell prompt, a one-off in a script, a CI job
//! that wants the branch line, and for anybody who wants to see what a segment
//! renders before deciding to run a daemon at all. It is the wrong shape for a
//! status bar, because every call pays the cold cost the cache exists to avoid:
//! about 51 ms for a cold `git status` against a large tree, against a cache
//! hit of well under one.
//!
//! Pair it with `--no-tmux` and the output is ANSI rather than tmux markup, so
//! these two flags together are the whole of "use this outside tmux".

use std::path::{Path, PathBuf};

use crate::{
    config::Config,
    segments::{
        git::{self, GstOptions, LineOpts},
        network,
    },
};

/// The git segment, computed here, with no cache and no daemon.
pub async fn gst(opts: &GstOptions) -> anyhow::Result<String> {
    let path = match &opts.path {
        Some(p) => p.canonicalize()?,
        None => std::env::current_dir()?,
    };
    if !git::is_inside_work_tree_uncached(&path).await {
        return Ok(String::new());
    }

    let status = git::fetch_git_status(&path).await?;
    let nvim_suspended = match opts.pane_pid {
        Some(p) => crate::segments::vim_bg::has_suspended_nvim(p)
            .await
            .unwrap_or(false),
        None => false,
    };

    let line = git::status_line_render(
        &status,
        nvim_suspended,
        false,
        &LineOpts {
            style: opts.style,
            cap_glyph: if opts.no_cap { Some("") } else { None },
            branch_max: opts.branch_max_len,
            branch_icon: opts.branch_icon,
            branch_types: &opts.branch_types,
            parts: &opts.parts,
            bar_bg: &opts.bar_bg,
        },
    );
    Ok(if opts.no_cap {
        line.trim_end().to_string()
    } else {
        line
    })
}

/// The bandwidth segment, computed here, with the previous counter reading in
/// a file instead of in a daemon.
///
/// A rate needs two readings, and without a resident process the earlier one
/// has to survive between two runs of the binary. The file holds the counters,
/// when they were taken, and what was drawn last, which is exactly the three
/// things `ServerState` holds for the daemon.
///
/// The first call after a reboot, or with no file yet, draws nothing and
/// records the anchor — the same thing the daemon does on its first call, for
/// the same reason: there is no interval to divide by yet.
pub async fn net(config: &Config) -> anyhow::Result<String> {
    let (rx, tx) = network::sample().await?;
    let now = unix_now();
    let path = sample_path();

    let previous = path.as_deref().and_then(read_sample);
    let Some(prev) = previous else {
        write_sample(path.as_deref(), rx, tx, now, "");
        return Ok(String::new());
    };

    let elapsed = now - prev.at;
    // A counter that went backwards means the machine rebooted or an interface
    // was replaced; re-anchor rather than reporting a rate off a wild delta.
    if elapsed <= 0.0 || rx < prev.rx || tx < prev.tx {
        write_sample(path.as_deref(), rx, tx, now, &prev.last);
        return Ok(prev.last);
    }
    if elapsed < network::MIN_ELAPSED.as_secs_f64() {
        // Leave the anchor alone so the next call has a full interval, which is
        // what the daemon does with the same reading.
        return Ok(prev.last);
    }

    let span = std::time::Duration::from_secs_f64(elapsed);
    let dl = network::rate(rx - prev.rx, span);
    let ul = network::rate(tx - prev.tx, span);
    let line = network::format_rates(dl, ul, &config.network, &config.bar.background);
    write_sample(path.as_deref(), rx, tx, now, &line);
    Ok(line)
}

/// What the file holds between two runs.
struct Sample {
    rx: u64,
    tx: u64,
    at: f64,
    last: String,
}

/// `$XDG_STATE_HOME/tmux-companion/net-sample`, or nothing if there is no home
/// to put it in, in which case every call is a first call and draws nothing.
fn sample_path() -> Option<PathBuf> {
    crate::server::state_dir().map(|d| d.join("net-sample"))
}

/// Wall-clock seconds. `Instant` is what the daemon uses and it cannot be
/// written to a file, so the no-daemon path measures against the clock and
/// accepts that a clock step shows up as one wrong reading.
fn unix_now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// Two lines: the counters and their timestamp, then what was drawn.
///
/// A file that does not parse is treated as absent, because the only cost of
/// that is one drawn-nothing call and the alternative is a status bar that
/// fails on a truncated write.
fn read_sample(path: &Path) -> Option<Sample> {
    let text = std::fs::read_to_string(path).ok()?;
    let mut lines = text.splitn(2, '\n');
    let mut head = lines.next()?.split_whitespace();
    let sample = Sample {
        rx: head.next()?.parse().ok()?,
        tx: head.next()?.parse().ok()?,
        at: head.next()?.parse().ok()?,
        last: lines
            .next()
            .unwrap_or("")
            .trim_end_matches('\n')
            .to_string(),
    };
    Some(sample)
}

/// Write via a temporary and rename, so two calls racing leave a whole file
/// rather than half of one. Failure is silent: not being able to write a cache
/// is not a reason to fail a status segment.
fn write_sample(path: Option<&Path>, rx: u64, tx: u64, at: f64, last: &str) {
    let Some(path) = path else { return };
    let Some(dir) = path.parent() else { return };
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    let tmp = path.with_extension(format!("tmp.{}", std::process::id()));
    if std::fs::write(&tmp, format!("{rx} {tx} {at}\n{last}")).is_err() {
        let _ = std::fs::remove_file(&tmp);
        return;
    }
    if std::fs::rename(&tmp, path).is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("tmux-companion-local-{name}"));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn a_written_sample_reads_back() {
        let p = tmp_dir("roundtrip").join("net-sample");
        write_sample(Some(&p), 10, 20, 1.5, "drawn");
        let got = read_sample(&p).expect("a sample");
        assert_eq!((got.rx, got.tx, got.at), (10, 20, 1.5));
        assert_eq!(got.last, "drawn");
    }

    #[test]
    fn an_empty_render_reads_back_as_empty() {
        // The first call writes no line, and the next call has to see that as
        // "nothing drawn" rather than as a missing field.
        let p = tmp_dir("empty").join("net-sample");
        write_sample(Some(&p), 1, 2, 3.0, "");
        assert_eq!(read_sample(&p).expect("a sample").last, "");
    }

    #[test]
    fn a_truncated_file_is_treated_as_absent() {
        let p = tmp_dir("truncated").join("net-sample");
        std::fs::write(&p, "10 20").unwrap();
        assert!(read_sample(&p).is_none());
    }

    #[test]
    fn a_file_of_nonsense_is_treated_as_absent() {
        let p = tmp_dir("nonsense").join("net-sample");
        std::fs::write(&p, "not a sample at all\n").unwrap();
        assert!(read_sample(&p).is_none());
    }

    #[test]
    fn a_missing_file_is_not_an_error() {
        assert!(read_sample(&tmp_dir("missing").join("nothing-here")).is_none());
    }

    #[test]
    fn writing_leaves_no_temporary_behind() {
        let d = tmp_dir("no-litter");
        write_sample(Some(&d.join("net-sample")), 1, 2, 3.0, "x");
        let left: Vec<_> = std::fs::read_dir(&d)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(left, vec!["net-sample".to_string()]);
    }
}
