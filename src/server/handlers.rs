use std::{sync::Arc, time::Duration};

use tokio::sync::Mutex;

use crate::{
    proto::{ClientsArgs, GstArgs, Request, Response, StatusRightArgs, VimBgArgs},
    segments::{self, git::GstOptions},
    server::state::{DEFAULT_GST_TTL, ServerState},
};

/// Requests served since start, reported by the `__rusage` probe so a benchmark
/// can turn a CPU delta into a per-call figure.  `__rusage` itself is excluded.
static REQ_COUNT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// The tmux literal that sat between the `net` and `battery` `#()` calls in
/// `status-right`.  Emitted by the server now that the two segments arrive in
/// one response; reproduced byte for byte so the bar does not shift.
///
/// The `ARROW_RIGHT` in the middle is load-bearing and was missed on the first
/// pass: the conf's literal is `#[reverse,fg=color237]<glyph>#[bg=…]`, and
/// dropping the glyph silently removes the powerline wedge in front of the
/// battery.  It cost 1 character out of 252, every test agreed with itself,
/// and only a diff against the real `tmux.conf` caught it.  Built from the
/// icons constant rather than a pasted codepoint so it tracks a change to the
/// glyph (see CLAUDE.md's ARROW_RIGHT invariant).
/// Only the tests call this now: the live separator comes from
/// `[status.right]`, whose default carries the same literal as a template.
/// Kept as the independent definition those tests check the config default
/// against, so the two cannot drift apart unnoticed.
#[cfg(test)]
pub fn right_separator() -> String {
    format!(
        "#[reverse,fg=color237]{}#[bg=color237,none]",
        crate::tmux::icons::ARROW_RIGHT
    )
}

/// Assemble the right-hand status side from its three rendered segments.
///
/// Pure, so the exact bytes — including the literals that used to live in
/// `tmux.conf` between the `#()` calls, and the trailing space that closed the
/// line — are pinned by unit tests rather than by eyeballing the status bar.
/// Assemble the right-hand side the way the hardcoded version did.
///
/// Test-only for the same reason as `right_separator`: production assembles
/// from the configured list, and this is what the pinned byte-for-byte tests
/// compare that against.
#[cfg(test)]
pub fn assemble_right(gst: &str, net: &str, battery: &str) -> String {
    assemble_right_with(&crate::config::StatusRight::default(), gst, net, battery)
}

/// Assemble the right-hand side from a configured segment list.
///
/// A separator is drawn whether or not the segment after it rendered anything,
/// which is what the tmux.conf literals did: they sat in the format string
/// unconditionally, so a non-repo pane on a quiet network still drew the wedge
/// in front of an absent battery. Dropping the separator with its segment
/// would be a tidier bar and a different one, and the pinned tests exist to
/// catch exactly that kind of silent improvement.
pub fn assemble_right_with(
    right: &crate::config::StatusRight,
    gst: &str,
    net: &str,
    battery: &str,
) -> String {
    use crate::config::SegmentName;

    let mut out = String::new();
    for segment in &right.segments {
        let rendered = match segment.name {
            SegmentName::Git => gst,
            SegmentName::Net => net,
            SegmentName::Battery => battery,
        };
        out.push_str(&crate::config::expand_glyphs(&segment.separator_before));
        out.push_str(rendered);
    }
    if right.trailing_space {
        out.push(' ');
    }
    out
}

pub async fn dispatch(req: Request, state: Arc<Mutex<ServerState>>) -> Response {
    if req.cmd != "__rusage" {
        REQ_COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    let result = match req.cmd.as_str() {
        "gst" => match req.parse_args::<GstArgs>() {
            Ok(args) => {
                let mut opts = gst_options(&args);
                opts.parts = state.lock().await.config.git.parts.clone();
                segments::git::render(&opts, &state).await
            }
            Err(e) => Err(e),
        },
        "status-right" => match req.parse_args::<StatusRightArgs>() {
            Ok(args) => render_right(&args, &state).await,
            Err(e) => Err(e),
        },
        "battery" => battery(&state).await,
        "net" => net(&state).await,
        "clients" => match req.parse_args::<ClientsArgs>() {
            Ok(a) => segments::clients::render(a.session_attached, a.window_active_clients).await,
            Err(e) => Err(e),
        },
        "vim-bg" => match req.parse_args::<VimBgArgs>() {
            Ok(a) => segments::vim_bg::render(a.pane_pid).await,
            Err(e) => Err(e),
        },
        "window" => {
            let dir_aliases = state.lock().await.dir_aliases.clone();
            match req.parse_args::<segments::window::WindowArgs>() {
                Ok(args) => Ok(segments::window::render(&args, &dir_aliases)),
                Err(e) => Err(e),
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
        // The glyph preset is applied here, once, to whatever a segment drew.
        // Rendering keeps using the constants in `tmux::icons` throughout; see
        // `GlyphMap` for why the substitution lives at the edge rather than
        // being threaded through every call site.
        Ok(output) => {
            let glyphs = state.lock().await.glyphs.clone();
            Response::ok(glyphs.apply(&output).into_owned())
        }
        Err(e) => Response::err(e),
    }
}

/// Turn the wire's `gst` arguments into the options the segment renders from.
///
/// The only work left here is the TTL, which travels as seconds and is used as
/// a `Duration`. Everything else is carried by the struct itself, so a field
/// added to one end is a compile error at the other rather than a key that
/// quietly reads back as `None`.
fn gst_options(args: &GstArgs) -> GstOptions {
    GstOptions {
        path: args.path.clone(),
        pane_pid: args.pane_pid,
        force: args.force,
        style: args.style,
        no_cap: args.no_cap,
        branch_max_len: args.branch_max_len,
        branch_icon: args.branch_icon,
        ttl: duration_from_secs(args.ttl_secs),
        // Filled by the caller from the daemon's config, which this function
        // has no access to and does not want: it stays a pure mapping from the
        // wire to the options.
        parts: crate::config::GitPart::all(),
    }
}

/// Seconds to a `Duration`, refusing to turn a negative or non-finite number
/// into a panic. `Duration::from_secs_f64` panics on both.
fn duration_from_secs(secs: f64) -> Duration {
    if secs.is_finite() && secs >= 0.0 {
        Duration::from_secs_f64(secs)
    } else {
        DEFAULT_GST_TTL
    }
}

/// The whole right-hand status side in one response: `gst` + `net` + the tmux
/// literal + `battery`, computed concurrently.
///
/// The TTL is applied per segment, inside the individual renders — never to
/// this assembled string.  Caching the assembly would freeze `net`, which is a
/// rate: the bar would repeat one window's average until the entry expired.
async fn render_right(
    args: &StatusRightArgs,
    state: &Arc<Mutex<ServerState>>,
) -> anyhow::Result<String> {
    let opts = GstOptions {
        path: args.path.clone(),
        force: args.force,
        style: args.style,
        branch_max_len: args.branch_max_len,
        branch_icon: args.branch_icon,
        ttl: duration_from_secs(args.ttl_secs),
        // The git segment opens the right-hand side, so it never draws an end
        // cap — this is what `--no-cap` did in the old three-call conf.
        no_cap: true,
        // Deliberately not plumbed from the request: see `GstOptions::pane_pid`.
        pane_pid: None,
        parts: state.lock().await.config.git.parts.clone(),
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
    let right = state.lock().await.config.status.right.clone();
    Ok(assemble_right_with(
        &right,
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
    use crate::tmux::icons::ARROW_RIGHT;

    // ── assemble_right: exact bytes ──────────────────────────────────────────
    //
    // These pin the replacement for three tmux.conf lines:
    //   set -g  status-right "#(tmux-companion gst --no-cap … )"
    //   set -ga status-right "#(tmux-companion net)"
    //   set -ga status-right "#[reverse,fg=color237]<ARROW_RIGHT>#[bg=color237,none]#(… battery) "

    /// The conf literal, written from the icons constant so a glyph change
    /// propagates here rather than being pinned to a stale codepoint.
    fn sep() -> String {
        format!("#[reverse,fg=color237]{ARROW_RIGHT}#[bg=color237,none]")
    }

    #[test]
    fn assembles_the_three_segments_and_literals_exactly() {
        assert_eq!(
            assemble_right("GST", "NET", "BAT"),
            format!("GSTNET{}BAT ", sep())
        );
    }

    #[test]
    fn separator_literal_is_byte_for_byte_the_conf_literal() {
        // The wedge between the two format directives is the whole point of
        // this separator, and its absence is invisible in every other test:
        // one character out of 252, no colour change, no layout shift that
        // reads as broken. The first implementation dropped it and shipped a
        // green suite. Assert the glyph explicitly.
        let sep = right_separator();
        assert!(
            sep.contains(ARROW_RIGHT),
            "powerline wedge missing: {sep:?}"
        );
        assert_eq!(
            sep,
            format!("#[reverse,fg=color237]{ARROW_RIGHT}#[bg=color237,none]")
        );
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
        let separator = sep();
        let sep = out.find(&separator).expect("separator present");
        assert!(out.find("<N>").unwrap() < sep, "separator follows net");
        assert!(sep < out.find("<B>").unwrap(), "separator precedes battery");
        assert_eq!(out.matches(&separator).count(), 1, "separator appears once");
    }

    #[test]
    fn empty_segments_still_produce_the_literals() {
        // A non-repo pane on a quiet network: gst and net are both empty, and
        // the battery segment must still be separated and spaced correctly.
        assert_eq!(assemble_right("", "", "BAT"), format!("{}BAT ", sep()));
    }

    #[test]
    fn all_empty_is_just_the_literals() {
        assert_eq!(assemble_right("", "", ""), format!("{} ", sep()));
    }

    #[test]
    fn the_configured_default_separator_is_the_conf_literal() {
        // `right_separator` is the independent definition and the config
        // default is a template; this is what stops them drifting.
        let right = crate::config::StatusRight::default();
        let battery = right
            .segments
            .iter()
            .find(|s| s.name == crate::config::SegmentName::Battery)
            .expect("battery is on the default side");
        assert_eq!(
            crate::config::expand_glyphs(&battery.separator_before),
            right_separator()
        );
    }

    // ── configured sides ─────────────────────────────────────────────────────

    #[test]
    fn the_default_side_is_the_hardcoded_one() {
        // `assemble_right` is now `assemble_right_with` against the defaults,
        // so this is the test that keeps those two from drifting apart.
        let right = crate::config::StatusRight::default();
        assert_eq!(
            assemble_right_with(&right, "G", "N", "B"),
            assemble_right("G", "N", "B")
        );
    }

    #[test]
    fn a_segment_can_be_left_off_the_side_entirely() {
        use crate::config::{RightSegment, SegmentName, StatusRight};
        let right = StatusRight {
            segments: vec![RightSegment {
                name: SegmentName::Git,
                separator_before: String::new(),
            }],
            trailing_space: true,
        };
        assert_eq!(assemble_right_with(&right, "G", "N", "B"), "G ");
    }

    #[test]
    fn segments_can_be_reordered() {
        use crate::config::{RightSegment, SegmentName, StatusRight};
        let right = StatusRight {
            segments: vec![
                RightSegment {
                    name: SegmentName::Battery,
                    separator_before: String::new(),
                },
                RightSegment {
                    name: SegmentName::Git,
                    separator_before: String::new(),
                },
            ],
            trailing_space: false,
        };
        assert_eq!(assemble_right_with(&right, "G", "N", "B"), "BG");
    }

    #[test]
    fn an_empty_separator_means_nothing_between_two_segments() {
        use crate::config::{RightSegment, SegmentName, StatusRight};
        let right = StatusRight {
            segments: vec![
                RightSegment {
                    name: SegmentName::Net,
                    separator_before: String::new(),
                },
                RightSegment {
                    name: SegmentName::Battery,
                    separator_before: String::new(),
                },
            ],
            trailing_space: false,
        };
        assert_eq!(assemble_right_with(&right, "G", "N", "B"), "NB");
    }

    #[test]
    fn a_separator_expands_glyph_placeholders() {
        use crate::config::{RightSegment, SegmentName, StatusRight};
        let right = StatusRight {
            segments: vec![RightSegment {
                name: SegmentName::Battery,
                separator_before: "<{ARROW_RIGHT}>".to_string(),
            }],
            trailing_space: false,
        };
        assert_eq!(
            assemble_right_with(&right, "G", "N", "B"),
            format!("<{}>B", crate::tmux::icons::ARROW_RIGHT)
        );
    }

    #[test]
    fn the_trailing_space_can_be_turned_off() {
        let right = crate::config::StatusRight {
            trailing_space: false,
            ..Default::default()
        };
        let with = assemble_right("G", "N", "B");
        let without = assemble_right_with(&right, "G", "N", "B");
        assert_eq!(with, format!("{without} "));
    }

    #[test]
    fn assembly_equals_the_old_three_call_concatenation() {
        // The exact string tmux used to build from three #() substitutions plus
        // the conf literals, written out independently of the implementation.
        let gst = "#[fg=color251,bg=color233] main";
        let net = "#[fg=#5cae36]<40KiB/s";
        let bat = "#[fg=color250,bg=color237] 87%";
        let old = format!("{gst}{net}#[reverse,fg=color237]{ARROW_RIGHT}#[bg=color237,none]{bat} ");
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

    /// The tests drive `gst_options` through the deserializer rather than
    /// building the struct by hand, so they cover the wire shape too.
    fn args(json: serde_json::Value) -> GstArgs {
        Request {
            cmd: "gst".into(),
            args: json,
        }
        .parse_args()
        .expect("valid gst args")
    }

    #[test]
    fn gst_options_default_ttl_is_five_seconds() {
        let o = gst_options(&args(serde_json::json!({})));
        assert_eq!(o.ttl, DEFAULT_GST_TTL);
        assert_eq!(o.ttl, Duration::from_secs(5));
    }

    #[test]
    fn gst_options_ttl_flag_overrides_the_default() {
        let o = gst_options(&args(serde_json::json!({"ttl_secs": 0.5})));
        assert_eq!(o.ttl, Duration::from_millis(500));
    }

    #[test]
    fn gst_options_zero_ttl_is_preserved_not_defaulted() {
        let o = gst_options(&args(serde_json::json!({"ttl_secs": 0})));
        assert_eq!(o.ttl, Duration::ZERO);
    }

    #[test]
    fn gst_options_reads_every_flag() {
        let o = gst_options(&args(serde_json::json!({
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
    fn a_misspelled_gst_key_is_an_error_rather_than_a_default() {
        // The reason the typed structs exist. `branch_maxlen` used to read
        // back as `None` and quietly render an untruncated branch.
        let r = Request {
            cmd: "gst".into(),
            args: serde_json::json!({"branch_maxlen": 40}),
        };
        let e = r.parse_args::<GstArgs>().unwrap_err().to_string();
        assert!(e.contains("invalid gst args"), "{e}");
        assert!(e.contains("branch_maxlen"), "{e}");
    }

    #[test]
    fn a_negative_ttl_falls_back_instead_of_panicking() {
        // `Duration::from_secs_f64` panics on a negative or non-finite value,
        // and the TTL arrives from a client this server does not control.
        assert_eq!(duration_from_secs(-1.0), DEFAULT_GST_TTL);
        assert_eq!(duration_from_secs(f64::NAN), DEFAULT_GST_TTL);
        assert_eq!(duration_from_secs(f64::INFINITY), DEFAULT_GST_TTL);
    }

    #[test]
    fn status_right_cannot_carry_a_pane_pid_at_all() {
        // The whole point of item 2: the combined side must never resolve a
        // pane pid, because that means scanning every process on the machine.
        // It used to be dropped in `render_right`; now the argument struct has
        // no such field, so a client sending one gets an error.
        let r = Request {
            cmd: "status-right".into(),
            args: serde_json::json!({"path": "/tmp/x", "pane_pid": 4242}),
        };
        let e = r.parse_args::<StatusRightArgs>().unwrap_err().to_string();
        assert!(e.contains("pane_pid"), "{e}");
    }

    #[test]
    fn standalone_gst_still_honours_a_pane_pid() {
        // The manual `gst <path> <pid>` form keeps working.
        let o = gst_options(&args(serde_json::json!({"path": "/x", "pane_pid": 7})));
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
            r.output.contains(&right_separator()),
            "missing separator: {:?}",
            r.output
        );
        assert!(
            r.output.ends_with(' '),
            "missing trailing space: {:?}",
            r.output
        );
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
        assert!(a.output.contains(&right_separator()));
        assert!(b.output.contains(&right_separator()));
    }
}
