//! Whether the network is there, asked on a timer and said on the health mark.
//!
//! An agent that stalls, a fetch that fails and a page that won't load are one
//! fact seen three times, and the bar is where it can be said once.
//! `tmux-plugins/tmux-online-status` drew a mark for it from a `ping` run by
//! `#()` on every redraw, and was last pushed in September 2023.
//!
//! Here the daemon opens a TCP connection to one address on a timer, and the
//! health mark reads `offline` when two in a row got no answer. One miss is a
//! lost packet, and a mark that comes up for ten seconds and goes away again
//! teaches people to stop looking at it. Online draws nothing, the way a
//! healthy bar draws nothing.
//!
//! A connection rather than a ping, because a ping needs a raw socket or
//! somebody else's binary and a connection needs neither. Off unless asked
//! for: it reaches an address outside the machine, and a daemon doing that on
//! its own is a surprise nobody should get from a status bar.

use std::time::Duration;

/// How many probes in a row have to fail before the mark comes up.
pub const MISSES: u32 = 2;

/// What the timer remembers between two probes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Watch {
    /// Probes that failed in a row.
    pub misses: u32,
    /// When the first of them failed, in unix seconds.
    pub first_miss: Option<u64>,
}

impl Watch {
    /// Take one probe's answer.
    pub fn step(&mut self, answered: bool, now: u64) {
        if answered {
            *self = Watch::default();
            return;
        }
        self.misses = self.misses.saturating_add(1);
        self.first_miss.get_or_insert(now);
    }

    /// Since when the network has been gone, once enough probes say so.
    ///
    /// Counted from the first miss and not from the one that tipped it: the
    /// network went when it went, and the second probe only confirmed it.
    pub fn offline_since(&self) -> Option<u64> {
        self.first_miss.filter(|_| self.misses >= MISSES)
    }
}

/// The line `doctor` and the brief print, and the bar shortens to `offline`.
pub fn reason(since: u64, now: u64, probe: &str) -> String {
    format!(
        "offline for {}: nothing answers at {probe}",
        crate::panes::age(now.saturating_sub(since))
    )
}

/// Whether an address is a host and a port, which is what a probe needs.
pub fn is_probe(address: &str) -> bool {
    address
        .trim()
        .rsplit_once(':')
        .is_some_and(|(host, port)| !host.is_empty() && port.parse::<u16>().is_ok_and(|p| p > 0))
}

/// One probe: a connection opened and dropped.
///
/// A name that doesn't resolve is no answer, the same as a connection that
/// timed out, since with the network gone the resolver is the first thing to
/// fail.
pub async fn answers(probe: &str, timeout: Duration) -> bool {
    matches!(
        tokio::time::timeout(timeout, tokio::net::TcpStream::connect(probe.trim())).await,
        Ok(Ok(_))
    )
}

/// The task: probe, remember, wait.
pub async fn online_loop(
    state: std::sync::Arc<tokio::sync::Mutex<crate::server::state::ServerState>>,
    settings: crate::config::Online,
) {
    let interval = Duration::from_secs(settings.interval_secs.max(1));
    let timeout = Duration::from_millis(settings.timeout_ms.max(100));
    let mut watch = Watch::default();
    loop {
        tokio::time::sleep(interval).await;
        let answered = answers(&settings.probe, timeout).await;
        watch.step(answered, crate::panes::now_secs());
        // One statement under the lock, the probe well outside it.
        state.lock().await.offline_since = watch.offline_since();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_miss_is_a_lost_packet_and_two_are_offline() {
        let mut w = Watch::default();
        assert_eq!(w.offline_since(), None);
        w.step(false, 100);
        assert_eq!(w.offline_since(), None);
        w.step(false, 130);
        assert_eq!(w.offline_since(), Some(100), "from the first miss");
        w.step(false, 160);
        assert_eq!(w.offline_since(), Some(100));
    }

    #[test]
    fn one_answer_clears_it() {
        let mut w = Watch::default();
        w.step(false, 100);
        w.step(false, 130);
        w.step(true, 160);
        assert_eq!(w, Watch::default());
        // And the next outage is counted from its own first miss.
        w.step(false, 500);
        w.step(false, 530);
        assert_eq!(w.offline_since(), Some(500));
    }

    #[test]
    fn the_reason_says_how_long_and_what_was_asked() {
        assert_eq!(
            reason(100, 280, "1.1.1.1:443"),
            "offline for 3m: nothing answers at 1.1.1.1:443"
        );
        // A clock that went backwards is no time at all.
        assert!(reason(280, 100, "x:1").starts_with("offline for 0s"));
    }

    #[test]
    fn a_probe_is_a_host_and_a_port() {
        assert!(is_probe("1.1.1.1:443"));
        assert!(is_probe("example.org:80"));
        assert!(is_probe("[::1]:8080"));
        assert!(!is_probe("1.1.1.1"));
        assert!(!is_probe("example.org:"));
        assert!(!is_probe(":443"));
        assert!(!is_probe("example.org:http"));
        assert!(!is_probe("example.org:0"));
        assert!(!is_probe(""));
    }

    #[tokio::test]
    async fn a_listener_answers_and_a_closed_port_does_not() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap().to_string();
        assert!(answers(&address, Duration::from_secs(2)).await);

        // A socket that is bound and never listens refuses every connection,
        // and holds its port while it does. Dropping the listener above and
        // probing its port again was the first version of this, and it failed
        // once in a run of the whole suite and passed alone. I didn't catch
        // what answered, and a port that has been let go is anybody's, so
        // this one is never let go.
        let closed = tokio::net::TcpSocket::new_v4().unwrap();
        closed.bind("127.0.0.1:0".parse().unwrap()).unwrap();
        let address = closed.local_addr().unwrap().to_string();
        assert!(!answers(&address, Duration::from_secs(2)).await);

        assert!(!answers("not an address", Duration::from_secs(2)).await);
    }
}
