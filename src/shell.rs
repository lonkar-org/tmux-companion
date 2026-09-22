//! The OSC 133 prompt marks, and the shell code that emits them.
//!
//! tmux has had `next-prompt` and `previous-prompt` since 3.3 and they do
//! nothing until the shell says where a prompt begins. Almost nobody wires that
//! up, so a feature that ships with tmux sits unused on most machines, and the
//! whole cost of turning it on is one line in a shell rc file.
//!
//! Three marks are emitted and the fourth is not. `A` is the start of a prompt,
//! `C` is the start of a command's output and `D;<status>` is the end of one
//! with its exit code. `B`, the end of the prompt, would mean rewriting `PS1`
//! around whatever the person already has in it, and tmux's prompt navigation
//! reads `A` and `C`, so the intrusive one is the one left out.
//!
//! These are emitted whether or not tmux is the terminal, because they are
//! useful in any terminal that reads them and invisible in one that does not.
//! What tmux will not do is pass them through to the terminal outside it, which
//! is [tmux#5237](https://github.com/tmux/tmux/issues/5237) and still open, so
//! a terminal's own shell integration goes quiet inside tmux whatever this
//! prints.

/// The shells `shell-init` knows.
pub const SHELLS: [&str; 3] = ["zsh", "bash", "fish"];

/// zsh, through `add-zsh-hook`, so it stacks with whatever else is hooked.
const ZSH: &str = r#"# tmux-companion: OSC 133 prompt marks.
# Add to ~/.zshrc:  eval "$(tmux-companion shell-init zsh)"
autoload -Uz add-zsh-hook

__tmux_companion_precmd() {
  # The exit status has to be read on the first line, before anything else in
  # this function overwrites it.
  local status_=$?
  printf '\033]133;D;%s\033\\' "$status_"
  printf '\033]133;A\033\\'
}

__tmux_companion_preexec() {
  printf '\033]133;C\033\\'
}

add-zsh-hook precmd __tmux_companion_precmd
add-zsh-hook preexec __tmux_companion_preexec
"#;

/// bash, where there is no preexec and the DEBUG trap has to stand in for one.
const BASH: &str = r#"# tmux-companion: OSC 133 prompt marks.
# Add to ~/.bashrc:  eval "$(tmux-companion shell-init bash)"

__tmux_companion_precmd() {
  local status_=$?
  printf '\033]133;D;%s\033\\' "$status_"
  printf '\033]133;A\033\\'
  __tmux_companion_at_prompt=1
}

# bash has no preexec, so the DEBUG trap stands in for one. It fires for every
# command including each one inside PROMPT_COMMAND, so the flag makes it fire
# once per line the person actually typed.
__tmux_companion_preexec() {
  [[ -n ${__tmux_companion_at_prompt:-} ]] || return 0
  [[ ${BASH_COMMAND:-} == __tmux_companion_precmd* ]] && return 0
  unset __tmux_companion_at_prompt
  printf '\033]133;C\033\\'
}

case ";${PROMPT_COMMAND:-};" in
  *";__tmux_companion_precmd;"*) ;;
  *) PROMPT_COMMAND="__tmux_companion_precmd${PROMPT_COMMAND:+;$PROMPT_COMMAND}" ;;
esac
trap '__tmux_companion_preexec' DEBUG
"#;

/// fish, which has the events the other two have to be given by hand.
const FISH: &str = r#"# tmux-companion: OSC 133 prompt marks.
# Add to ~/.config/fish/config.fish:  tmux-companion shell-init fish | source

function __tmux_companion_precmd --on-event fish_prompt
    printf '\033]133;A\033\\'
end

function __tmux_companion_preexec --on-event fish_preexec
    printf '\033]133;C\033\\'
end

function __tmux_companion_postexec --on-event fish_postexec
    printf '\033]133;D;%s\033\\' $status
end
"#;

/// The shell code for a shell, or `None` for one this does not know.
pub fn init(shell: &str) -> Option<&'static str> {
    // A path is accepted because `$SHELL` is one, and somebody reaching for
    // this has that variable to hand more often than the bare name.
    let name = shell.rsplit('/').next().unwrap_or(shell);
    match name {
        "zsh" => Some(ZSH),
        "bash" => Some(BASH),
        "fish" => Some(FISH),
        _ => None,
    }
}

/// What to say when the shell is not one of the three.
pub fn unknown(shell: &str) -> String {
    format!(
        "shell-init does not know {shell}. It knows {}.\n\
         The marks are three escape sequences, so any shell with a pre-command \
         and a pre-execution hook can emit them by hand: \\033]133;A\\033\\\\ at \
         the start of a prompt, \\033]133;C\\033\\\\ before a command runs, and \
         \\033]133;D;<status>\\033\\\\ after one finishes.",
        SHELLS.join(", ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_named_shell_has_a_snippet() {
        for s in SHELLS {
            assert!(init(s).is_some(), "{s}");
        }
    }

    #[test]
    fn a_shell_path_works_as_well_as_a_name() {
        // $SHELL is a path, and that is what people have to hand.
        assert_eq!(init("/bin/zsh"), init("zsh"));
        assert_eq!(init("/opt/homebrew/bin/fish"), init("fish"));
    }

    #[test]
    fn an_unknown_shell_is_none_rather_than_a_default() {
        assert!(init("tcsh").is_none());
        assert!(init("").is_none());
    }

    #[test]
    fn every_snippet_emits_all_three_marks() {
        for s in SHELLS {
            let text = init(s).unwrap();
            assert!(text.contains(r"\033]133;A\033\\"), "{s} has no prompt mark");
            assert!(
                text.contains(r"\033]133;C\033\\"),
                "{s} has no command mark"
            );
            assert!(
                text.contains(r"\033]133;D;%s\033\\"),
                "{s} has no exit mark"
            );
        }
    }

    #[test]
    fn no_snippet_emits_the_prompt_end_mark() {
        // B would mean rewriting PS1 around whatever is already in it, and
        // tmux reads A and C. If this ever changes it should be a decision and
        // not a paste.
        for s in SHELLS {
            assert!(!init(s).unwrap().contains("133;B"), "{s}");
        }
    }

    #[test]
    fn the_exit_status_is_read_before_anything_can_overwrite_it() {
        // `local status_=$?` has to be the first statement in the function. A
        // line above it makes every command report the status of that line.
        for s in ["zsh", "bash"] {
            let text = init(s).unwrap();
            let body = text.split("__tmux_companion_precmd() {").nth(1).unwrap();
            let first = body
                .lines()
                .map(str::trim)
                .find(|l| !l.is_empty() && !l.starts_with('#'))
                .unwrap();
            assert_eq!(first, "local status_=$?", "{s}");
        }
    }

    #[test]
    fn bash_guards_the_debug_trap_so_it_fires_once_per_typed_line() {
        // Without the flag the DEBUG trap fires for every command inside
        // PROMPT_COMMAND as well, and the output mark lands in the middle of
        // the prompt.
        let text = init("bash").unwrap();
        assert!(text.contains("__tmux_companion_at_prompt"), "{text}");
        assert!(
            text.contains("trap '__tmux_companion_preexec' DEBUG"),
            "{text}"
        );
    }

    #[test]
    fn bash_does_not_stack_itself_into_prompt_command_twice() {
        // Sourcing an rc file twice is normal, and two copies would print the
        // marks twice per prompt.
        let text = init("bash").unwrap();
        assert!(
            text.contains(r#"*";__tmux_companion_precmd;"*) ;;"#),
            "{text}"
        );
    }

    #[test]
    fn zsh_uses_add_zsh_hook_so_it_stacks_with_what_is_already_there() {
        let text = init("zsh").unwrap();
        assert!(text.contains("autoload -Uz add-zsh-hook"), "{text}");
        assert!(text.contains("add-zsh-hook precmd"), "{text}");
        assert!(
            !text.lines().any(|l| l.starts_with("precmd()")),
            "it must not define a bare precmd, which would replace anything already hooked"
        );
    }

    #[test]
    fn fish_reads_status_in_postexec_where_it_is_still_the_command_s_own() {
        let text = init("fish").unwrap();
        assert!(text.contains("--on-event fish_postexec"), "{text}");
        assert!(text.contains("$status"), "{text}");
    }

    #[test]
    fn every_snippet_says_where_it_goes() {
        for s in SHELLS {
            let text = init(s).unwrap();
            assert!(text.contains("shell-init"), "{s}");
            assert!(text.lines().next().unwrap().starts_with('#'), "{s}");
        }
    }

    #[test]
    fn the_unknown_message_names_the_shells_and_the_escapes() {
        let m = unknown("tcsh");
        assert!(m.contains("tcsh"));
        for s in SHELLS {
            assert!(m.contains(s), "{m}");
        }
        assert!(m.contains("133;A"), "{m}");
    }
}
