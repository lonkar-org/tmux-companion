//! One screen with the state of the server, for the moment you sit down.
//!
//! Everything on it exists as a command already: `inbox` for the agents
//! waiting, `doctor` for the health reasons, `sessions idle` for the sessions
//! nobody has touched, `sessions list` for the last snapshot. This is the
//! version that answers "what needs me" in one look, and the `--hook` form for
//! `client-attached` opens it only when the answer is not "nothing".

use std::io::IsTerminal;

use crate::inbox::{self, Entry};
use crate::sessions::idle::IdleSession;

/// Days a session has to sit untouched before the brief lists it.
pub const IDLE_DAYS: u64 = 3;

/// What the brief has to say.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Brief {
    /// The agents waiting on you, longest wait first.
    pub waiting: Vec<Entry>,
    /// Why the health mark is up; empty means it is not.
    pub health: Vec<String>,
    /// Sessions with no client and nothing happening for [`IDLE_DAYS`].
    pub idle: Vec<IdleSession>,
    /// How many sessions the server has.
    pub sessions: usize,
    /// How many agents run, and how many of those are waiting.
    pub agents: (usize, usize),
    /// The last snapshot's stamp and how long ago it was taken, when any.
    pub last_snapshot: Option<(String, u64)>,
}

impl Brief {
    /// Whether the screen has anything that needs a person: a waiting agent,
    /// or a health reason. Idle sessions and an old snapshot are not news.
    pub fn has_news(&self) -> bool {
        !self.waiting.is_empty() || !self.health.is_empty()
    }
}

/// Unix seconds from a snapshot stamp, `20260926T133256`.
///
/// The inverse of `store::stamp_from`, days-from-civil in the other
/// direction; both are UTC, so no zone gets a say.
pub fn stamp_secs(stamp: &str) -> Option<u64> {
    let s = stamp.trim();
    if s.len() != 15 || s.as_bytes()[8] != b'T' {
        return None;
    }
    let n = |a: usize, b: usize| s[a..b].parse::<i64>().ok();
    let (y, m, d) = (n(0, 4)?, n(4, 6)?, n(6, 8)?);
    let (hh, mm, ss) = (n(9, 11)?, n(11, 13)?, n(13, 15)?);
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) || hh > 23 || mm > 59 || ss > 59 {
        return None;
    }
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    u64::try_from(days * 86_400 + hh * 3600 + mm * 60 + ss).ok()
}

/// The screen, as text.
///
/// Sections in the order they need answering: what is waiting on you, what
/// is wrong, what can go, then the one line of numbers.
pub fn render(b: &Brief, now: u64, home: &str) -> String {
    let mut out = String::new();
    if b.waiting.is_empty() {
        out.push_str("Nothing is waiting on you.\n");
    } else {
        out.push_str(&format!("Waiting on you ({})\n", b.waiting.len()));
        for e in &b.waiting {
            out.push_str(&format!(
                "  {:<14} {:<12} {:<5} {}\n",
                e.at,
                e.program,
                crate::panes::age(now.saturating_sub(e.since)),
                inbox::question(&e.lines)
            ));
        }
    }
    out.push('\n');
    if b.health.is_empty() {
        out.push_str("Health: ok\n");
    } else {
        out.push_str("Health\n");
        for r in &b.health {
            out.push_str(&format!("  {r}\n"));
        }
    }
    out.push('\n');
    if !b.idle.is_empty() {
        out.push_str(&format!("Idle for {IDLE_DAYS}+ days ({})\n", b.idle.len()));
        for s in &b.idle {
            let cols = crate::sessions::idle::columns(s, home);
            out.push_str(&format!("  {}\n", cols.join("  ")));
        }
        out.push('\n');
    }
    let snapshot = match &b.last_snapshot {
        Some((_, age)) => format!("last snapshot {} ago", crate::panes::age(*age)),
        None => "no snapshot yet".to_string(),
    };
    out.push_str(&format!(
        "{} session{}, {} agent{} ({} waiting), {snapshot}\n",
        b.sessions,
        if b.sessions == 1 { "" } else { "s" },
        b.agents.0,
        if b.agents.0 == 1 { "" } else { "s" },
        b.agents.1
    ));
    out
}

/// Ask the daemon and tmux for everything the screen shows.
pub async fn gather() -> Brief {
    let now = crate::panes::now_secs();
    let ask = |cmd: &'static str| async move {
        crate::client::send(crate::proto::Request::raw(cmd, serde_json::Value::Null))
            .await
            .ok()
            .filter(|r| r.error.is_none())
            .map(|r| r.output)
            .unwrap_or_default()
    };
    let (inbox_json, health_text, idle, sessions_text) = tokio::join!(
        ask("__inbox"),
        ask("__health"),
        crate::sessions::idle::list(IDLE_DAYS),
        crate::cli::tmux_capture(&["list-sessions", "-F", "#{session_name}"]),
    );
    let waiting: Vec<Entry> = serde_json::from_str(&inbox_json).unwrap_or_default();
    let health: Vec<String> = health_text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(str::to_string)
        .collect();
    let config = crate::cli::config_or_default();
    let panes = crate::panes::list().await;
    let sample = crate::segments::agents::count(
        &panes,
        &config.agents.programs,
        now,
        config.agents.waiting_secs,
    );
    let last_snapshot = crate::server::state_dir()
        .and_then(|d| crate::sessions::store::last_stamp_in(&d))
        .map(|stamp| {
            let age = stamp_secs(&stamp).map_or(0, |t| now.saturating_sub(t));
            (stamp, age)
        });
    Brief {
        waiting,
        health,
        idle,
        sessions: sessions_text
            .lines()
            .filter(|l| !l.trim().is_empty())
            .count(),
        agents: (sample.total, sample.waiting),
        last_snapshot,
    }
}

/// `brief`: print the screen; in a popup, wait for a key so it can be read.
///
/// `--hook` is the `client-attached` half: it opens the popup only when there
/// is news, and otherwise says nothing, so attaching to a quiet server stays
/// quiet. The binary is named by its own path for the reason `start --hook`
/// gives: a popup's shell has the tmux server's PATH.
pub async fn run(print: bool, hook: bool) -> anyhow::Result<()> {
    let b = gather().await;
    if hook {
        if !b.has_news() {
            return Ok(());
        }
        let me = std::env::current_exe()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| "tmux-companion".to_string());
        crate::cli::tmux(&[
            "display-popup",
            "-E",
            "-w",
            "70%",
            "-h",
            "60%",
            &format!("{me} brief"),
        ])
        .await;
        return Ok(());
    }
    let home = std::env::var("HOME").unwrap_or_default();
    print!("{}", render(&b, crate::panes::now_secs(), &home));
    if !print && std::io::stdin().is_terminal() {
        println!("\npress enter to close");
        let mut line = String::new();
        let _ = std::io::stdin().read_line(&mut line);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stamp_goes_back_to_the_seconds_it_came_from() {
        for secs in [
            0i64,
            951_868_800,
            1_709_210_096,
            1_790_067_600,
            1_790_427_176,
        ] {
            let stamp = crate::sessions::store::stamp_from(secs);
            assert_eq!(stamp_secs(&stamp), Some(secs as u64), "{stamp}");
        }
        assert_eq!(stamp_secs("2026-09-26"), None);
        assert_eq!(stamp_secs("20261326T000000"), None);
    }

    #[test]
    fn a_quiet_server_is_two_lines_and_no_news() {
        let b = Brief {
            sessions: 3,
            ..Default::default()
        };
        assert!(!b.has_news());
        let text = render(&b, 1000, "/home/me");
        assert!(text.starts_with("Nothing is waiting on you.\n"), "{text}");
        assert!(text.contains("Health: ok\n"), "{text}");
        assert!(
            text.contains("3 sessions, 0 agents (0 waiting), no snapshot yet"),
            "{text}"
        );
        assert!(!text.contains("Idle for"), "{text}");
    }

    #[test]
    fn a_waiting_agent_and_a_health_reason_are_news_and_come_first() {
        let pane = crate::panes::Pane {
            session: "api".into(),
            window_index: 2,
            window_name: "ai".into(),
            pane_index: 1,
            id: "%7".into(),
            command: "claude".into(),
            path: "/home/me/w/api".into(),
            title: "laptop".into(),
            visible: false,
            activity: 700,
            in_mode: false,
            host: "laptop".into(),
        };
        let b = Brief {
            waiting: vec![inbox::entry(&pane, "/home/me", "> Continue? (y/n)".into())],
            health: vec![
                "config.toml changed after the daemon started; run tmux-companion restart".into(),
            ],
            sessions: 1,
            agents: (1, 1),
            last_snapshot: Some(("20260926T133256".into(), 720)),
            ..Default::default()
        };
        assert!(b.has_news());
        let text = render(&b, 1000, "/home/me");
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines[0], "Waiting on you (1)");
        assert!(
            lines[1].contains("api:2.1")
                && lines[1].contains("5m")
                && lines[1].ends_with("> Continue? (y/n)"),
            "{}",
            lines[1]
        );
        assert!(text.contains("Health\n  config.toml changed"), "{text}");
        assert!(
            text.ends_with("1 session, 1 agent (1 waiting), last snapshot 12m ago\n"),
            "{text}"
        );
    }
}
