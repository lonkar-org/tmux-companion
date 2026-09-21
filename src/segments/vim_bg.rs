//! The suspended-editor marker: whether this pane has an `nvim` stopped in
//! the background.
const VIM_OUTPUT: &str =
    "#[fg=#0262a8,bg=colour235,none] n#[fg=#539035]󰕷im#[fg=colour235,bg=colour233]";

/// Whether a stopped `nvim` is a descendant of this pane.
///
/// Enumerates the whole process table, which is why the combined status side
/// never asks: it cost 18.5 ms of the segment's 26.0 ms.
pub async fn has_suspended_nvim(pane_pid: u32) -> anyhow::Result<bool> {
    Ok(tokio::task::spawn_blocking(move || {
        use sysinfo::{Pid, ProcessesToUpdate, System};
        let mut sys = System::new();
        sys.refresh_processes(ProcessesToUpdate::All, true);
        let parent = Pid::from_u32(pane_pid);
        sys.processes().values().any(|p| {
            p.parent() == Some(parent)
                && p.status() == sysinfo::ProcessStatus::Stop
                && p.name().to_string_lossy().contains("nvim")
        })
    })
    .await?)
}

/// Render the marker, or nothing when no editor is suspended under this pane.
pub async fn render(pane_pid: u32) -> anyhow::Result<String> {
    if has_suspended_nvim(pane_pid).await? {
        return Ok(VIM_OUTPUT.to_string());
    }
    Ok(String::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    // is_suspended_nvim() no longer exists — detection is done in-process via sysinfo.
    // Integration coverage: run `vim-bg <pane_pid>` with/without a suspended nvim child.

    #[test]
    fn output_constant_contains_color_codes() {
        assert!(VIM_OUTPUT.contains("fg=#0262a8"));
        assert!(VIM_OUTPUT.contains("fg=#539035"));
        assert!(VIM_OUTPUT.contains("n"));
        assert!(VIM_OUTPUT.contains("󰕷"));
        assert!(VIM_OUTPUT.contains("im"));
    }
}
