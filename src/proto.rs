use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct Request {
    pub cmd: String,
    pub args: serde_json::Value,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Response {
    pub output: String,
    pub error: Option<String>,
}

impl Response {
    pub fn ok(output: String) -> Self {
        Self {
            output,
            error: None,
        }
    }

    pub fn err(e: impl std::fmt::Display) -> Self {
        Self {
            output: String::new(),
            error: Some(e.to_string()),
        }
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
