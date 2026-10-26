//! `tmux-companion bridge`: the host's half of the link, serving the
//! `Bridge` interface on its own stdin and stdout and answering from this
//! machine's daemon.
//!
//! It is what a link's host command runs, `ssh vm 'tmux-companion bridge'`,
//! and nothing a person types. Each method is one question to the local
//! daemon over its socket, so the bridge itself keeps no state and starts no
//! loop; the daemon answers from the pane poll it was running anyway.
//!
//! Nothing here writes to stdout but the RPC system: stdout is the link, and
//! one stray line on it is a link the other side calls broken. Diagnostics go
//! to stderr, which ssh carries back to the local side as the reason.

use std::rc::Rc;

use capnp_rpc::{RpcSystem, rpc_twoparty_capnp, twoparty};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

use super::{
    METHODS, PROTOCOL, Segment, SegmentsAnswer, Session, SessionsAnswer, WatchAnswer, Window,
    reader_options, sessions_differ, write_sessions,
};
use crate::{
    link_capnp::{bridge, session_sink},
    proto::{LinkSegmentsArgs, LinkWatchArgs, Request},
};

/// The bootstrap object: the methods it serves, and whether a daemon was
/// answering before the bridge asked it anything.
struct Bridge {
    served: Vec<String>,
    daemon_was_up: bool,
}

impl Bridge {
    /// `unimplemented` for a method this bridge was told not to serve, the
    /// same answer an older bridge gives for one it never had.
    fn gate(&self, method: &str) -> Result<(), capnp::Error> {
        if self.served.iter().any(|m| m == method) {
            Ok(())
        } else {
            Err(capnp::Error::unimplemented(format!(
                "this bridge does not serve {method}"
            )))
        }
    }
}

/// One question to this machine's daemon, starting it when nothing answers,
/// with the daemon's own error as the call's.
async fn ask(req: Request) -> Result<String, capnp::Error> {
    let resp = crate::client::send(req)
        .await
        .map_err(|e| capnp::Error::failed(format!("{e:#}")))?;
    match resp.error {
        Some(e) => Err(capnp::Error::failed(e)),
        None => Ok(resp.output),
    }
}

/// `tmux -V`, or an empty string when there is no tmux to ask.
async fn tmux_version() -> String {
    match crate::tmux::command().arg("-V").output().await {
        Ok(o) => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        Err(_) => String::new(),
    }
}

fn json<T: serde::de::DeserializeOwned>(text: &str) -> Result<T, capnp::Error> {
    serde_json::from_str(text)
        .map_err(|e| capnp::Error::failed(format!("the daemon's answer: {e}")))
}

impl bridge::Server for Bridge {
    async fn hello(
        self: Rc<Self>,
        _: bridge::HelloParams,
        mut results: bridge::HelloResults,
    ) -> Result<(), capnp::Error> {
        // A daemon that refuses its config says why here, which is what
        // the local side shows for the host, as `doctor` would locally.
        ask(Request::raw("noop", serde_json::Value::Null)).await?;
        let tmux = tmux_version().await;
        let mut r = results.get();
        r.set_build(crate::proto::build_id());
        r.set_protocol(PROTOCOL);
        r.set_started_daemon(!self.daemon_was_up);
        r.set_tmux(&tmux);
        let mut list = r.init_methods(self.served.len() as u32);
        for (i, m) in self.served.iter().enumerate() {
            list.set(i as u32, m);
        }
        Ok(())
    }

    async fn sessions(
        self: Rc<Self>,
        _: bridge::SessionsParams,
        mut results: bridge::SessionsResults,
    ) -> Result<(), capnp::Error> {
        self.gate("sessions")?;
        let answer: SessionsAnswer =
            json(&ask(Request::raw("__link_sessions", serde_json::Value::Null)).await?)?;
        let mut r = results.get();
        r.set_polled_at(answer.polled_at);
        write_sessions(
            r.init_sessions(answer.sessions.len() as u32),
            &answer.sessions,
        );
        Ok(())
    }

    async fn segments(
        self: Rc<Self>,
        params: bridge::SegmentsParams,
        mut results: bridge::SegmentsResults,
    ) -> Result<(), capnp::Error> {
        self.gate("segments")?;
        let session = super::clean(&String::from_utf8_lossy(
            params.get()?.get_session()?.as_bytes(),
        ));
        let answer: SegmentsAnswer = json(
            &ask(Request::build(
                "__link_segments",
                &LinkSegmentsArgs { session },
            ))
            .await?,
        )?;
        let mut r = results.get();
        r.set_polled_at(answer.polled_at);
        write_segments(
            r.reborrow().init_segments(answer.segments.len() as u32),
            &answer.segments,
        );
        let windows = &answer.windows[..answer.windows.len().min(super::MAX_WINDOWS)];
        write_windows(r.reborrow().init_windows(windows.len() as u32), windows);
        super::write_theme(r.init_theme(), &answer.theme);
        Ok(())
    }

    async fn inbox(
        self: Rc<Self>,
        _: bridge::InboxParams,
        mut results: bridge::InboxResults,
    ) -> Result<(), capnp::Error> {
        self.gate("inbox")?;
        let entries: Vec<crate::inbox::Entry> =
            json(&ask(Request::raw("__inbox", serde_json::Value::Null)).await?)?;
        // This machine's own agents only: one this daemon heard about from a
        // host of its own is that host's to tell.
        let entries: Vec<crate::inbox::Entry> =
            entries.into_iter().filter(|e| e.host.is_empty()).collect();
        write_entries(results.get().init_entries(entries.len() as u32), &entries);
        Ok(())
    }

    async fn dirs(
        self: Rc<Self>,
        _: bridge::DirsParams,
        mut results: bridge::DirsResults,
    ) -> Result<(), capnp::Error> {
        self.gate("dirs")?;
        // The source this machine's own config names, run by its daemon:
        // the request carries nothing, so nothing the other side sends can
        // pick what runs.
        let paths: Vec<String> =
            json(&ask(Request::raw("__link_dirs", serde_json::Value::Null)).await?)?;
        let paths = &paths[..paths.len().min(super::MAX_DIRS)];
        let mut list = results.get().init_paths(paths.len() as u32);
        for (i, p) in paths.iter().enumerate() {
            list.set(i as u32, p);
        }
        Ok(())
    }

    async fn watch(
        self: Rc<Self>,
        params: bridge::WatchParams,
        _: bridge::WatchResults,
    ) -> Result<(), capnp::Error> {
        self.gate("watch")?;
        let p = params.get()?;
        let sink = p.get_sink()?;
        let machine = super::clean(&String::from_utf8_lossy(p.get_machine()?.as_bytes()));
        let mut pushed = Pushed::default();
        let mut since = None;
        // Until the other side goes: then a call on the sink fails, and the
        // error ends this call, which nobody is waiting on any more.
        loop {
            let answer: WatchAnswer = json(
                &ask(Request::build(
                    "__link_watch",
                    &LinkWatchArgs {
                        since,
                        machine: machine.clone(),
                    },
                ))
                .await?,
            )?;
            since = Some(answer.generation);
            push(&sink, &mut pushed, answer).await?;
        }
    }

    async fn ping(
        self: Rc<Self>,
        _: bridge::PingParams,
        _: bridge::PingResults,
    ) -> Result<(), capnp::Error> {
        self.gate("ping")
    }
}

/// What a `watch` last sent, so the next answer sends only what changed.
#[derive(Default)]
struct Pushed {
    sessions: Option<Vec<Session>>,
    inbox: Option<Vec<crate::inbox::Entry>>,
    segments: std::collections::HashMap<String, (Vec<Segment>, Vec<Window>, super::Theme)>,
}

/// Whether two inbox lists differ in anything the other side keeps.
fn inbox_differs(a: &[crate::inbox::Entry], b: &[crate::inbox::Entry]) -> bool {
    a.len() != b.len()
        || a.iter().zip(b).any(|(x, y)| {
            (
                &x.id,
                &x.at,
                &x.program,
                &x.path,
                x.since,
                &x.state,
                x.question_line(),
            ) != (
                &y.id,
                &y.at,
                &y.program,
                &y.path,
                y.since,
                &y.state,
                y.question_line(),
            )
        })
}

/// The sink calls one watch answer earns against what was sent before: the
/// sessions and the inbox when they differ, each grouped session's segments
/// windows and theme when any of them differs or is new, and empty lists for
/// one no longer there.
/// Each call is waited for, so the other side hears them in order.
async fn push(
    sink: &session_sink::Client,
    pushed: &mut Pushed,
    answer: WatchAnswer,
) -> Result<(), capnp::Error> {
    let mut sessions = answer.sessions.sessions;
    sessions.truncate(super::MAX_SESSIONS);
    if pushed
        .sessions
        .as_ref()
        .is_none_or(|old| sessions_differ(old, &sessions))
    {
        let mut req = sink.sessions_request();
        req.get().set_polled_at(answer.sessions.polled_at);
        write_sessions(req.get().init_sessions(sessions.len() as u32), &sessions);
        req.send().promise.await?;
        pushed.sessions = Some(sessions);
    }
    let mut inbox: Vec<crate::inbox::Entry> = answer
        .inbox
        .into_iter()
        .filter(|e| e.host.is_empty())
        .collect();
    inbox.truncate(super::MAX_SESSIONS);
    if pushed
        .inbox
        .as_ref()
        .is_none_or(|old| inbox_differs(old, &inbox))
    {
        let mut req = sink.inbox_request();
        write_entries(req.get().init_entries(inbox.len() as u32), &inbox);
        req.send().promise.await?;
        pushed.inbox = Some(inbox);
    }
    let now = crate::panes::now_secs();
    let mut gone: Vec<String> = pushed.segments.keys().cloned().collect();
    let mut windows = answer.windows;
    let mut themes = answer.themes;
    for (grouped, segs) in answer.segments {
        gone.retain(|g| *g != grouped);
        let mut wins = windows
            .iter_mut()
            .find(|(g, _)| *g == grouped)
            .map(|(_, w)| std::mem::take(w))
            .unwrap_or_default();
        wins.truncate(super::MAX_WINDOWS);
        let theme = themes
            .iter_mut()
            .find(|(g, _)| *g == grouped)
            .map(|(_, t)| std::mem::take(t))
            .unwrap_or_default();
        let bar = (segs, wins, theme);
        if pushed.segments.get(&grouped) == Some(&bar) {
            continue;
        }
        let mut req = sink.segments_request();
        req.get().set_session(&grouped);
        req.get().set_polled_at(now);
        write_segments(req.get().init_segments(bar.0.len() as u32), &bar.0);
        write_windows(req.get().init_windows(bar.1.len() as u32), &bar.1);
        super::write_theme(req.get().init_theme(), &bar.2);
        req.send().promise.await?;
        pushed.segments.insert(grouped, bar);
    }
    for grouped in gone {
        let mut req = sink.segments_request();
        req.get().set_session(&grouped);
        req.get().set_polled_at(now);
        req.get().init_segments(0);
        req.get().init_windows(0);
        req.send().promise.await?;
        pushed.segments.remove(&grouped);
    }
    Ok(())
}

/// Write inbox entries into a list builder sized for them.
fn write_entries(
    mut list: capnp::struct_list::Builder<'_, crate::link_capnp::entry::Owned>,
    entries: &[crate::inbox::Entry],
) {
    for (i, e) in entries.iter().enumerate() {
        let mut b = list.reborrow().get(i as u32);
        b.set_pane(&e.id);
        b.set_at(&e.at);
        b.set_program(&e.program);
        b.set_path(&e.path);
        b.set_since(e.since);
        b.set_state(&e.state);
        b.set_question(e.question_line());
    }
}

/// Write segments into a list builder sized for them.
fn write_segments(
    mut list: capnp::struct_list::Builder<'_, crate::link_capnp::segment::Owned>,
    segments: &[Segment],
) {
    for (i, s) in segments.iter().enumerate() {
        let mut b = list.reborrow().get(i as u32);
        b.set_role(&s.role);
        b.set_text(&s.text);
        b.set_fg(&s.fg);
        b.set_bg(&s.bg);
        b.set_glyph(&s.glyph);
        if !s.runs.is_empty() {
            let mut runs = b.init_runs(s.runs.len() as u32);
            for (j, r) in s.runs.iter().enumerate() {
                let mut rb = runs.reborrow().get(j as u32);
                rb.set_text(&r.text);
                rb.set_fg(&r.fg);
                rb.set_bg(&r.bg);
            }
        }
    }
}

/// Write windows into a list builder sized for them.
fn write_windows(
    mut list: capnp::struct_list::Builder<'_, crate::link_capnp::window::Owned>,
    windows: &[Window],
) {
    for (i, w) in windows.iter().enumerate() {
        let mut b = list.reborrow().get(i as u32);
        b.set_index(w.index);
        b.set_name(&w.name);
        b.set_active(w.active);
        b.set_bell(w.bell);
    }
}

/// The methods to serve: every one this build has, or, with `--methods`, the
/// ones named that it has. `hello` is always served; without it there is no
/// link.
pub fn served(methods: Option<&str>) -> Vec<String> {
    match methods {
        None => METHODS.iter().map(|m| m.to_string()).collect(),
        Some(list) => {
            let named: Vec<&str> = list.split(',').map(str::trim).collect();
            METHODS
                .iter()
                .filter(|m| **m == "hello" || named.contains(m))
                .map(|m| m.to_string())
                .collect()
        }
    }
}

/// Serve the link on stdin and stdout until the other side goes.
///
/// Exits the process when it does: tokio's stdin reads on a blocking thread,
/// and a runtime dropped while that thread waits would wait with it.
pub async fn run(methods: Option<String>) -> anyhow::Result<()> {
    let bridge = Bridge {
        served: served(methods.as_deref()),
        daemon_was_up: std::os::unix::net::UnixStream::connect(crate::client::sock_path()).is_ok(),
    };
    let local = tokio::task::LocalSet::new();
    let result = local
        .run_until(async move {
            let network = twoparty::VatNetwork::new(
                futures::io::BufReader::new(tokio::io::stdin().compat()),
                futures::io::BufWriter::new(tokio::io::stdout().compat_write()),
                rpc_twoparty_capnp::Side::Server,
                reader_options(),
            );
            let client: bridge::Client = capnp_rpc::new_client(bridge);
            RpcSystem::new(Box::new(network), Some(client.client)).await
        })
        .await;
    match result {
        Ok(()) => std::process::exit(0),
        Err(e) => {
            eprintln!("tmux-companion bridge: {e}");
            std::process::exit(1)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_older_bridge_is_one_told_to_serve_fewer_methods() {
        assert_eq!(served(None), METHODS);
        assert_eq!(served(Some("sessions")), ["hello", "sessions"]);
        assert_eq!(served(Some("")), ["hello"], "hello always");
        assert_eq!(
            served(Some("dirs,inbox,bogus")),
            ["hello", "inbox", "dirs"],
            "nothing this build doesn't have"
        );
    }
}
