use std::{sync::Arc, time::Duration};

use tokio::sync::Mutex;

use crate::{
    proto::{Request, Response},
    segments::{self, git::GstOptions},
    server::state::{DEFAULT_GST_TTL, ServerState},
};

/// Requests served since start, reported by the `__rusage` probe so a benchmark
/// can turn a CPU delta into a per-call figure.  `__rusage` itself is excluded.
static REQ_COUNT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// The tmux literal that sat between the `net` and `battery` `#()` calls in
/// `status-right`.  Emitted by the server now that the two segments arrive in
/// one response; reproduced byte for byte so the bar does not shift.
pub const RIGHT_SEPARATOR: &str = "#[reverse,fg=color237]#[bg=color237,none]";

/// Assemble the right-hand status side from its three rendered segments.
///
/// Pure, so the exact bytes — including the literals that used to live in
/// `tmux.conf` between the `#()` calls, and the trailing space that closed the
/// line — are pinned by unit tests rather than by eyeballing the status bar.
pub fn assemble_right(gst: &str, net: &str, battery: &str) -> String {
    format!("{gst}{net}{RIGHT_SEPARATOR}{battery} ")
}

pub async fn dispatch(req: Request, state: Arc<Mutex<ServerState>>) -> Response {
    if req.cmd != "__rusage" {
        REQ_COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    let result = match req.cmd.as_str() {
        "gst" => {
            let opts = gst_options(&req);
            segments::git::render(&opts, &state).await
        }
        "status-right" => render_right(&req, &state).await,
        "battery" => battery(&state).await,
        "net" => net(&state).await,
        "clients" => {
            let sa = req.args["session_attached"].as_u64().unwrap_or(0) as u32;
            let wac = req.args["window_active_clients"].as_u64().unwrap_or(0) as u32;
            segments::clients::render(sa, wac).await
        }
        "vim-bg" => {
            let pid = req.args["pane_pid"].as_u64().unwrap_or(0) as u32;
            segments::vim_bg::render(pid).await
        }
        "window" => {
            let dir_aliases = state.lock().await.dir_aliases.clone();
            match serde_json::from_value::<segments::window::WindowArgs>(req.args) {
                Ok(args) => Ok(segments::window::render(&args, &dir_aliases)),
                Err(e) => Err(anyhow::anyhow!("invalid window args: {}", e)),
            }
        }
        // Diagnostics.  Not clap subcommands users are expected to reach for;
        // `noop` prices a bare client round trip and `__rusage` is how
        // BENCHMARKS.md measures per-call server CPU.
        "noop" => Ok(String::new()),
        "__rusage" => rusage_line(),
        other => Err(anyhow::anyhow!("unknown command: {}", other)),
    };

    match result {
        Ok(output) => Response::ok(output),
        Err(e) => Response::err(e),
    }
}

/// Read `gst` options out of a request, applying the same defaults the CLI does.
fn gst_options(req: &Request) -> GstOptions {
    GstOptions {
        path: req.args["path"].as_str().map(std::path::PathBuf::from),
        pane_pid: req.args["pane_pid"].as_u64().map(|n| n as u32),
        force: req.args["force"].as_bool().unwrap_or(false),
        style: req.args["style"]
            .as_str()
            .and_then(crate::tmux::format::Style::parse)
            .unwrap_or_default(),
        no_cap: req.args["no_cap"].as_bool().unwrap_or(false),
        branch_max_len: req.args["branch_max_len"].as_u64().map(|n| n as usize),
        branch_icon: req.args["branch_icon"].as_bool().unwrap_or(false),
        ttl: req.args["ttl_secs"]
            .as_f64()
            .map(Duration::from_secs_f64)
            .unwrap_or(DEFAULT_GST_TTL),
    }
}

/// The whole right-hand status side in one response: `gst` + `net` + the tmux
/// literal + `battery`, computed concurrently.
///
/// The TTL is applied per segment, inside the individual renders — never to
/// this assembled string.  Caching the assembly would freeze `net`, which is a
/// rate: the bar would repeat one window's average until the entry expired.
async fn render_right(req: &Request, state: &Arc<Mutex<ServerState>>) -> anyhow::Result<String> {
    let opts = GstOptions {
        // The git segment opens the right-hand side, so it never draws an end
        // cap — this is what `--no-cap` did in the old three-call conf.
        no_cap: true,
        // Deliberately not plumbed from the request: see `GstOptions::pane_pid`.
        pane_pid: None,
        ..gst_options(req)
    };

    // All three run concurrently.  `net`'s expensive half is the counter read,
    // which touches no shared state; its arithmetic needs `&mut ServerState`
    // and is applied afterwards, so nothing here holds a lock across an await.
    let (gst, battery, net_sample) = tokio::join!(
        segments::git::render(&opts, state),
        battery(state),
        segments::network::sample(),
    );

    let net = match net_sample {
        Ok((rx, tx)) => {
            let mut st = state.lock().await;
            let ServerState {
                net_previous,
                net_last_render,
                ..
            } = &mut *st;
            segments::network::advance(
                net_previous,
                net_last_render,
                rx,
                tx,
                std::time::Instant::now(),
            )
        }
        Err(e) => {
            eprintln!("tmux-companion: net segment failed: {e}");
            String::new()
        }
    };

    // A failing segment must not blank the whole side, but it must not vanish
    // silently either — a permanently empty git segment is otherwise
    // indistinguishable from a directory that is not a repository.
    Ok(assemble_right(
        &segment_or_empty("gst", gst),
        &net,
        &segment_or_empty("battery", battery),
    ))
}

fn segment_or_empty(name: &str, result: anyhow::Result<String>) -> String {
    match result {
        Ok(s) => s,
        Err(e) => {
            eprintln!("tmux-companion: {name} segment failed: {e}");
            String::new()
        }
    }
}

/// Battery, behind its 30-second cache.  Shared by the `battery` command and
/// the combined side so the two cannot drift apart.
async fn battery(state: &Arc<Mutex<ServerState>>) -> anyhow::Result<String> {
    if let Some(cached) = state.lock().await.battery_cached() {
        return Ok(cached);
    }
    let rendered = segments::battery::render().await?;
    // Don't cache while charging — the percentage changes every second.
    if !rendered.contains(segments::battery::CHARGING_ICON) {
        state.lock().await.battery_store(rendered.clone());
    }
    Ok(rendered)
}

/// Bandwidth.  Never cached: it is a rate, and the user wants it live.  At
/// 1.05 ms per call there is nothing worth caching anyway.
async fn net(state: &Arc<Mutex<ServerState>>) -> anyhow::Result<String> {
    let (rx, tx) = segments::network::sample().await?;
    let mut st = state.lock().await;
    let ServerState {
        net_previous,
        net_last_render,
        ..
    } = &mut *st;
    Ok(segments::network::advance(
        net_previous,
        net_last_render,
        rx,
        tx,
        std::time::Instant::now(),
    ))
}

/// `utime_us stime_us cutime_us cstime_us request_count` — the server's own
/// `getrusage(2)` counters in microseconds, self and reaped children.
///
/// This exists because nothing outside the process can measure per-call server
/// CPU at the resolution the question needs: `top` quantises to 10 ms and `ps`
/// to a whole second, while a status bar segment costs single-digit
/// milliseconds.  See BENCHMARKS.md.
fn rusage_line() -> anyhow::Result<String> {
    use nix::sys::resource::{UsageWho, getrusage};

    fn micros(t: nix::sys::time::TimeVal) -> i64 {
        t.tv_sec() * 1_000_000 + i64::from(t.tv_usec())
    }

    let me = getrusage(UsageWho::RUSAGE_SELF)?;
    let kids = getrusage(UsageWho::RUSAGE_CHILDREN)?;
    Ok(format!(
        "{} {} {} {} {}",
        micros(me.user_time()),
        micros(me.system_time()),
        micros(kids.user_time()),
        micros(kids.system_time()),
        REQ_COUNT.load(std::sync::atomic::Ordering::Relaxed),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── assemble_right: exact bytes ──────────────────────────────────────────
    //
    // These pin the replacement for three tmux.conf lines:
    //   set -g  status-right "#(tmux-companion gst --no-cap … )"
    //   set -ga status-right "#(tmux-companion net)"
    //   set -ga status-right "#[reverse,fg=color237]#[bg=color237,none]#(… battery) "

    #[test]
    fn assembles_the_three_segments_and_literals_exactly() {
        assert_eq!(
            assemble_right("GST", "NET", "BAT"),
            "GSTNET#[reverse,fg=color237]#[bg=color237,none]BAT "
        );
    }

    #[test]
    fn separator_literal_is_byte_for_byte_the_conf_literal() {
        // Spelled out rather than referencing the constant: the point of the
        // test is that the constant has not drifted from the shipped conf.
        assert_eq!(RIGHT_SEPARATOR, "#[reverse,fg=color237]#[bg=color237,none]");
    }

    #[test]
    fn assembly_ends_with_exactly_one_trailing_space() {
        let out = assemble_right("a", "b", "c");
        assert!(out.ends_with("c "), "battery then one space: {out:?}");
        assert!(!out.ends_with("  "), "not two spaces: {out:?}");
    }

    #[test]
    fn segment_order_is_gst_then_net_then_battery() {
        let out = assemble_right("<G>", "<N>", "<B>");
        let g = out.find("<G>").expect("gst present");
        let n = out.find("<N>").expect("net present");
        let b = out.find("<B>").expect("battery present");
        assert!(g < n && n < b, "wrong order: {out:?}");
    }

    #[test]
    fn separator_sits_between_net_and_battery_only() {
        let out = assemble_right("<G>", "<N>", "<B>");
        let sep = out.find(RIGHT_SEPARATOR).expect("separator present");
        assert!(out.find("<N>").unwrap() < sep, "separator follows net");
        assert!(sep < out.find("<B>").unwrap(), "separator precedes battery");
        assert_eq!(
            out.matches(RIGHT_SEPARATOR).count(),
            1,
            "separator appears once"
        );
    }

    #[test]
    fn empty_segments_still_produce_the_literals() {
        // A non-repo pane on a quiet network: gst and net are both empty, and
        // the battery segment must still be separated and spaced correctly.
        assert_eq!(
            assemble_right("", "", "BAT"),
            "#[reverse,fg=color237]#[bg=color237,none]BAT "
        );
    }

    #[test]
    fn all_empty_is_just_the_literals() {
        assert_eq!(
            assemble_right("", "", ""),
            "#[reverse,fg=color237]#[bg=color237,none] "
        );
    }

    #[test]
    fn assembly_equals_the_old_three_call_concatenation() {
        // The exact string tmux used to build from three #() substitutions plus
        // the conf literals, written out independently of the implementation.
        let gst = "#[fg=color251,bg=color233] main";
        let net = "#[fg=#5cae36]<40KiB/s";
        let bat = "#[fg=color250,bg=color237] 87%";
        let old = format!(
            "{gst}{net}#[reverse,fg=color237]#[bg=color237,none]{bat} "
        );
        assert_eq!(assemble_right(gst, net, bat), old);
    }

    #[test]
    fn assembly_preserves_high_codepoint_glyphs() {
        let icon = "\u{f0f0f}";
        let out = assemble_right(icon, "", "");
        assert!(out.starts_with(icon));
        assert_eq!(out.as_bytes()[..icon.len()], *icon.as_bytes());
    }

    // ── gst_options ──────────────────────────────────────────────────────────

    fn req(args: serde_json::Value) -> Request {
        Request {
            cmd: "gst".into(),
            args,
        }
    }

    #[test]
    fn gst_options_default_ttl_is_five_seconds() {
        let o = gst_options(&req(serde_json::json!({})));
        assert_eq!(o.ttl, DEFAULT_GST_TTL);
        assert_eq!(o.ttl, Duration::from_secs(5));
    }

    #[test]
    fn gst_options_ttl_flag_overrides_the_default() {
        let o = gst_options(&req(serde_json::json!({"ttl_secs": 0.5})));
        assert_eq!(o.ttl, Duration::from_millis(500));
    }

    #[test]
    fn gst_options_zero_ttl_is_preserved_not_defaulted() {
        let o = gst_options(&req(serde_json::json!({"ttl_secs": 0})));
        assert_eq!(o.ttl, Duration::ZERO);
    }

    #[test]
    fn gst_options_reads_every_flag() {
        let o = gst_options(&req(serde_json::json!({
            "path": "/tmp/x",
            "pane_pid": 4242,
            "force": true,
            "no_cap": true,
            "branch_max_len": 40,
            "branch_icon": true,
        })));
        assert_eq!(o.path, Some(std::path::PathBuf::from("/tmp/x")));
        assert_eq!(o.pane_pid, Some(4242));
        assert!(o.force);
        assert!(o.no_cap);
        assert_eq!(o.branch_max_len, Some(40));
        assert!(o.branch_icon);
    }

    #[test]
    fn status_right_never_forwards_a_pane_pid() {
        // The whole point of item 2: even if a client sends one, the combined
        // side must not resolve it, because that means scanning every process
        // on the machine.
        let r = Request {
            cmd: "status-right".into(),
            args: serde_json::json!({"path": "/tmp/x", "pane_pid": 4242}),
        };
        let opts = GstOptions {
            no_cap: true,
            pane_pid: None,
            ..gst_options(&r)
        };
        assert_eq!(opts.pane_pid, None, "pane_pid must be dropped");
        assert!(opts.no_cap, "gst opens the right side, so no end cap");
        assert_eq!(opts.path, Some(std::path::PathBuf::from("/tmp/x")));
    }

    #[test]
    fn standalone_gst_still_honours_a_pane_pid() {
        // The manual `gst <path> <pid>` form keeps working.
        let o = gst_options(&req(serde_json::json!({"path": "/x", "pane_pid": 7})));
        assert_eq!(o.pane_pid, Some(7));
    }

    // ── dispatch ─────────────────────────────────────────────────────────────

    fn state() -> Arc<Mutex<ServerState>> {
        Arc::new(Mutex::new(ServerState::new()))
    }

    #[tokio::test]
    async fn unknown_command_is_an_error() {
        let r = dispatch(
            Request {
                cmd: "nope".into(),
                args: serde_json::Value::Null,
            },
            state(),
        )
        .await;
        assert!(r.error.is_some());
    }

    #[tokio::test]
    async fn noop_succeeds_with_empty_output() {
        let r = dispatch(
            Request {
                cmd: "noop".into(),
                args: serde_json::Value::Null,
            },
            state(),
        )
        .await;
        assert!(r.error.is_none(), "{:?}", r.error);
        assert_eq!(r.output, "");
    }

    #[tokio::test]
    async fn rusage_probe_reports_five_counters() {
        let r = dispatch(
            Request {
                cmd: "__rusage".into(),
                args: serde_json::Value::Null,
            },
            state(),
        )
        .await;
        assert!(r.error.is_none(), "{:?}", r.error);
        let fields: Vec<&str> = r.output.split_whitespace().collect();
        assert_eq!(fields.len(), 5, "got {:?}", r.output);
        for f in &fields {
            assert!(f.parse::<i64>().is_ok(), "non-numeric field {f:?}");
        }
    }

    #[tokio::test]
    async fn gst_on_a_non_repo_path_renders_empty() {
        let dir = tempfile::tempdir().unwrap();
        let r = dispatch(
            Request {
                cmd: "gst".into(),
                args: serde_json::json!({"path": dir.path().to_string_lossy()}),
            },
            state(),
        )
        .await;
        assert!(r.error.is_none(), "{:?}", r.error);
        assert_eq!(r.output, "");
    }

    #[tokio::test]
    async fn status_right_on_a_non_repo_path_still_emits_the_literals() {
        // gst is empty, so the response is the separator, whatever battery
        // produced, and the trailing space — never a bare empty string.
        let dir = tempfile::tempdir().unwrap();
        let r = dispatch(
            Request {
                cmd: "status-right".into(),
                args: serde_json::json!({"path": dir.path().to_string_lossy()}),
            },
            state(),
        )
        .await;
        assert!(r.error.is_none(), "{:?}", r.error);
        assert!(
            r.output.contains(RIGHT_SEPARATOR),
            "missing separator: {:?}",
            r.output
        );
        assert!(r.output.ends_with(' '), "missing trailing space: {:?}", r.output);
    }

    #[tokio::test]
    async fn repeated_status_right_calls_are_consistent() {
        // Second call is served from the git and battery caches; the shape of
        // the output must not change between a cold and a warm call.
        let dir = tempfile::tempdir().unwrap();
        let st = state();
        let make = || Request {
            cmd: "status-right".into(),
            args: serde_json::json!({"path": dir.path().to_string_lossy()}),
        };
        let a = dispatch(make(), Arc::clone(&st)).await;
        let b = dispatch(make(), Arc::clone(&st)).await;
        assert!(a.error.is_none() && b.error.is_none());
        assert!(a.output.contains(RIGHT_SEPARATOR));
        assert!(b.output.contains(RIGHT_SEPARATOR));
    }
}
