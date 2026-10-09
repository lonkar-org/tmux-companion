//! timing marks for the benchmark: when `TMUX_COMPANION_TRACE` names a file,
//! each mark appends one line saying what happened and when.
//!
//! The time is wall clock, not an `Instant`, because the reader is another
//! process: `scripts/bench-keys.py` notes the wall clock when it presses the
//! key, and the gap between that and this process's `main` is the part no
//! `Instant` taken in here could see (tmux forking the popup, the shell, exec
//! and the dynamic loader).
//!
//! Unset, a mark is one load of a `OnceLock` and nothing else, so the marks can
//! stay in the code that ships rather than living on a branch that rots.

use std::io::Write;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

const ENV: &str = "TMUX_COMPANION_TRACE";

static SINK: OnceLock<Option<Mutex<std::fs::File>>> = OnceLock::new();

fn sink() -> Option<&'static Mutex<std::fs::File>> {
    SINK.get_or_init(|| {
        let path = std::env::var_os(ENV).filter(|p| !p.is_empty())?;
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .ok()
            .map(Mutex::new)
    })
    .as_ref()
}

/// One line, `pid<TAB>what<TAB>seconds since the epoch`, when tracing is on.
///
/// `what` is a fixed word per call site (`main`, `rows`, `drawn`...) so the
/// reader can match on it; anything variable belongs in a second mark.
pub fn mark(what: &str) {
    let Some(file) = sink() else { return };
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or_default();
    let line = format!("{}\t{what}\t{now:.6}\n", std::process::id());
    if let Ok(mut f) = file.lock() {
        // One write per line, so two processes appending to the same file
        // interleave whole lines rather than halves of them.
        let _ = f.write_all(line.as_bytes());
    }
}

/// Whether marks go anywhere, for a call site that would otherwise build
/// something only to throw it away.
pub fn on() -> bool {
    sink().is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mark_with_tracing_off_does_nothing_and_does_not_panic() {
        // The test binary does not set the variable; if somebody runs the
        // suite with it set, the mark goes to their file, which is also fine.
        mark("test");
        let _ = on();
    }
}
