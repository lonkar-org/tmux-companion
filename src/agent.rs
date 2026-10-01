//! What an agent says about itself, and the hooks that make it say so.
//!
//! Silence is a poor proxy for a question. An agent running a two-minute
//! build draws nothing and reads as waiting; one that has answered and sits at
//! its prompt reads the same as one that stopped on a permission box. The
//! agents themselves know which, and the ones with hooks can say: `agent busy`
//! when a prompt is sent or a tool has run, `agent asked` when a permission
//! prompt or a question goes up, `agent done` when the answer is complete.
//! The daemon keeps the last word per pane and it wins over the window's
//! quiet time on the bar, in the inbox, in `panes` and in the brief.
//!
//! `agent hooks claude` prints the block of `settings.json` that wires this
//! up, the way `shell-init` prints the prompt marks. Only claude so far; a
//! second program goes in [`hooks`](crate::agent::hooks) beside it.
//!
//! The report runs from inside the agent's own hook, so it never fails out
//! loud: an error there would land in the agent's transcript, and a bar that
//! is a few seconds behind is the better outcome.

use clap::Subcommand;

use crate::panes::Report;

/// `agent`: say what an agent is doing, or print the hooks that will.
#[derive(Subcommand, Debug, Clone, PartialEq, Eq)]
pub enum AgentAction {
    /// Working: a prompt was sent, or a tool just ran
    Busy {
        /// The pane, defaulting to $TMUX_PANE, which is the hook's own
        #[arg(long, value_name = "ID")]
        pane: Option<String>,
    },
    /// Stopped on a question or a permission prompt
    Asked {
        /// The pane, defaulting to $TMUX_PANE, which is the hook's own
        #[arg(long, value_name = "ID")]
        pane: Option<String>,
    },
    /// Answered, and waiting for the next prompt
    Done {
        /// The pane, defaulting to $TMUX_PANE, which is the hook's own
        #[arg(long, value_name = "ID")]
        pane: Option<String>,
    },
    /// Forget what the pane last said, so the window's quiet time decides
    /// again: for an agent restarted without its hooks
    Clear {
        /// The pane, defaulting to $TMUX_PANE
        #[arg(long, value_name = "ID")]
        pane: Option<String>,
    },
    /// Print the hooks block for an agent's settings file, so it reports
    /// busy, asked and done as they happen
    Hooks {
        /// Which agent: `claude` is the only one so far
        #[arg(default_value = "claude")]
        program: String,
    },
}

/// The hooks block for a program, as JSON to merge into its settings, or
/// nothing for a program this knows no hooks for.
///
/// claude: a prompt sent and a tool finished are `busy`; a permission box,
/// an `AskUserQuestion` going up and an MCP server asking are `asked`; the
/// end of a response is `done`. `idle_prompt` is deliberately not `asked`:
/// it fires a minute after every answer and would turn each `done` orange.
/// A session starting names the pane `claude` for key routing, since claude's
/// process is named for its version, and records that version as the claim's
/// owner; a session ending takes the claim back.
pub fn hooks(program: &str) -> Option<String> {
    match program.trim() {
        "claude" | "claude-code" => Some(
            r#"{
  "hooks": {
    "UserPromptSubmit": [
      { "hooks": [{ "type": "command", "command": "tmux-companion agent busy" }] }
    ],
    "PreToolUse": [
      {
        "matcher": "AskUserQuestion",
        "hooks": [{ "type": "command", "command": "tmux-companion agent asked" }]
      }
    ],
    "PostToolUse": [
      { "hooks": [{ "type": "command", "command": "tmux-companion agent busy" }] }
    ],
    "Notification": [
      {
        "matcher": "permission_prompt|agent_needs_input|elicitation_dialog|elicitation_url_dialog",
        "hooks": [{ "type": "command", "command": "tmux-companion agent asked" }]
      }
    ],
    "Stop": [
      { "hooks": [{ "type": "command", "command": "tmux-companion agent done" }] }
    ],
    "SessionStart": [
      { "hooks": [{ "type": "command", "command": "tmux-companion keys claim --app claude --owner --quiet" }] }
    ],
    "SessionEnd": [
      { "hooks": [{ "type": "command", "command": "tmux-companion keys release --quiet" }] }
    ]
  }
}
"#
            .to_string(),
        ),
        _ => None,
    }
}

/// The programs [`hooks`] knows, for the error when it does not know one.
pub const KNOWN: &[&str] = &["claude"];

/// Tell the daemon what the agent in `pane` is doing, or with `None` that
/// it no longer knows.
///
/// Nothing is printed and the exit is 0 whatever happens: outside tmux there
/// is no pane to speak of, and a daemon that cannot be reached will read the
/// window's quiet time instead, which is what it did before hooks existed.
pub async fn report(state: Option<Report>, pane: Option<String>) -> anyhow::Result<()> {
    let Some(pane) = pane
        .or_else(|| std::env::var("TMUX_PANE").ok())
        .filter(|p| !p.trim().is_empty())
    else {
        return Ok(());
    };
    let args = crate::proto::AgentArgs { pane, state };
    let _ = crate::client::send(crate::proto::Request::build("__agent", &args)).await;
    Ok(())
}

/// `agent`: dispatch.
pub async fn run(action: AgentAction) -> anyhow::Result<()> {
    match action {
        AgentAction::Busy { pane } => report(Some(Report::Busy), pane).await,
        AgentAction::Asked { pane } => report(Some(Report::Asked), pane).await,
        AgentAction::Done { pane } => report(Some(Report::Done), pane).await,
        AgentAction::Clear { pane } => report(None, pane).await,
        AgentAction::Hooks { program } => match hooks(&program) {
            Some(text) => {
                print!("{text}");
                Ok(())
            }
            None => anyhow::bail!(
                "no hooks known for {program}; this knows {}",
                KNOWN.join(", ")
            ),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_claude_block_is_json_and_names_each_state_once_per_event() {
        let text = hooks("claude").unwrap();
        let v: serde_json::Value = serde_json::from_str(&text).expect("parses");
        let events = v["hooks"].as_object().unwrap();
        let command = |event: &str, i: usize| {
            events[event][i]["hooks"][0]["command"]
                .as_str()
                .unwrap()
                .to_string()
        };
        assert_eq!(command("UserPromptSubmit", 0), "tmux-companion agent busy");
        assert_eq!(command("PostToolUse", 0), "tmux-companion agent busy");
        assert_eq!(command("PreToolUse", 0), "tmux-companion agent asked");
        assert_eq!(events["PreToolUse"][0]["matcher"], "AskUserQuestion");
        assert_eq!(command("Notification", 0), "tmux-companion agent asked");
        assert!(
            events["Notification"][0]["matcher"]
                .as_str()
                .unwrap()
                .contains("permission_prompt")
        );
        assert!(
            !events["Notification"][0]["matcher"]
                .as_str()
                .unwrap()
                .contains("idle_prompt"),
            "idle_prompt would turn every answer into a question"
        );
        assert_eq!(command("Stop", 0), "tmux-companion agent done");
        assert_eq!(
            command("SessionStart", 0),
            "tmux-companion keys claim --app claude --owner --quiet"
        );
        assert_eq!(
            command("SessionEnd", 0),
            "tmux-companion keys release --quiet"
        );
        assert!(hooks("codex").is_none());
        assert!(KNOWN.contains(&"claude"));
    }
}
