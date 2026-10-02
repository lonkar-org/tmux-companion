//! What each layer binds, found without asking any app to do anything new.
//!
//! Every app gets an adapter that tries three rungs, best first, and each row
//! records which rung answered. Live asks a running instance through an
//! interface it already has; config reads the app's own configuration, or
//! starts it headless with that configuration; declared is
//! `[keys.app.<name>] claims` from the registry, added on top of the other two.
//!
//! Only a fixed list of read-only calls is sent to a live instance, never an
//! expression from config or the command line, and this runs in the client on
//! demand. The daemon never connects to an editor or starts one.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::spell;

/// Which rung of an adapter a row came from.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum Rung {
    /// Asked of a running instance.
    Live,
    /// Read from the app's configuration, or from a headless start with it.
    Config,
    /// Written in the registry by hand.
    Declared,
}

impl Rung {
    /// The word the report shows.
    pub fn as_str(self) -> &'static str {
        match self {
            Rung::Live => "live",
            Rung::Config => "config",
            Rung::Declared => "declared",
        }
    }
}

/// One binding in one layer.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// `tmux`, or the app's name as `[keys.app.<name>]` would spell it.
    pub layer: String,
    /// The tmux table, or the app's mode (`n`, `i`, `x`, ...). Empty when the
    /// claim holds in every mode.
    pub mode: String,
    /// The key in tmux's spelling.
    pub key: String,
    /// What the binding says it does: a note, a desc. Empty when it says nothing.
    pub desc: String,
    /// What it runs: the tmux command, the rhs. Empty when unknown.
    pub source: String,
    /// Which rung found it.
    pub rung: Rung,
    /// The pane a live answer came from, `%7`. Empty for every other rung.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub pane: String,
}

/// What `keys discover` writes: the rows, and a line per layer that could not
/// be read, so a missing layer is said rather than looking empty.
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq, Eq)]
pub struct Discovered {
    /// Seconds since the epoch when it was taken.
    pub taken_at: u64,
    /// Every row, sorted by key then layer.
    pub rows: Vec<Row>,
    /// Why a layer has fewer rows than it might, one line each.
    pub notes: Vec<String>,
}

/// The file `keys discover` writes and `keys collide` reads.
pub const FILE: &str = "keys-discovered.json";

/// Where that file lives.
pub fn path() -> Option<PathBuf> {
    crate::server::state_dir().map(|d| d.join(FILE))
}

/// Read the last discovery, `None` when there has not been one.
pub fn load(path: &Path) -> Option<Discovered> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

/// The layers `--layer` accepts.
pub const LAYERS: [&str; 7] = ["tmux", "nvim", "vim", "claude", "fzf", "nano", "zsh"];

/// Layers that are reported and never routed on. zsh is the command in every
/// idle pane and binds most of `C-*`, so counting it would hold every key it
/// shares at the prompt; fzf-lua lives inside nvim, where the publisher
/// speaks for it.
pub const REPORT_ONLY: [&str; 2] = ["zsh", "fzf-lua"];

/// Run every adapter, or only `only`, and add the declared claims.
pub async fn discover(
    only: Option<&str>,
    apps: &BTreeMap<String, crate::config::KeyApp>,
) -> Discovered {
    let mut rows = Vec::new();
    let mut notes = Vec::new();
    let wants = |layer: &str| only.is_none_or(|o| o == layer);

    if wants("tmux") {
        match crate::keys::collect().await {
            Ok(keys) => rows.extend(tmux_rows(&keys)),
            Err(e) => notes.push(format!("tmux: {e}")),
        }
    }
    if wants("nvim") {
        let (found, note) = nvim().await;
        rows.extend(found);
        notes.extend(note);
    }
    if wants("vim") {
        let (found, note) = vim().await;
        rows.extend(found);
        notes.extend(note);
    }
    if wants("claude") && on_path("claude") {
        let dir = std::env::var_os("CLAUDE_CONFIG_DIR")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".claude")));
        let file = dir
            .map(|d| d.join("keybindings.json"))
            .and_then(|p| std::fs::read_to_string(p).ok());
        match super::apps::claude_rows(file.as_deref()) {
            Ok(r) => rows.extend(r),
            Err(e) => notes.push(format!("claude: {e}")),
        }
    }
    if wants("fzf") && on_path("fzf") {
        let mut opts = std::env::var("FZF_DEFAULT_OPTS").unwrap_or_default();
        if let Some(file) = std::env::var_os("FZF_DEFAULT_OPTS_FILE")
            && let Ok(text) = std::fs::read_to_string(file)
        {
            opts = format!("{text} {opts}");
        }
        rows.extend(super::apps::fzf_rows(&opts));
    }
    if wants("nano") && on_path("nano") {
        let xdg = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")));
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let text: String = [
            home.map(|h| h.join(".nanorc")),
            xdg.map(|x| x.join("nano/nanorc")),
        ]
        .into_iter()
        .flatten()
        .filter_map(|p| std::fs::read_to_string(p).ok())
        .collect::<Vec<_>>()
        .join("\n");
        rows.extend(super::apps::nano_rows(&text));
    }
    if wants("zsh") && on_path("zsh") {
        let mut cmd = tokio::process::Command::new("zsh");
        cmd.args([
            "-ic",
            "bindkey -L -M emacs; bindkey -L -M viins; bindkey -L -M vicmd",
        ]);
        match run(cmd, SHELL_TIMEOUT).await {
            Ok(out) => rows.extend(super::apps::zsh_rows(&out, "emacs")),
            Err(e) => notes.push(format!("zsh: {e}")),
        }
    }
    rows.extend(
        declared_rows(apps)
            .into_iter()
            .filter(|r| only.is_none_or(|o| o == r.layer)),
    );

    sort_and_dedupe(&mut rows);
    Discovered {
        taken_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs()),
        rows,
        notes,
    }
}

/// Sort by key, layer, mode, pane, and drop exact repeats: an nvo map shows up
/// once per mode it is asked about and once is enough per mode.
fn sort_and_dedupe(rows: &mut Vec<Row>) {
    rows.sort_by(|a, b| {
        (&a.key, &a.layer, &a.mode, &a.pane, &a.desc)
            .cmp(&(&b.key, &b.layer, &b.mode, &b.pane, &b.desc))
    });
    rows.dedup_by(|a, b| {
        a.key == b.key && a.layer == b.layer && a.mode == b.mode && a.pane == b.pane
    });
}

/// The root table's bindings in scope. Only root matters: a prefix binding
/// never stands between a key and the pane.
pub fn tmux_rows(keys: &[crate::keys::KeyRow]) -> Vec<Row> {
    keys.iter()
        .filter(|k| k.table == "root" && spell::in_scope(&k.key))
        .map(|k| Row {
            layer: "tmux".into(),
            mode: "root".into(),
            key: k.key.clone(),
            desc: crate::keys::with_current_prefix(&k.note),
            source: k.command.clone(),
            rung: Rung::Live,
            pane: String::new(),
        })
        .collect()
}

/// `[keys.app.<name>] claims`, one row per key, every mode.
pub fn declared_rows(apps: &BTreeMap<String, crate::config::KeyApp>) -> Vec<Row> {
    apps.iter()
        .flat_map(|(name, app)| {
            app.claims.iter().map(move |key| Row {
                layer: name.clone(),
                mode: String::new(),
                key: key.clone(),
                desc: String::new(),
                source: String::new(),
                rung: Rung::Declared,
                pane: String::new(),
            })
        })
        .collect()
}

// nvim -------------------------------------------------------------------

/// The modes nvim is asked about. `v` is `x` plus `s`, so it is left out.
pub const NVIM_MODES: [&str; 7] = ["n", "i", "x", "s", "o", "c", "t"];

/// The one Lua chunk ever sent to an nvim: every global and current-buffer map
/// per mode, as `[mode, lhs, desc, rhs]`, and fzf-lua's fzf binds as
/// `['fzf-lua', key, action, '']` when fzf-lua is already loaded. Read-only:
/// `package.loaded`, never `require`, so asking doesn't load a plugin.
/// Single quotes only, so it fits inside `luaeval("...")` without escaping.
const NVIM_DUMP: &str = "(function() local out = {} \
for _, m in ipairs({'n','i','x','s','o','c','t'}) do \
for _, k in ipairs(vim.api.nvim_get_keymap(m)) do table.insert(out, {m, k.lhs, k.desc or '', k.rhs or ''}) end \
for _, k in ipairs(vim.api.nvim_buf_get_keymap(0, m)) do table.insert(out, {m, k.lhs, k.desc or '', k.rhs or ''}) end \
end \
local cfg = package.loaded['fzf-lua.config'] \
if cfg and cfg.globals and cfg.globals.keymap then \
for k, v in pairs(cfg.globals.keymap.fzf or {}) do if v then table.insert(out, {'fzf-lua', k, tostring(v), ''}) end end \
end return vim.json.encode(out) end)()";

/// How long a live nvim gets to answer. It answers in about 13 ms; one that is
/// busy past this is left out rather than holding the report up.
const LIVE_TIMEOUT: Duration = Duration::from_secs(2);

/// How long a headless start gets. A full config with lazy loading starts in
/// well under a second; a first start that installs plugins does not, and is
/// cut off rather than waited for.
const HEADLESS_TIMEOUT: Duration = Duration::from_secs(10);

/// Every nvim pane asked live, or one headless start when none answered.
async fn nvim() -> (Vec<Row>, Vec<String>) {
    let mut rows = Vec::new();
    let mut notes = Vec::new();

    let panes = nvim_panes().await;
    let tree = process_tree().await;
    let dirs = socket_dirs();
    let mut answered = 0;
    for (pane, pane_pid) in &panes {
        let Some(sock) = find_socket(&tree, *pane_pid, &dirs) else {
            notes.push(format!(
                "nvim in {pane}: no socket found, started with --listen elsewhere?"
            ));
            continue;
        };
        let expr = format!("luaeval(\"{NVIM_DUMP}\")");
        let mut cmd = tokio::process::Command::new("nvim");
        cmd.arg("--server")
            .arg(&sock)
            .arg("--remote-expr")
            .arg(&expr);
        match run(cmd, LIVE_TIMEOUT).await {
            Ok(out) => match nvim_rows(&out, Rung::Live, pane) {
                Ok(r) => {
                    answered += 1;
                    rows.extend(r);
                }
                Err(e) => notes.push(format!("nvim in {pane}: {e}")),
            },
            Err(e) => notes.push(format!("nvim in {pane}: {e}")),
        }
    }
    if answered > 0 {
        return (rows, notes);
    }

    let lua = format!("lua io.stdout:write({NVIM_DUMP})");
    let mut cmd = tokio::process::Command::new("nvim");
    cmd.args(["--headless", "-c", &lua, "-c", "qa!"]);
    match run(cmd, HEADLESS_TIMEOUT).await {
        Ok(out) => match nvim_rows(&out, Rung::Config, "") {
            Ok(r) => {
                notes.push("nvim: no running instance answered, read from a headless start".into());
                rows.extend(r);
            }
            Err(e) => notes.push(format!("nvim headless: {e}")),
        },
        Err(e) => notes.push(format!("nvim headless: {e}")),
    }
    (rows, notes)
}

/// How long an interactive zsh gets to start and list its keymaps.
const SHELL_TIMEOUT: Duration = Duration::from_secs(5);

/// Whether a program is on `PATH`.
fn on_path(name: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|path| std::env::split_paths(&path).any(|d| d.join(name).is_file()))
}

/// vim started silent with the user's vimrc, asked for `:map` and `:map!`.
/// Without a vimrc there is nothing of the user's to read.
async fn vim() -> (Vec<Row>, Vec<String>) {
    if !on_path("vim") {
        return (Vec::new(), Vec::new());
    }
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    let Some(vimrc) = [home.join(".vimrc"), home.join(".vim/vimrc")]
        .into_iter()
        .find(|p| p.exists())
    else {
        return (Vec::new(), Vec::new());
    };
    let mut cmd = tokio::process::Command::new("vim");
    cmd.arg("-Es").arg("-u").arg(&vimrc).args([
        "-i",
        "NONE",
        "-c",
        "redir! > /dev/stdout",
        "-c",
        "silent map",
        "-c",
        "silent map!",
        "-c",
        "redir END",
        "-c",
        "qa!",
    ]);
    match run(cmd, HEADLESS_TIMEOUT).await {
        Ok(out) => (super::apps::vim_rows(&out), Vec::new()),
        Err(e) => (Vec::new(), vec![format!("vim: {e}")]),
    }
}

/// Run a command, stdout as text, with a deadline. A child past it is killed.
async fn run(mut cmd: tokio::process::Command, timeout: Duration) -> anyhow::Result<String> {
    cmd.stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    let out = tokio::time::timeout(timeout, cmd.output())
        .await
        .map_err(|_| anyhow::anyhow!("no answer in {} s", timeout.as_secs()))??;
    if !out.status.success() {
        anyhow::bail!("exited {}", out.status);
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// `[mode, lhs, desc, rhs]` arrays from nvim, as rows in scope.
pub fn nvim_rows(json: &str, rung: Rung, pane: &str) -> anyhow::Result<Vec<Row>> {
    let maps: Vec<(String, String, String, String)> =
        serde_json::from_str(json.trim()).map_err(|e| anyhow::anyhow!("unreadable answer: {e}"))?;
    Ok(maps
        .into_iter()
        .filter_map(|(mode, lhs, desc, rhs)| {
            if mode == "fzf-lua" {
                return Some(Row {
                    layer: "fzf-lua".into(),
                    key: spell::from_fzf(&lhs)?,
                    mode: String::new(),
                    desc,
                    source: lhs,
                    rung,
                    pane: pane.to_string(),
                });
            }
            Some(Row {
                layer: "nvim".into(),
                key: spell::from_nvim_lhs(&lhs)?,
                mode,
                desc,
                source: if rhs.is_empty() { lhs } else { rhs },
                rung,
                pane: pane.to_string(),
            })
        })
        .collect())
}

/// Panes whose foreground command is nvim, as (pane id, pane pid).
async fn nvim_panes() -> Vec<(String, u32)> {
    let out = crate::tmux::command()
        .args([
            "list-panes",
            "-a",
            "-F",
            "#{pane_id}\t#{pane_pid}\t#{pane_current_command}",
        ])
        .output()
        .await;
    match out {
        Ok(o) => parse_nvim_panes(&String::from_utf8_lossy(&o.stdout)),
        Err(_) => Vec::new(),
    }
}

/// The parsing half of `nvim_panes`.
pub fn parse_nvim_panes(listing: &str) -> Vec<(String, u32)> {
    listing
        .lines()
        .filter_map(|line| {
            let mut f = line.split('\t');
            let (pane, pid, cmd) = (f.next()?, f.next()?, f.next()?);
            (cmd == "nvim").then(|| Some((pane.to_string(), pid.parse().ok()?)))?
        })
        .collect()
}

/// Parent to children, from one `ps` call.
async fn process_tree() -> HashMap<u32, Vec<u32>> {
    let out = tokio::process::Command::new("ps")
        .args(["-A", "-o", "pid=,ppid="])
        .output()
        .await;
    match out {
        Ok(o) => parse_process_tree(&String::from_utf8_lossy(&o.stdout)),
        Err(_) => HashMap::new(),
    }
}

/// The parsing half of `process_tree`.
pub fn parse_process_tree(listing: &str) -> HashMap<u32, Vec<u32>> {
    let mut tree: HashMap<u32, Vec<u32>> = HashMap::new();
    for line in listing.lines() {
        let mut f = line.split_whitespace();
        if let (Some(Ok(pid)), Some(Ok(ppid))) =
            (f.next().map(str::parse), f.next().map(str::parse))
        {
            tree.entry(ppid).or_default().push(pid);
        }
    }
    tree
}

/// Where nvim puts its sockets: `$XDG_RUNTIME_DIR` on Linux, a per-user
/// directory under `$TMPDIR` (or `/tmp`) everywhere, with one random
/// subdirectory per instance on macOS.
fn socket_dirs() -> Vec<PathBuf> {
    let user = std::env::var("USER").unwrap_or_default();
    let mut dirs = Vec::new();
    if let Some(run) = std::env::var_os("XDG_RUNTIME_DIR") {
        dirs.push(PathBuf::from(run));
    }
    for tmp in [
        std::env::var_os("TMPDIR").map(PathBuf::from),
        Some(PathBuf::from("/tmp")),
    ]
    .into_iter()
    .flatten()
    {
        let base = tmp.join(format!("nvim.{user}"));
        dirs.push(base.clone());
        if let Ok(entries) = std::fs::read_dir(&base) {
            dirs.extend(entries.flatten().map(|e| e.path()).filter(|p| p.is_dir()));
        }
    }
    dirs
}

/// The socket of the nvim under a pane. The pane's pid is the shell; the nvim
/// UI is under it, and since nvim 0.10 the API is served by its `--embed`
/// child, so the search goes a few levels down and takes the first pid that
/// has a socket.
pub fn find_socket(
    tree: &HashMap<u32, Vec<u32>>,
    pane_pid: u32,
    dirs: &[PathBuf],
) -> Option<PathBuf> {
    let mut level = vec![pane_pid];
    for _ in 0..4 {
        for pid in &level {
            let name = format!("nvim.{pid}.0");
            if let Some(p) = dirs.iter().map(|d| d.join(&name)).find(|p| p.exists()) {
                return Some(p);
            }
        }
        level = level
            .iter()
            .flat_map(|p| tree.get(p).cloned().unwrap_or_default())
            .collect();
        if level.is_empty() {
            break;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nvim_maps_become_rows_in_scope_only() {
        let json = r#"[["n","<M-1>","nvim-tree: open","<Cmd>NvimTreeFocus<CR>"],
                       ["n","gx","open","" ],
                       ["i","<C-W><C-D>","","<Cmd>x<CR>"],
                       ["n","<M-h>","",""]]"#;
        let rows = nvim_rows(json, Rung::Live, "%3").unwrap();
        let keys: Vec<_> = rows
            .iter()
            .map(|r| (r.mode.as_str(), r.key.as_str()))
            .collect();
        assert_eq!(keys, [("n", "M-1"), ("i", "C-w"), ("n", "M-h")]);
        assert_eq!(rows[0].desc, "nvim-tree: open");
        assert_eq!(rows[0].pane, "%3");
        assert_eq!(
            rows[2].source, "<M-h>",
            "no rhs, a Lua callback: the lhs stands in"
        );
    }

    #[test]
    fn fzf_lua_binds_come_back_as_their_own_layer() {
        let rows = nvim_rows(r#"[["fzf-lua","alt-a","toggle-all",""]]"#, Rung::Live, "%1").unwrap();
        assert_eq!(rows[0].layer, "fzf-lua");
        assert_eq!(rows[0].key, "M-a");
        assert_eq!(rows[0].desc, "toggle-all");
    }

    #[test]
    fn an_answer_that_is_not_json_is_an_error_not_an_empty_layer() {
        assert!(nvim_rows("E5108: Error", Rung::Live, "").is_err());
    }

    #[test]
    fn only_nvim_panes_are_picked() {
        let listing = "%1\t100\tzsh\n%2\t200\tnvim\n%3\tx\tnvim\n%4\t400\tnvim\n";
        assert_eq!(
            parse_nvim_panes(listing),
            [("%2".to_string(), 200), ("%4".to_string(), 400)]
        );
    }

    #[test]
    fn the_socket_is_found_on_the_embed_child() {
        let dir = std::env::temp_dir().join(format!("kc-sock-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("nvim.302.0"), "").unwrap();
        // shell 100 → nvim UI 301 → nvim --embed 302
        let tree = parse_process_tree("  301   100\n  302   301\n  999     1\n");
        assert_eq!(
            find_socket(&tree, 100, std::slice::from_ref(&dir)),
            Some(dir.join("nvim.302.0"))
        );
        assert_eq!(find_socket(&tree, 999, std::slice::from_ref(&dir)), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn tmux_rows_keep_root_in_scope_only() {
        let row = |table: &str, key: &str| crate::keys::KeyRow {
            table: table.into(),
            key: key.into(),
            shown: key.into(),
            note: "companion: next window".into(),
            command: "next-window".into(),
        };
        let rows = tmux_rows(&[
            row("root", "M-a"),
            row("prefix", "M-b"),
            row("root", "Enter"),
            row("root", "M-MouseDown3Pane"),
        ]);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].key, "M-a");
        assert_eq!(rows[0].mode, "root");
    }

    #[test]
    fn declared_claims_are_rows_for_every_mode() {
        let mut apps = BTreeMap::new();
        apps.insert(
            "nano".to_string(),
            crate::config::KeyApp {
                claims: vec!["C-o".into(), "C-x".into()],
                modes: None,
            },
        );
        let rows = declared_rows(&apps);
        assert_eq!(rows.len(), 2);
        assert!(
            rows.iter()
                .all(|r| r.layer == "nano" && r.mode.is_empty() && r.rung == Rung::Declared)
        );
    }

    #[test]
    fn repeats_from_nvo_maps_collapse_per_mode() {
        let r = |mode: &str| Row {
            layer: "nvim".into(),
            mode: mode.into(),
            key: "M-a".into(),
            desc: String::new(),
            source: String::new(),
            rung: Rung::Live,
            pane: "%1".into(),
        };
        let mut rows = vec![r("n"), r("x"), r("n")];
        sort_and_dedupe(&mut rows);
        assert_eq!(rows.len(), 2);
    }
}
