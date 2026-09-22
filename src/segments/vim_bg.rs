//! The old name for [`crate::segments::sh_jobs`].
//!
//! `vim-bg` asked one question with one answer baked in. The module stays for
//! one release because `has_suspended_nvim` is what the git segment calls when
//! a pane pid is passed by hand, and because deleting a name somebody has in
//! their tmux.conf is a separate decision from replacing it.

/// Whether a stopped `nvim` is a descendant of this pane.
///
/// Enumerates the whole process table, which is why the combined status side
/// never asks: it cost 18.5 ms of the segment's 26.0 ms.
pub async fn has_suspended_nvim(pane_pid: u32) -> anyhow::Result<bool> {
    crate::segments::sh_jobs::has_suspended_nvim(pane_pid).await
}

/// Render the marker with the default job table.
pub async fn render(pane_pid: u32) -> anyhow::Result<String> {
    crate::segments::sh_jobs::render(pane_pid, &crate::config::ShJobs::default()).await
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_old_output_constant_still_describes_what_is_drawn() {
        // Guards the rename: whatever `sh-jobs` draws for a suspended nvim by
        // default has to be what `vim-bg` drew.
        let out = crate::config::ShJobs::VIM_OUTPUT;
        assert!(out.contains("fg=#0262a8"));
        assert!(out.contains("fg=#539035"));
        assert!(out.contains("n"));
        assert!(out.contains("\u{f0577}"));
        assert!(out.contains("im"));
    }
}
