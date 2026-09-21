const WINDOW_CLIENTS_ICON: &str = "\u{eb7f} "; // nf-cod-device_desktop (U+EB7F)
const SESSION_CLIENTS_ICON: &str = "󱘖 "; // nf-md-account_multiple

/// Pure formatting logic extracted for unit testing.
/// `srv_clients` is the raw count from `tmux list-clients` (self already subtracted).
fn format_client_output(
    srv_clients: u32,
    session_attached: u32,
    window_active_clients: u32,
) -> String {
    if srv_clients == 0 {
        return String::new();
    }
    let s_clients = session_attached.saturating_sub(1);
    let w_clients = window_active_clients.saturating_sub(1);

    let w_part = if w_clients > 0 {
        format!("{}{}{}", WINDOW_CLIENTS_ICON, w_clients, "|")
    } else {
        String::new()
    };
    let s_part = if s_clients > 0 {
        format!("{}{}{}", SESSION_CLIENTS_ICON, s_clients, "|")
    } else {
        String::new()
    };

    format!(
        "#[fg=colour025,bg=colour033]#[fg=colour232] {}{} {} ",
        w_part, s_part, srv_clients
    )
}

pub async fn render(session_attached: u32, window_active_clients: u32) -> anyhow::Result<String> {
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        tokio::process::Command::new("tmux")
            .args(["-S", "/tmp/tmux-sock", "list-clients"])
            .output(),
    )
    .await;

    let raw_count = match result {
        Ok(Ok(o)) => String::from_utf8_lossy(&o.stdout).lines().count() as u32,
        _ => return Ok(String::new()),
    };

    if raw_count <= 1 {
        return Ok(String::new());
    }
    let srv_clients = raw_count - 1; // subtract self

    Ok(format_client_output(
        srv_clients,
        session_attached,
        window_active_clients,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_srv_clients_returns_empty() {
        assert_eq!(format_client_output(0, 2, 1), "");
    }

    #[test]
    fn one_srv_client_shows_count() {
        let out = format_client_output(1, 1, 1);
        assert!(out.contains("1 "), "expected srv count: {out}");
        assert!(out.contains("colour025"), "expected colour: {out}");
    }

    #[test]
    fn no_window_or_session_extras_when_counts_are_one() {
        // session_attached=1 → s_clients=0 (subtract self), so no session part
        // window_active=1 → w_clients=0, so no window part
        let out = format_client_output(2, 1, 1);
        assert!(!out.contains(WINDOW_CLIENTS_ICON), "no window icon: {out}");
        assert!(
            !out.contains(SESSION_CLIENTS_ICON),
            "no session icon: {out}"
        );
        assert!(out.contains("2"), "srv count present: {out}");
    }

    #[test]
    fn window_clients_shown_when_above_one() {
        let out = format_client_output(3, 1, 3); // w_active=3 → w_clients=2
        assert!(
            out.contains(WINDOW_CLIENTS_ICON),
            "missing window icon: {out}"
        );
        assert!(out.contains("2|"), "expected w_clients=2: {out}");
    }

    #[test]
    fn session_clients_shown_when_above_one() {
        let out = format_client_output(3, 3, 1); // sa=3 → s_clients=2
        assert!(
            out.contains(SESSION_CLIENTS_ICON),
            "missing session icon: {out}"
        );
        assert!(out.contains("2|"), "expected s_clients=2: {out}");
    }

    #[test]
    fn both_extras_shown() {
        let out = format_client_output(4, 3, 2); // s=2, w=1
        assert!(out.contains(WINDOW_CLIENTS_ICON));
        assert!(out.contains(SESSION_CLIENTS_ICON));
        assert!(out.contains("4"));
    }

    #[test]
    fn output_format_structure() {
        let out = format_client_output(2, 1, 1);
        assert!(out.starts_with("#[fg=colour025,bg=colour033]"));
        assert!(out.contains("#[fg=colour232]"));
        assert!(out.ends_with(' '));
    }
}
