//! Giving a pane a session of its own, named for the directory it is in.
//!
//! A pane gets opened beside the editor to look at something in another
//! repository, and an hour later it is the thing being worked on, in a
//! session named after a project it has nothing to do with. `project` would
//! open that repository properly and leave the hour's scrollback and the
//! running process behind in the old pane. This moves the pane instead.
//!
//! The session is named the way `project` names one, from the pane's
//! directory, so the promoted pane is what `project` finds the next time that
//! directory is picked and a second session for the same repository is never
//! made. When that session is already there the pane joins it as a window.
//!
//! The idea is `tmux-plugins/tmux-sessionist`'s `prefix + @`, which named the
//! session by asking. It has not been pushed since May 2023.

/// Where the pane is now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Here {
    /// `#{pane_id}`.
    pub pane: String,
    /// `#{window_id}`.
    pub window: String,
    /// `#{session_name}`.
    pub session: String,
    /// `#{pane_current_path}`.
    pub path: String,
    /// `#{window_panes}`.
    pub window_panes: u32,
    /// `#{session_windows}`.
    pub session_windows: u32,
}

impl Here {
    /// The `-F` string [`Here::parse`] reads.
    pub fn format() -> &'static str {
        "#{pane_id}\t#{window_id}\t#{session_name}\t#{pane_current_path}\t\
         #{window_panes}\t#{session_windows}"
    }

    /// What tmux answered for [`Here::format`].
    pub fn parse(text: &str) -> Option<Self> {
        let f: Vec<&str> = text.trim_end_matches('\n').split('\t').collect();
        if f.len() != 6 || !f[0].starts_with('%') {
            return None;
        }
        Some(Here {
            pane: f[0].to_string(),
            window: f[1].to_string(),
            session: f[2].to_string(),
            path: f[3].to_string(),
            window_panes: f[4].trim().parse().ok()?,
            session_windows: f[5].trim().parse().ok()?,
        })
    }

    /// The pane is the only one in its window.
    fn fills_its_window(&self) -> bool {
        self.window_panes <= 1
    }

    /// The pane is all its session holds.
    fn is_the_whole_session(&self) -> bool {
        self.fills_its_window() && self.session_windows <= 1
    }
}

/// A name tmux will take for a session: no dot and no colon.
pub fn clean(name: &str) -> String {
    name.trim()
        .chars()
        .map(|c| if c == '.' || c == ':' { '_' } else { c })
        .collect()
}

/// What promoting a pane comes to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Plan {
    /// The pane is all its session holds, so the session is renamed.
    Rename(String),
    /// A session of that name is there already, and the pane joins it.
    Join(String),
    /// A session is made and the pane moved into it.
    New(String),
    /// Nothing is done, and this is why.
    Refuse(String),
}

/// Decide, given the name asked for and whether a session has it.
pub fn plan(here: &Here, asked: Option<&str>, exists: impl Fn(&str) -> bool) -> Plan {
    let name = match asked.map(clean).filter(|n| !n.is_empty()) {
        Some(n) => n,
        None => crate::project::session_name(&here.path),
    };
    // `/` is what the directory gives at the root, and no session is named
    // with a slash in it by anything here.
    if name.is_empty() || name.contains('/') {
        return Plan::Refuse("the pane's directory gives no name; pass one".to_string());
    }
    if name == here.session {
        return Plan::Refuse(format!(
            "this pane is in {name} already; pass the name the new session should have"
        ));
    }
    if exists(&name) {
        Plan::Join(name)
    } else if here.is_the_whole_session() {
        Plan::Rename(name)
    } else {
        Plan::New(name)
    }
}

fn args(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|s| s.to_string()).collect()
}

/// The command that moves the pane into a session, as a window of its own.
///
/// A pane that fills its window is moved as that window, so the window keeps
/// its name and its options.
pub fn move_args(here: &Here, session: &str) -> Vec<String> {
    let to = format!("={session}:");
    if here.fills_its_window() {
        args(&["move-window", "-d", "-s", &here.window, "-t", &to])
    } else {
        args(&["break-pane", "-d", "-s", &here.pane, "-t", &to])
    }
}

/// The session a [`Plan::New`] starts from: one window nobody wants, whose id
/// is printed so it can be closed once the pane has arrived.
pub fn new_session_args(name: &str, path: &str) -> Vec<String> {
    args(&[
        "new-session",
        "-d",
        "-s",
        name,
        "-c",
        path,
        "-P",
        "-F",
        "#{window_id}",
    ])
}

/// Closing the window the session started with, and numbering what is left
/// from the base index, so the promoted pane is window one and not two.
pub fn tidy_args(placeholder: &str, session: &str) -> Vec<Vec<String>> {
    vec![
        args(&["kill-window", "-t", placeholder]),
        args(&["move-window", "-r", "-t", &format!("={session}:")]),
    ]
}

async fn tmux(args: &[String]) {
    let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
    crate::cli::tmux(&borrowed).await;
}

/// `promote`: move a pane into a session of its own.
pub async fn run(name: Option<String>, pane: Option<String>) -> anyhow::Result<()> {
    let pane = pane
        .filter(|p| !p.trim().is_empty())
        .or_else(crate::cli::pane_target);
    let Some(here) =
        Here::parse(&crate::cli::tmux_display_at(pane.as_deref(), Here::format()).await)
    else {
        anyhow::bail!("not inside tmux, or the pane is gone");
    };

    // Asked one name at a time because tmux is: the candidates are the name
    // given or the one the directory gives, and only that one is looked up.
    let wanted = match name.as_deref().map(clean).filter(|n| !n.is_empty()) {
        Some(n) => n,
        None => crate::project::session_name(&here.path),
    };
    let there = crate::cli::session_exists(&format!("={wanted}")).await;

    match plan(&here, name.as_deref(), |_| there) {
        Plan::Refuse(why) => {
            println!("{why}");
        }
        Plan::Rename(to) => {
            tmux(&args(&[
                "rename-session",
                "-t",
                &format!("={}", here.session),
                &to,
            ]))
            .await;
            println!("{} is now {to}", here.session);
        }
        Plan::Join(session) => {
            // The client goes first when the pane is all its session holds:
            // the move closes that session, and with `detach-on-destroy` on,
            // which is tmux's default, a client still in it is detached.
            if here.is_the_whole_session() {
                crate::cli::focus_session(&session).await?;
            }
            tmux(&move_args(&here, &session)).await;
            crate::panes::jump(&here.pane).await;
            println!("{} joined {session}", here.pane);
        }
        Plan::New(session) => {
            let made = new_session_args(&session, &here.path);
            let borrowed: Vec<&str> = made.iter().map(String::as_str).collect();
            let placeholder = crate::cli::tmux_capture(&borrowed).await.trim().to_string();
            if !placeholder.starts_with('@') {
                anyhow::bail!("tmux would not make a session called {session}");
            }
            tmux(&move_args(&here, &session)).await;
            for cmd in tidy_args(&placeholder, &session) {
                tmux(&cmd).await;
            }
            let config = crate::cli::config_or_default();
            if config.journal.enabled {
                let home = std::env::var("HOME").unwrap_or_default();
                crate::journal::append(&crate::journal::Event {
                    at: crate::panes::now_secs(),
                    session: session.clone(),
                    path: crate::project::short_path(&here.path, &home),
                    kind: crate::journal::Kind::Opened,
                    detail: format!("promoted from {}", here.session),
                });
            }
            crate::panes::jump(&here.pane).await;
            println!("{} is now session {session}", here.pane);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn here(window_panes: u32, session_windows: u32) -> Here {
        Here {
            pane: "%7".into(),
            window: "@3".into(),
            session: "api".into(),
            path: "/home/me/w/web.site".into(),
            window_panes,
            session_windows,
        }
    }

    #[test]
    fn the_format_and_the_parser_agree() {
        assert_eq!(Here::format().split('\t').count(), 6);
        let got = Here::parse("%7\t@3\tapi\t/home/me/w/web.site\t2\t4\n").unwrap();
        assert_eq!(got, here(2, 4));
        assert_eq!(Here::parse(""), None);
        assert_eq!(Here::parse("api\t@3\tapi\t/w\t2\t4"), None, "no pane id");
        assert_eq!(Here::parse("%7\t@3\tapi\t/w\ttwo\t4"), None);
    }

    #[test]
    fn the_session_is_named_for_the_directory_the_way_project_names_one() {
        assert_eq!(
            plan(&here(2, 4), None, |_| false),
            Plan::New("web_site".into())
        );
    }

    #[test]
    fn a_name_that_was_asked_for_wins_and_is_cleaned() {
        assert_eq!(
            plan(&here(2, 4), Some(" notes.v2 "), |_| false),
            Plan::New("notes_v2".into())
        );
        // Nothing but spaces is no name, and the directory decides.
        assert_eq!(
            plan(&here(2, 4), Some("  "), |_| false),
            Plan::New("web_site".into())
        );
    }

    #[test]
    fn a_session_that_is_there_already_is_joined() {
        assert_eq!(
            plan(&here(2, 4), None, |n| n == "web_site"),
            Plan::Join("web_site".into())
        );
        // Also when the pane is all its session holds: two sessions become one.
        assert_eq!(
            plan(&here(1, 1), None, |n| n == "web_site"),
            Plan::Join("web_site".into())
        );
    }

    #[test]
    fn a_pane_that_is_its_whole_session_is_a_rename() {
        assert_eq!(
            plan(&here(1, 1), None, |_| false),
            Plan::Rename("web_site".into())
        );
        // One pane in its window, but the session has other windows.
        assert_eq!(
            plan(&here(1, 3), None, |_| false),
            Plan::New("web_site".into())
        );
    }

    #[test]
    fn a_pane_already_in_that_session_is_refused_with_what_to_do() {
        let mut h = here(2, 4);
        h.session = "web_site".into();
        let Plan::Refuse(why) = plan(&h, None, |_| true) else {
            panic!("not refused");
        };
        assert!(why.contains("pass the name"), "{why}");
        // A directory with no name in it, which is `/`.
        h.path = "/".into();
        assert!(matches!(plan(&h, None, |_| false), Plan::Refuse(_)));
    }

    #[test]
    fn a_pane_is_broken_off_and_a_window_is_moved_whole() {
        assert_eq!(
            move_args(&here(2, 4), "web_site").join(" "),
            "break-pane -d -s %7 -t =web_site:"
        );
        assert_eq!(
            move_args(&here(1, 4), "web_site").join(" "),
            "move-window -d -s @3 -t =web_site:"
        );
    }

    #[test]
    fn a_new_session_prints_its_first_window_so_it_can_be_closed() {
        assert_eq!(
            new_session_args("web_site", "/home/me/w/web.site").join(" "),
            "new-session -d -s web_site -c /home/me/w/web.site -P -F #{window_id}"
        );
        let tidy: Vec<String> = tidy_args("@9", "web_site")
            .iter()
            .map(|c| c.join(" "))
            .collect();
        assert_eq!(
            tidy,
            vec!["kill-window -t @9", "move-window -r -t =web_site:"]
        );
    }
}
