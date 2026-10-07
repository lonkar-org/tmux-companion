//! Who is listening on which port, and which pane started it.
//!
//! `address already in use` is the message, and the question behind it is
//! which of nine panes still has last week's dev server running in it. With
//! agents in those panes it gets asked more often, because an agent starts a
//! server to try something and doesn't always stop it. `jrmoulton/tmux-port`
//! listed ports from inside tmux and its repository is gone; nothing living
//! was found to send people to.
//!
//! The list comes from `lsof`, or from `ss` on a machine that has no `lsof`,
//! which between them cover macOS and every Linux. Both are asked for
//! listening TCP sockets, and with `--udp` for UDP sockets that are bound
//! and not connected, which is as near as UDP comes to listening. UDP is
//! asked for rather than given, because every program that resolves a name
//! holds one for a moment and a list of those is noise. What neither tool
//! knows is tmux, so each listener's
//! process is walked up through its parents until one of them is a pane's own
//! process, and that pane is the row's.
//!
//! This runs in the **client** and talks to tmux directly, as `panes` does.

use std::collections::{BTreeSet, HashMap};

/// Which of the two a socket is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Protocol {
    /// A socket in `LISTEN`.
    Tcp,
    /// A socket that is bound and connected to nobody.
    Udp,
}

impl Protocol {
    /// The word in the column.
    pub fn word(self) -> &'static str {
        match self {
            Protocol::Tcp => "tcp",
            Protocol::Udp => "udp",
        }
    }

    /// The protocol a tool's own name for it means, in either case.
    fn named(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "tcp" => Some(Protocol::Tcp),
            "udp" => Some(Protocol::Udp),
            _ => None,
        }
    }
}

/// One listening socket as the system reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listener {
    /// TCP or UDP.
    pub protocol: Protocol,
    /// The process holding it.
    pub pid: u32,
    /// The address it is bound to, as [`address`] words it.
    pub address: String,
    /// The port.
    pub port: u16,
}

/// An address as the list shows it.
///
/// Every spelling of "any interface" is `*` and both loopbacks are
/// `localhost`, because the question the column answers is whether the rest
/// of the network can reach the port, and `0.0.0.0` against `[::]` is not
/// that question.
pub fn address(raw: &str) -> String {
    // `127.0.0.53%lo` is an address on an interface, as `ss` writes it.
    let raw = raw.split('%').next().unwrap_or(raw);
    match raw.trim().trim_start_matches('[').trim_end_matches(']') {
        "*" | "0.0.0.0" | "::" | "" => "*".to_string(),
        "127.0.0.1" | "::1" => "localhost".to_string(),
        other => other.to_string(),
    }
}

/// `address:port`, split at the last colon so an IPv6 address keeps its own.
fn endpoint(text: &str) -> Option<(String, u16)> {
    let (host, port) = text.trim().rsplit_once(':')?;
    Some((address(host), port.trim().parse().ok()?))
}

/// What `lsof -nP -iTCP -sTCP:LISTEN -FpPn` prints, with `-iUDP` or without.
///
/// One field per line, the first byte naming it: `p` starts a process, `P`
/// is the protocol of the socket that follows and `n` is the socket. Anything
/// else is a field nobody asked for, which some versions print anyway. A
/// socket with no `P` before it is TCP, which is all that was asked for
/// before there was a `P` to read.
///
/// `-sTCP:LISTEN` does not reach UDP, so every UDP socket comes back, and the
/// connected ones are told apart by the arrow in their name:
/// `192.168.1.2:49961->17.248.195.69:443` is a conversation, not a port
/// somebody can send to.
pub fn parse_lsof(text: &str) -> Vec<Listener> {
    let mut pid = None;
    let mut protocol = Protocol::Tcp;
    let mut out = Vec::new();
    for line in text.lines() {
        let Some(kind) = line.chars().next() else {
            continue;
        };
        let rest = &line[kind.len_utf8()..];
        match kind {
            'p' => {
                pid = rest.trim().parse().ok();
                protocol = Protocol::Tcp;
            }
            'P' => protocol = Protocol::named(rest).unwrap_or(protocol),
            'n' if !rest.contains("->") => {
                if let (Some(pid), Some((address, port))) = (pid, endpoint(rest)) {
                    out.push(Listener {
                        protocol,
                        pid,
                        address,
                        port,
                    });
                }
            }
            _ => {}
        }
    }
    out
}

/// What `ss -Hltnp` prints, or `ss -Hltunp`.
///
/// A row that starts with `tcp` or `udp` has the protocol in front of the
/// state and one that doesn't is taken for TCP, so the first column says
/// which shape the row has and nothing here depends on which one `ss` chose.
/// The local endpoint is the fourth column after the state. I have no Linux
/// to run `ss` on from this laptop, so both shapes are read from its manual
/// and the e2e test on the runner is what holds them to the real thing. The owners are in the last, as
/// `users:(("node",pid=812,fd=23),("node",pid=813,fd=23))`, and a row
/// without one is a socket that belongs to somebody else, which `ss` will
/// not name for anybody but root.
pub fn parse_ss(text: &str) -> Vec<Listener> {
    let mut out = Vec::new();
    for line in text.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        let named = fields.first().and_then(|f| Protocol::named(f));
        let local = if named.is_some() { 4 } else { 3 };
        let protocol = named.unwrap_or(Protocol::Tcp);
        let Some((address, port)) = fields.get(local).and_then(|f| endpoint(f)) else {
            continue;
        };
        for part in line.split("pid=").skip(1) {
            let digits: String = part.chars().take_while(char::is_ascii_digit).collect();
            if let Ok(pid) = digits.parse() {
                out.push(Listener {
                    protocol,
                    pid,
                    address: address.clone(),
                    port,
                });
            }
        }
    }
    out
}

/// One process, as far as the walk needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Process {
    /// Its parent, when it has one.
    pub parent: Option<u32>,
    /// Its name.
    pub name: String,
}

/// Every process on the machine, by pid.
pub type Table = HashMap<u32, Process>;

/// The pane a process runs under: the first of its ancestors, itself
/// included, that is a pane's own process.
///
/// The walk stops after sixty-four steps. A process table is read while it
/// changes, and a pid reused between two reads can make a parent chain that
/// goes round.
pub fn pane_of<'a>(pid: u32, table: &Table, panes: &'a HashMap<u32, String>) -> Option<&'a str> {
    let mut at = pid;
    for _ in 0..64 {
        if let Some(id) = panes.get(&at) {
            return Some(id);
        }
        at = table.get(&at)?.parent?;
    }
    None
}

/// `#{pane_pid}` to `#{pane_id}`, from `list-panes -a -F` of [`pid_format`].
pub fn parse_pane_pids(text: &str) -> HashMap<u32, String> {
    text.lines()
        .filter_map(|l| {
            let (id, pid) = l.split_once('\t')?;
            Some((pid.trim().parse().ok()?, id.trim().to_string()))
        })
        .collect()
}

/// The `-F` string [`parse_pane_pids`] reads.
pub fn pid_format() -> &'static str {
    "#{pane_id}\t#{pane_pid}"
}

/// One row of the picker, or one line of `--print`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// TCP or UDP.
    pub protocol: Protocol,
    /// The port.
    pub port: u16,
    /// Where it can be reached from: `*`, `localhost`, or an address.
    pub address: String,
    /// The name of the process listening.
    pub program: String,
    /// Its pid.
    pub pid: u32,
    /// `session:window.pane`, when a pane started it.
    pub at: Option<String>,
    /// The pane's `%N`.
    pub id: Option<String>,
    /// The pane's directory, shortened.
    pub cwd: String,
}

/// Which rows the list holds.
#[derive(Debug, Clone, Copy, Default)]
pub struct Filter<'a> {
    /// The ports no pane started as well.
    pub all: bool,
    /// Only this session's panes.
    pub session: Option<&'a str>,
}

/// The rows, the ones a pane started first and each half by port.
///
/// A process listening on one port over IPv4 and IPv6 is two sockets and one
/// row. When one of them is every interface the row says `*`, since that is
/// the wider answer and the one worth knowing.
pub fn rows(
    listeners: &[Listener],
    table: &Table,
    pane_pids: &HashMap<u32, String>,
    panes: &[crate::panes::Pane],
    filter: &Filter,
    home: &str,
) -> Vec<Row> {
    let mut merged: HashMap<(u32, u16, Protocol), BTreeSet<&str>> = HashMap::new();
    for l in listeners {
        merged
            .entry((l.pid, l.port, l.protocol))
            .or_default()
            .insert(l.address.as_str());
    }
    let mut rows: Vec<Row> = merged
        .into_iter()
        .filter_map(|((pid, port, protocol), addresses)| {
            let pane =
                pane_of(pid, table, pane_pids).and_then(|id| panes.iter().find(|p| p.id == id));
            match pane {
                Some(p) if filter.session.is_some_and(|s| s != p.session) => return None,
                None if !filter.all || filter.session.is_some() => return None,
                _ => {}
            }
            Some(Row {
                protocol,
                port,
                address: if addresses.contains("*") {
                    "*".to_string()
                } else {
                    addresses.into_iter().collect::<Vec<_>>().join(",")
                },
                program: table
                    .get(&pid)
                    .map(|p| p.name.clone())
                    .filter(|n| !n.is_empty())
                    .unwrap_or_else(|| "?".to_string()),
                pid,
                at: pane.map(|p| format!("{}:{}.{}", p.session, p.window_index, p.pane_index)),
                id: pane.map(|p| p.id.clone()),
                cwd: pane
                    .map(|p| crate::project::short_path(&p.path, home))
                    .unwrap_or_default(),
            })
        })
        .collect();
    rows.sort_by(|a, b| {
        a.at.is_none()
            .cmp(&b.at.is_none())
            .then(a.port.cmp(&b.port))
            .then(a.protocol.cmp(&b.protocol))
            .then(a.pid.cmp(&b.pid))
    });
    rows
}

/// The listening sockets, from whichever of the two tools is installed.
///
/// `lsof` exits 1 when it has nothing to list, so its status says nothing and
/// what it printed is read either way. Only a tool that could not be started
/// is a reason to try the next one.
async fn listeners(udp: bool) -> anyhow::Result<Vec<Listener>> {
    let mut asked = vec!["-nP", "-iTCP", "-sTCP:LISTEN"];
    if udp {
        asked.push("-iUDP");
    }
    asked.push("-FpPn");
    let lsof = tokio::process::Command::new("lsof")
        .args(&asked)
        .output()
        .await;
    if let Ok(out) = lsof {
        return Ok(parse_lsof(&String::from_utf8_lossy(&out.stdout)));
    }
    let ss = tokio::process::Command::new("ss")
        .args([if udp { "-Hltunp" } else { "-Hltnp" }])
        .output()
        .await;
    match ss {
        Ok(out) => Ok(parse_ss(&String::from_utf8_lossy(&out.stdout))),
        Err(_) => anyhow::bail!("neither lsof nor ss is on PATH, and one of them has to be"),
    }
}

/// The process table, read on a worker thread for the reason `sh-jobs` reads
/// it there.
pub async fn table() -> Table {
    tokio::task::spawn_blocking(|| {
        use sysinfo::{ProcessesToUpdate, System};
        let mut sys = System::new();
        sys.refresh_processes(ProcessesToUpdate::All, true);
        sys.processes()
            .iter()
            .map(|(pid, p)| {
                (
                    pid.as_u32(),
                    Process {
                        parent: p.parent().map(|p| p.as_u32()),
                        name: p.name().to_string_lossy().into_owned(),
                    },
                )
            })
            .collect()
    })
    .await
    .unwrap_or_default()
}

/// `ports`: the listening ports, and the pane behind the one picked.
///
/// With `stop` the pick is stopped where it would have been visited, after
/// that long a grace. Only the process holding the socket is signalled and
/// not its group: the one listening is the one in the way, and what started
/// it is often the shell of the pane.
pub async fn run(
    all: bool,
    udp: bool,
    print: bool,
    target: Option<String>,
    stop: Option<std::time::Duration>,
) -> anyhow::Result<()> {
    let config = crate::cli::config_or_default();
    let home = std::env::var("HOME").unwrap_or_default();
    let listing = ["list-panes", "-a", "-F", pid_format()];
    let (listeners, table, panes, pane_pids) = tokio::join!(
        listeners(udp),
        table(),
        crate::panes::list(),
        crate::cli::tmux_capture(&listing)
    );
    let filter = Filter {
        all,
        session: target.as_deref(),
    };
    let rows = rows(
        &listeners?,
        &table,
        &parse_pane_pids(&pane_pids),
        &panes,
        &filter,
        &home,
    );

    if rows.is_empty() {
        crate::picker::say_nothing_to_show(
            if all {
                "nothing is listening"
            } else {
                "no pane is listening on a port; --all lists the ones no pane started"
            },
            print,
        )
        .await;
        return Ok(());
    }
    if print {
        for r in &rows {
            println!(
                "{}\t{}\t{}\t{}\t{}\t{}\t{}",
                r.port,
                r.protocol.word(),
                r.address,
                r.program,
                r.pid,
                r.at.as_deref().unwrap_or("-"),
                r.id.as_deref().unwrap_or("-")
            );
        }
        return Ok(());
    }

    let mut items = Vec::with_capacity(rows.len());
    for r in &rows {
        let preview = match &r.id {
            Some(id) => crate::panes::tail_of(id).await,
            None => String::new(),
        };
        items.push(
            crate::picker::Item::with_preview(
                format!(
                    "{} {} {} {} {} {}",
                    r.port,
                    r.protocol.word(),
                    r.address,
                    r.program,
                    r.at.as_deref().unwrap_or("-"),
                    r.cwd
                ),
                preview,
            )
            // The port is what you came looking for, so it is bold. The pane
            // is where enter goes, and a port with no pane behind it has
            // nowhere to go, so its `-` is grey.
            .in_cells(vec![
                crate::picker::Cell::strong(format!("{}/{}", r.port, r.protocol.word())),
                crate::picker::Cell::dim(r.address.clone()),
                crate::picker::Cell::plain(r.program.clone()),
                crate::picker::Cell::dim(r.pid.to_string()),
                match &r.at {
                    Some(at) => crate::picker::Cell::plain(at.clone()),
                    None => crate::picker::Cell::dim("-"),
                },
                crate::picker::Cell::dim(r.cwd.clone()),
            ])
            .with_icon(crate::tmux::icons::PORT, crate::picker::Tone::Dim),
        );
    }

    let chrome = crate::picker::Chrome {
        title: if stop.is_some() {
            "[ Stop a port ]"
        } else {
            "[ Ports ]"
        }
        .into(),
        footer: if stop.is_some() {
            "enter stop it   ctrl-a clear   esc cancel"
        } else {
            "enter jump   ctrl-a clear   esc cancel"
        }
        .into(),
        preview_title: "[ Screen ]".into(),
        icon: crate::tmux::icons::PORT.into(),
        ..Default::default()
    }
    .configured(&config.picker, crate::config::Picker::Panes);

    if let Some(index) = crate::picker::run(items, "", &chrome)?
        && let Some(row) = rows.get(index)
    {
        match (stop, &row.id) {
            (Some(grace), id) => {
                let name = format!(
                    "{} on {} port {}",
                    row.program,
                    row.protocol.word(),
                    row.port
                );
                crate::kill::stop_process(row.pid, &name, grace, id.as_deref()).await;
            }
            (None, Some(id)) => crate::panes::jump(id).await,
            (None, None) => eprintln!("port {} was started by no pane", row.port),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listener(pid: u32, address: &str, port: u16) -> Listener {
        Listener {
            protocol: Protocol::Tcp,
            pid,
            address: address.to_string(),
            port,
        }
    }

    #[test]
    fn every_spelling_of_anywhere_is_a_star_and_both_loopbacks_are_localhost() {
        for any in ["*", "0.0.0.0", "[::]", "::"] {
            assert_eq!(address(any), "*", "{any}");
        }
        for local in ["127.0.0.1", "[::1]"] {
            assert_eq!(address(local), "localhost", "{local}");
        }
        assert_eq!(address("192.168.1.20"), "192.168.1.20");
        assert_eq!(address("[fe80::1]"), "fe80::1");
        assert_eq!(address("127.0.0.53%lo"), "127.0.0.53");
    }

    #[test]
    fn lsof_fields_become_one_listener_per_socket() {
        // As printed on macOS 15, with the `f` lines nobody asked for.
        let text = "p498\nf9\nn*:7000\nf10\nn*:7000\nf11\nn*:5000\n\
                    p6867\nf26\nn[::1]:4321\nf27\nn127.0.0.1:4321\n";
        assert_eq!(
            parse_lsof(text),
            vec![
                listener(498, "*", 7000),
                listener(498, "*", 7000),
                listener(498, "*", 5000),
                listener(6867, "localhost", 4321),
                listener(6867, "localhost", 4321),
            ]
        );
        assert!(parse_lsof("").is_empty());
        assert!(parse_lsof("n*:80\n").is_empty(), "a socket with no owner");
    }

    fn udp(pid: u32, address: &str, port: u16) -> Listener {
        Listener {
            protocol: Protocol::Udp,
            ..listener(pid, address, port)
        }
    }

    #[test]
    fn lsof_names_the_protocol_and_a_connected_udp_socket_is_no_port() {
        // As printed on macOS 15 by `-iTCP -sTCP:LISTEN -iUDP -FpPn`.
        let text = "p465\nf13\nPTCP\nn*:50069\nf22\nPUDP\nn*:3722\n\
                    p466\nf11\nPUDP\nn192.168.68.50:49961->17.248.195.69:443\n\
                    f12\nPUDP\nn127.0.0.1:5353\n\
                    p467\nf3\nn*:80\n";
        assert_eq!(
            parse_lsof(text),
            vec![
                listener(465, "*", 50069),
                udp(465, "*", 3722),
                udp(466, "localhost", 5353),
                // No `P` line, and the one before it belonged to 466.
                listener(467, "*", 80),
            ]
        );
    }

    #[test]
    fn ss_asked_for_both_says_which_in_front() {
        let text = "udp UNCONN 0 0 127.0.0.53%lo:53 0.0.0.0:* users:((\"resolved\",pid=560,fd=13))\n\
                    tcp LISTEN 0 511 127.0.0.1:3000 0.0.0.0:* users:((\"node\",pid=812,fd=23))\n";
        assert_eq!(
            parse_ss(text),
            vec![udp(560, "127.0.0.53", 53), listener(812, "localhost", 3000)]
        );
    }

    #[test]
    fn one_port_over_both_protocols_is_two_rows() {
        let listeners = [listener(400, "*", 4433), udp(400, "*", 4433)];
        let got = rows(
            &listeners,
            &table(),
            &pane_pids(),
            &panes(),
            &Filter::default(),
            "/home/me",
        );
        let shown: Vec<(u16, &str)> = got.iter().map(|r| (r.port, r.protocol.word())).collect();
        assert_eq!(shown, vec![(4433, "tcp"), (4433, "udp")]);
    }

    #[test]
    fn ss_rows_become_one_listener_per_owner() {
        let text = "LISTEN 0 511 127.0.0.1:3000 0.0.0.0:* users:((\"node\",pid=812,fd=23))\n\
                    LISTEN 0 128 [::]:8080 [::]:* users:((\"nginx\",pid=90,fd=6),(\"nginx\",pid=91,fd=6))\n\
                    LISTEN 0 128 0.0.0.0:22 0.0.0.0:*\n";
        assert_eq!(
            parse_ss(text),
            vec![
                listener(812, "localhost", 3000),
                listener(90, "*", 8080),
                listener(91, "*", 8080),
            ]
        );
    }

    fn table() -> Table {
        // 1 → 100 (tmux) → 200 (zsh, a pane) → 300 (npm) → 400 (node)
        //              └─→ 210 (zsh, a pane)
        // 1 → 900 (a daemon no pane started)
        [
            (1, None, "launchd"),
            (100, Some(1), "tmux"),
            (200, Some(100), "zsh"),
            (210, Some(100), "zsh"),
            (300, Some(200), "npm"),
            (400, Some(300), "node"),
            (900, Some(1), "postgres"),
        ]
        .into_iter()
        .map(|(pid, parent, name)| {
            (
                pid,
                Process {
                    parent,
                    name: name.to_string(),
                },
            )
        })
        .collect()
    }

    fn pane_pids() -> HashMap<u32, String> {
        parse_pane_pids("%1\t200\n%2\t210\nnonsense\n")
    }

    fn panes() -> Vec<crate::panes::Pane> {
        crate::panes::parse(
            "api\t1\tedit\t0\t%1\tnode\t/home/me/w/api\tlaptop\t1\t1\t1\t0\t0\tlaptop\t0\n\
             web\t2\trun\t1\t%2\tzsh\t/home/me/w/web\tlaptop\t1\t1\t1\t0\t0\tlaptop\t0\n",
        )
    }

    #[test]
    fn a_process_belongs_to_the_pane_above_it() {
        let (t, p) = (table(), pane_pids());
        assert_eq!(pane_of(400, &t, &p), Some("%1"), "through npm to the shell");
        assert_eq!(pane_of(200, &t, &p), Some("%1"), "the pane's own process");
        assert_eq!(pane_of(900, &t, &p), None);
        assert_eq!(pane_of(12345, &t, &p), None, "a pid that has gone");
    }

    #[test]
    fn a_parent_chain_that_goes_round_ends() {
        let mut t = table();
        t.insert(
            1,
            Process {
                parent: Some(900),
                name: "launchd".into(),
            },
        );
        assert_eq!(pane_of(900, &t, &pane_pids()), None);
    }

    #[test]
    fn rows_are_the_panes_ports_by_port_and_two_sockets_are_one_row() {
        let listeners = [
            listener(400, "localhost", 5173),
            listener(400, "*", 5173),
            listener(400, "localhost", 3000),
            listener(900, "localhost", 5432),
        ];
        let got = rows(
            &listeners,
            &table(),
            &pane_pids(),
            &panes(),
            &Filter::default(),
            "/home/me",
        );
        assert_eq!(got.len(), 2, "{got:?}");
        assert_eq!(got[0].port, 3000);
        assert_eq!(got[0].address, "localhost");
        assert_eq!(got[0].program, "node");
        assert_eq!(got[0].pid, 400);
        assert_eq!(got[0].at.as_deref(), Some("api:1.0"));
        assert_eq!(got[0].id.as_deref(), Some("%1"));
        assert_eq!(got[0].cwd, "~/w/api");
        assert_eq!(got[1].port, 5173);
        assert_eq!(got[1].address, "*", "every interface is the wider answer");
    }

    #[test]
    fn all_adds_the_ports_no_pane_started_after_the_ones_one_did() {
        let listeners = [
            listener(900, "localhost", 80),
            listener(400, "localhost", 3000),
        ];
        let all = Filter {
            all: true,
            session: None,
        };
        let got = rows(
            &listeners,
            &table(),
            &pane_pids(),
            &panes(),
            &all,
            "/home/me",
        );
        let ports: Vec<u16> = got.iter().map(|r| r.port).collect();
        assert_eq!(ports, vec![3000, 80]);
        assert_eq!(got[1].at, None);
        assert_eq!(got[1].program, "postgres");
    }

    #[test]
    fn a_session_keeps_its_own_panes_ports_and_nothing_else() {
        let listeners = [
            listener(400, "localhost", 3000),
            listener(210, "localhost", 4000),
            listener(900, "localhost", 80),
        ];
        let web = Filter {
            all: true,
            session: Some("web"),
        };
        let got = rows(
            &listeners,
            &table(),
            &pane_pids(),
            &panes(),
            &web,
            "/home/me",
        );
        let ports: Vec<u16> = got.iter().map(|r| r.port).collect();
        assert_eq!(ports, vec![4000]);
    }
}
