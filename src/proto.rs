use serde::{Deserialize, Serialize};

/// One line from a client: which command, and its arguments as JSON.
#[derive(Serialize, Deserialize, Debug)]
pub struct Request {
    /// Command name, matched by the server's dispatch.
    pub cmd: String,
    /// The command's own args struct, serialised.
    pub args: serde_json::Value,
}

/// One line back: what to print, or what went wrong.
#[derive(Serialize, Deserialize, Debug)]
pub struct Response {
    /// What the client prints, which for a segment is tmux markup.
    pub output: String,
    /// Set when the command failed; the client prints it to stderr.
    pub error: Option<String>,
}

impl Response {
    /// A successful response.
    pub fn ok(output: String) -> Self {
        Self {
            output,
            error: None,
        }
    }

    /// A failed one, with empty output.
    pub fn err(e: impl std::fmt::Display) -> Self {
        Self {
            output: String::new(),
            error: Some(e.to_string()),
        }
    }
}

// ── Per-command arguments ────────────────────────────────────────────────────
//
// One struct per command, serialised by the client and deserialised by the
// handler, so the clap flags, the wire and the handler share a single
// definition.  What this replaces was a hand-built `json!` block on one side
// and `req.args["name"]` reads on the other, where a key that was never sent,
// or was sent under a different spelling, read back as `None` and silently
// changed behaviour instead of failing.
//
// `deny_unknown_fields` is what turns that class of drift into an error with a
// field name in it.  It also means a new client talking to a daemon from an
// older build gets a parse error rather than a wrong answer, which is the
// symptom the version handshake in phase 0 step 10 turns into a restart.

use std::path::PathBuf;

use crate::tmux::format::Style;

/// Arguments for `gst`, the git status segment.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct GstArgs {
    /// Repository path; `None` means the server's own working directory.
    #[serde(default)]
    pub path: Option<PathBuf>,
    /// Pane pid, which turns on the suspended-job marker.  Never sent by
    /// `status-right`: it costs a whole-process-table scan.
    #[serde(default)]
    pub pane_pid: Option<u32>,
    /// Bypass the cache for this call. The fresh result is still stored.
    #[serde(default)]
    pub force: bool,
    /// Fill, outline or outline-bright.
    #[serde(default)]
    pub style: Style,
    /// Omit the trailing end cap.
    #[serde(default)]
    pub no_cap: bool,
    /// Middle-ellipsize a branch name longer than this.
    #[serde(default)]
    pub branch_max_len: Option<usize>,
    /// Draw the git glyph before the branch name.
    #[serde(default)]
    pub branch_icon: bool,
    /// Cache freshness window in seconds.  Zero disables the cache.
    #[serde(default = "default_ttl_secs")]
    pub ttl_secs: f64,
}

/// Arguments for `status-right`, the whole right-hand side in one call.
///
/// Deliberately not `GstArgs`: there is no `pane_pid` here and no `no_cap`,
/// because the server sets both itself.  A shared struct would offer the
/// caller two fields that do nothing.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct StatusRightArgs {
    /// The active pane's directory.
    #[serde(default)]
    pub path: Option<PathBuf>,
    /// Fill, outline or outline-bright.
    #[serde(default)]
    pub style: Style,
    /// Middle-ellipsize a branch name longer than this.
    #[serde(default)]
    pub branch_max_len: Option<usize>,
    /// Draw the git glyph before the branch name.
    #[serde(default)]
    pub branch_icon: bool,
    /// Bypass the git cache for this call.
    #[serde(default)]
    pub force: bool,
    /// Git cache freshness window in seconds. Bandwidth is always live.
    #[serde(default = "default_ttl_secs")]
    pub ttl_secs: f64,
}

/// Arguments for `clients`, the multi-client indicator.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct ClientsArgs {
    /// `#{session_attached}`: clients attached to this session, including this one.
    #[serde(default)]
    pub session_attached: u32,
    /// `#{window_active_clients}`: clients with this window active.
    #[serde(default)]
    pub window_active_clients: u32,
}

/// Arguments for `vim-bg`, the suspended-editor marker.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct VimBgArgs {
    /// The pane whose descendants to look through.
    #[serde(default)]
    pub pane_pid: u32,
}

/// The default `--ttl`, kept here rather than imported from `server::state` so
/// the wire types do not depend on the server.
fn default_ttl_secs() -> f64 {
    5.0
}

impl Request {
    /// Build a request from a typed args struct.
    ///
    /// Serialisation of a struct of plain fields cannot fail, so this does not
    /// return a `Result`: a panic here would mean the struct grew a field
    /// serde cannot represent, which is a bug to fix rather than an error to
    /// handle at a call site.
    pub fn build<T: Serialize>(cmd: &str, args: &T) -> Self {
        Self {
            cmd: cmd.to_string(),
            args: serde_json::to_value(args).expect("args struct is serialisable"),
        }
    }

    /// Read this request's arguments as `T`, naming the command and the field
    /// serde tripped on.
    pub fn parse_args<T: serde::de::DeserializeOwned>(&self) -> anyhow::Result<T> {
        serde_json::from_value(self.args.clone())
            .map_err(|e| anyhow::anyhow!("invalid {} args: {}", self.cmd, e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn response_ok_has_no_error() {
        let r = Response::ok("hello".into());
        assert_eq!(r.output, "hello");
        assert!(r.error.is_none());
    }

    #[test]
    fn response_err_has_empty_output() {
        let r = Response::err("boom");
        assert_eq!(r.output, "");
        assert_eq!(r.error.as_deref(), Some("boom"));
    }

    #[test]
    fn request_serde_roundtrip() {
        let req = Request {
            cmd: "gst".into(),
            args: serde_json::json!({"path": "/tmp"}),
        };
        let json = serde_json::to_string(&req).unwrap();
        let decoded: Request = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.cmd, "gst");
        assert_eq!(decoded.args["path"], "/tmp");
    }

    #[test]
    fn response_serde_roundtrip_ok() {
        let r = Response::ok("result".into());
        let json = serde_json::to_string(&r).unwrap();
        let decoded: Response = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.output, "result");
        assert!(decoded.error.is_none());
    }

    #[test]
    fn response_serde_roundtrip_err() {
        let r = Response::err("oops");
        let json = serde_json::to_string(&r).unwrap();
        let decoded: Response = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.output, "");
        assert_eq!(decoded.error.as_deref(), Some("oops"));
    }

    #[test]
    fn request_with_null_args_field() {
        let req = Request {
            cmd: "battery".into(),
            args: serde_json::Value::Null,
        };
        let json = serde_json::to_string(&req).unwrap();
        let decoded: Request = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.cmd, "battery");
    }

    // ── typed args ───────────────────────────────────────────────────────────

    #[test]
    fn gst_args_round_trip_through_a_request() {
        let sent = GstArgs {
            path: Some(PathBuf::from("/tmp/repo")),
            pane_pid: Some(7),
            force: true,
            style: Style::Fill,
            no_cap: true,
            branch_max_len: Some(40),
            branch_icon: true,
            ttl_secs: 0.0,
        };
        let req = Request::build("gst", &sent);
        assert_eq!(req.cmd, "gst");
        assert_eq!(req.parse_args::<GstArgs>().unwrap(), sent);
    }

    #[test]
    fn gst_args_defaults_match_the_cli_defaults() {
        let req = Request {
            cmd: "gst".into(),
            args: serde_json::json!({}),
        };
        let a: GstArgs = req.parse_args().unwrap();
        assert_eq!(a.ttl_secs, 5.0);
        assert_eq!(a.style, Style::OutlineBright);
        assert!(!a.force && !a.no_cap && !a.branch_icon);
        assert!(a.path.is_none() && a.pane_pid.is_none() && a.branch_max_len.is_none());
    }

    #[test]
    fn style_crosses_the_wire_as_the_string_the_flag_uses() {
        // `--style outline-bright` and the JSON value have to agree, or a bar
        // rendered through the wire looks different from `preview`.
        let v = serde_json::to_value(Style::OutlineBright).unwrap();
        assert_eq!(v, serde_json::json!("outline-bright"));
        assert_eq!(serde_json::to_value(Style::Fill).unwrap(), "fill");
        assert_eq!(serde_json::to_value(Style::Outline).unwrap(), "outline");
    }

    #[test]
    fn an_unknown_key_names_the_command_and_the_key() {
        let req = Request {
            cmd: "clients".into(),
            args: serde_json::json!({"session_atached": 2}),
        };
        let e = req.parse_args::<ClientsArgs>().unwrap_err().to_string();
        assert!(e.contains("invalid clients args"), "{e}");
        assert!(e.contains("session_atached"), "{e}");
    }

    #[test]
    fn argument_free_commands_send_an_empty_object() {
        // `battery`, `net` and `noop` carry nothing, and the handler reads
        // nothing, so the shape only has to be something serde accepts.
        let req = Request::build("battery", &());
        assert_eq!(req.cmd, "battery");
        assert!(req.args.is_null() || req.args.as_object().is_some_and(|m| m.is_empty()));
    }

    #[test]
    fn response_with_unicode_output() {
        // Verify high-codepoint chars survive JSON round-trip.
        let icon = "\u{f0f0f}"; // U+F0F0F selected index icon
        let r = Response::ok(icon.to_string());
        let json = serde_json::to_string(&r).unwrap();
        let decoded: Response = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.output, icon);
        assert_eq!(decoded.output.as_bytes(), icon.as_bytes());
    }
}
