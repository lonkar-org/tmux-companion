//! What a pane is running, by the name it was started as, when tmux's name
//! for it says too little.
//!
//! tmux's `pane_current_command` is the kernel's name for the foreground
//! process group of the pane's terminal. On macOS that is the name of the file
//! that was executed, cut to 15 bytes; on Linux it is `argv[0]`. Neither is
//! the name a person typed often enough to match `[agents] programs` on its
//! own:
//!
//! - claude runs a file named for its version through a symlink called
//!   `claude`, so macOS says `2.1.296`;
//! - grok does the same with `grok-1.0.50-macos-x86_64`, so macOS says
//!   `grok-1.0.50-mac`;
//! - gemini, cursor-agent and the npm installs of codex and opencode are node
//!   scripts, so both systems say `node`, and aider is a Python script, which
//!   macOS calls `Python` and Linux `python3`.
//!
//! The argument vector says what each one is. `argv[0]` is the name it was
//! started by, `claude` or `grok`, whatever the file behind the symlink is
//! called, and for an interpreter the script it was given is the program:
//! `node /usr/local/bin/gemini` is gemini.
//!
//! It is read only for a pane whose name is one of the kinds above
//! ([`unreliable`]); a shell, an editor or `cargo` costs nothing. For one that
//! is, the terminal's foreground group is one system call on macOS
//! (`proc_pidinfo`) and one small read of `/proc` on Linux, and the arguments
//! of that group's leader one more (`KERN_PROCARGS2`, `/proc/<pid>/cmdline`).
//! The arguments are kept by foreground group and name, so a daemon that
//! lists the panes every two seconds reads them once per program started and
//! asks only for the group after that. Nothing here starts a process. See
//! `docs/dev/design-agent-identity.md`.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

/// The name a pane's foreground program was started by, when tmux's name for
/// it is one of the kinds that hides it, and differs from that name.
///
/// `None` for a pane whose name is already the program's, for one whose
/// process cannot be read, and for a pane with no pid (a test's listing).
pub fn of(pane_pid: u32, command: &str) -> Option<String> {
    if pane_pid == 0 || !unreliable(command) {
        return None;
    }
    let leader = foreground(pane_pid)?;
    let key = (leader, command.to_string());
    if let Some(kept) = KEPT.lock().ok().and_then(|k| k.get(&key).cloned()) {
        return kept;
    }
    let name = argv(leader)
        .as_deref()
        .and_then(name_from_argv)
        .filter(|n| n != command);
    if let Ok(mut kept) = KEPT.lock() {
        // Bounded rather than pruned: the key is a process group, and a
        // server that has started more than this many programs since the
        // daemon did can afford one more read each.
        if kept.len() >= KEPT_MAX {
            kept.clear();
        }
        kept.insert(key, name.clone());
    }
    name
}

/// Identities already read, by foreground group and tmux's name for it. A
/// pid reused for another program would have to come back under the same
/// name to be mistaken for the first, and the name is part of the key.
static KEPT: LazyLock<Mutex<Table>> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// Foreground group and tmux's name, to the name read for it.
type Table = HashMap<(u32, String), Option<String>>;

/// How many identities are kept before the table starts again.
const KEPT_MAX: usize = 256;

/// Whether tmux's name for a pane is one that can hide the program: a bare
/// version (claude), a name with a release after it (grok), an interpreter
/// (node, bun, deno, Python, ruby, perl), or a name 15 bytes long, which on
/// macOS is a longer one cut short. A shell is not: in front, it is at its
/// prompt.
pub fn unreliable(command: &str) -> bool {
    let c = command.trim();
    crate::panes::is_version_name(c)
        || crate::panes::versioned_name(c).is_some()
        || is_interpreter(c)
        || c.len() >= MACOS_COMM
}

/// How many bytes of a name macOS keeps.
const MACOS_COMM: usize = 15;

/// Interpreters whose program is the script they were given.
fn is_interpreter(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    matches!(
        n.as_str(),
        "node" | "nodejs" | "bun" | "deno" | "ruby" | "perl" | "pypy" | "pypy3"
    ) || (n.starts_with("python")
        && n["python".len()..]
            .bytes()
            .all(|b| b.is_ascii_digit() || b == b'.'))
}

/// Shells, which are the program when they are at a prompt and name their
/// script when they run one.
fn is_shell(name: &str) -> bool {
    matches!(
        name,
        "sh" | "bash" | "zsh" | "dash" | "ksh" | "mksh" | "fish"
    )
}

/// The last component of a path, without the `-` a login shell's `argv[0]`
/// starts with: what tmux itself shows for one.
fn base(path: &str) -> &str {
    let p = path.trim_end_matches('/');
    let b = p.rsplit('/').next().unwrap_or(p);
    b.trim_start_matches('-')
}

/// Flags that take the next argument as their value, across the interpreters
/// above, so the value is not mistaken for the script.
const VALUE_FLAGS: &[&str] = &[
    "-r",
    "--require",
    "--import",
    "--loader",
    "--experimental-loader",
    "-C",
    "--conditions",
    "--title",
    "-W",
    "-X",
    "-I",
    "-o",
    "+o",
    "--config",
    "--preload",
];

/// Flags after which the program is code on the command line, not a file.
const INLINE_FLAGS: &[&str] = &["-c", "-e", "--eval", "-p", "--print"];

/// Words an interpreter takes before the script: `bun run x.ts`, `deno run`.
const SUBCOMMANDS: &[&str] = &["run", "x"];

/// The program an argument vector runs.
///
/// `argv[0]`'s last component, unless that is an interpreter or a shell with
/// a script, in which case the script's: `node /usr/local/bin/gemini` is
/// `gemini`, `python3 -m aider` is `aider`. A script with a generic name,
/// `index.js` or `__main__.py`, is named for the package it sits in:
/// `.../cursor-agent/versions/2025.09.18-a1/index.js` is `cursor-agent`, and
/// `.../@google/gemini-cli/dist/index.js` is `gemini`, since a package called
/// `x-cli` is the command-line `x`. `None` for an empty vector.
pub fn name_from_argv(argv: &[String]) -> Option<String> {
    let first = base(argv.first()?);
    if first.is_empty() {
        return None;
    }
    if !is_interpreter(first) && !is_shell(first) {
        return Some(first.to_string());
    }
    Some(match script(&argv[1..]) {
        Some(Script::Path(p)) => script_name(p).unwrap_or(first).to_string(),
        Some(Script::Module(m)) => m.split('.').next().unwrap_or(m).to_string(),
        None => first.to_string(),
    })
}

/// What an interpreter was asked to run.
#[derive(Debug, PartialEq, Eq)]
enum Script<'a> {
    /// A file.
    Path(&'a str),
    /// `python -m module`.
    Module(&'a str),
}

/// The script among an interpreter's arguments, past its own flags.
fn script(args: &[String]) -> Option<Script<'_>> {
    let mut it = args.iter().map(String::as_str);
    let mut first = true;
    while let Some(a) = it.next() {
        if a == "--" {
            return it.next().map(Script::Path);
        }
        if a == "-m" {
            return it.next().map(Script::Module);
        }
        if INLINE_FLAGS.contains(&a) {
            return None;
        }
        if VALUE_FLAGS.contains(&a) {
            it.next();
            continue;
        }
        if a.starts_with('-') || a.starts_with('+') {
            continue;
        }
        if first && SUBCOMMANDS.contains(&a) {
            first = false;
            continue;
        }
        return Some(Script::Path(a));
    }
    None
}

/// Script names that say nothing about the program.
const GENERIC: &[&str] = &[
    "index", "main", "cli", "__main__", "entry", "bin", "run", "start", "app",
];

/// Directories a package keeps its scripts in, which are not its name.
const PLUMBING: &[&str] = &[
    "dist",
    "build",
    "lib",
    "bin",
    "src",
    "out",
    "cjs",
    "esm",
    "node_modules",
    "versions",
    "libexec",
    "current",
    "latest",
    "site-packages",
];

/// A script path's program name: its file name without the extension, or,
/// for a generic one, the package directory it sits in.
fn script_name(path: &str) -> Option<&str> {
    let file = base(path);
    let stem = match file.rsplit_once('.') {
        Some((s, ext)) if is_script_ext(ext) && !s.is_empty() => s,
        _ => file,
    };
    if !GENERIC.contains(&stem) {
        return (!stem.is_empty()).then_some(stem);
    }
    let package = path
        .rsplit('/')
        .skip(1)
        .filter(|c| !c.is_empty())
        .find(|c| !PLUMBING.contains(c) && !version_like(c))?;
    let package = package.trim_start_matches('@');
    Some(package.strip_suffix("-cli").unwrap_or(package))
}

fn is_script_ext(ext: &str) -> bool {
    matches!(
        ext,
        "js" | "mjs" | "cjs" | "ts" | "mts" | "py" | "rb" | "pl" | "sh"
    )
}

/// `2025.09.18-a1`, `v1.2.0`, `1.0.50`: a directory a release lives in.
fn version_like(c: &str) -> bool {
    let c = c.strip_prefix('v').unwrap_or(c);
    c.bytes().next().is_some_and(|b| b.is_ascii_digit())
}

// ── reading the process ──────────────────────────────────────────────────

/// The foreground process group of the terminal `pid` controls, which is
/// what tmux names the pane after. The shell a pane started with holds that
/// terminal, so its record says which group is in front.
#[cfg(target_os = "macos")]
fn foreground(pid: u32) -> Option<u32> {
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
    // SAFETY: the buffer is a zeroed proc_bsdinfo of exactly `size` bytes,
    // which is what PROC_PIDTBSDINFO writes.
    let got = unsafe {
        libc::proc_pidinfo(
            pid as libc::c_int,
            libc::PROC_PIDTBSDINFO,
            0,
            (&mut info as *mut libc::proc_bsdinfo).cast(),
            size,
        )
    };
    (got == size && info.e_tpgid != 0).then_some(info.e_tpgid)
}

/// The argument vector of `pid`, from `KERN_PROCARGS2`.
#[cfg(target_os = "macos")]
fn argv(pid: u32) -> Option<Vec<String>> {
    let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid as libc::c_int];
    let mut size: libc::size_t = 0;
    // SAFETY: a null buffer asks for the size only.
    let ok = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            3,
            std::ptr::null_mut(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if ok != 0 || size == 0 {
        return None;
    }
    let mut buf = vec![0u8; size];
    // SAFETY: `buf` holds `size` bytes, and sysctl writes at most that many
    // and says how many it wrote.
    let ok = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            3,
            buf.as_mut_ptr().cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if ok != 0 {
        return None;
    }
    buf.truncate(size);
    parse_procargs2(&buf)
}

#[cfg(target_os = "linux")]
fn foreground(pid: u32) -> Option<u32> {
    tpgid_from_stat(&std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?)
}

#[cfg(target_os = "linux")]
fn argv(pid: u32) -> Option<Vec<String>> {
    let raw = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    let v = split_nul(&raw);
    (!v.is_empty()).then_some(v)
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn foreground(_pid: u32) -> Option<u32> {
    None
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn argv(_pid: u32) -> Option<Vec<String>> {
    None
}

/// The arguments in a `KERN_PROCARGS2` buffer: a native-endian `argc`, the
/// executable's path, NUL padding, then `argc` NUL-terminated arguments and
/// the environment, which is left alone.
pub fn parse_procargs2(buf: &[u8]) -> Option<Vec<String>> {
    let argc = i32::from_ne_bytes(buf.get(..4)?.try_into().ok()?);
    let argc = usize::try_from(argc).ok().filter(|n| *n > 0)?;
    let rest = &buf[4..];
    let path_end = rest.iter().position(|b| *b == 0)?;
    let args_start = path_end + rest[path_end..].iter().position(|b| *b != 0)?;
    let args: Vec<String> = rest[args_start..]
        .split(|b| *b == 0)
        .take(argc)
        .map(|a| String::from_utf8_lossy(a).into_owned())
        .collect();
    (args.len() == argc).then_some(args)
}

/// `/proc/<pid>/cmdline`: arguments separated and ended by NUL.
pub fn split_nul(raw: &[u8]) -> Vec<String> {
    raw.split(|b| *b == 0)
        .filter(|a| !a.is_empty())
        .map(|a| String::from_utf8_lossy(a).into_owned())
        .collect()
}

/// The terminal's foreground group from `/proc/<pid>/stat`: the fifth field
/// after the command name, which is in parentheses and may hold anything,
/// so the fields are counted from its last `)`.
pub fn tpgid_from_stat(stat: &str) -> Option<u32> {
    let after = &stat[stat.rfind(')')? + 1..];
    after
        .split_whitespace()
        .nth(5)?
        .parse::<i64>()
        .ok()
        .and_then(|n| u32::try_from(n).ok())
        .filter(|n| *n != 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| s.to_string()).collect()
    }

    fn name(args: &[&str]) -> Option<String> {
        name_from_argv(&v(args))
    }

    #[test]
    fn a_native_agent_is_the_name_it_was_started_by() {
        // macOS calls these `2.1.296` and `grok-1.0.50-mac`; argv[0] is the
        // symlink's name.
        assert_eq!(name(&["claude", "--continue"]).as_deref(), Some("claude"));
        assert_eq!(name(&["grok"]).as_deref(), Some("grok"));
        assert_eq!(name(&["/Users/you/.local/bin/agy"]).as_deref(), Some("agy"));
        assert_eq!(
            name(&["/home/you/.opencode/bin/opencode", "run"]).as_deref(),
            Some("opencode")
        );
        assert_eq!(name(&["codex", "resume"]).as_deref(), Some("codex"));
    }

    #[test]
    fn a_node_agent_is_the_script_it_was_given() {
        // `#!/usr/bin/env node` makes the kernel run node with the path the
        // shell found on PATH, the bin symlink, not the file behind it.
        assert_eq!(
            name(&["node", "/usr/local/bin/gemini"]).as_deref(),
            Some("gemini")
        );
        assert_eq!(
            name(&[
                "/usr/bin/node",
                "--no-warnings=DEP0040",
                "/home/you/.npm-global/bin/codex"
            ])
            .as_deref(),
            Some("codex")
        );
        assert_eq!(
            name(&["node", "/opt/homebrew/bin/opencode"]).as_deref(),
            Some("opencode")
        );
        // gemini restarts itself with a bigger heap, under the resolved path.
        assert_eq!(
            name(&[
                "/usr/local/bin/node",
                "--max-old-space-size=8192",
                "/usr/local/lib/node_modules/@google/gemini-cli/dist/index.js"
            ])
            .as_deref(),
            Some("gemini")
        );
        // cursor-agent's launcher runs its own node on index.js.
        assert_eq!(
            name(&[
                "/home/you/.local/share/cursor-agent/versions/2025.09.18-7ae6800/node",
                "--use-system-ca",
                "/home/you/.local/share/cursor-agent/versions/2025.09.18-7ae6800/index.js"
            ])
            .as_deref(),
            Some("cursor-agent")
        );
        assert_eq!(
            name(&["bun", "run", "/w/tool/cli.ts"]).as_deref(),
            Some("tool")
        );
        assert_eq!(
            name(&["node", "-r", "ts-node/register", "/w/agent.ts"]).as_deref(),
            Some("agent")
        );
    }

    #[test]
    fn a_python_agent_is_its_script_or_its_module() {
        // pip, pipx and uv write a console script with a python shebang.
        assert_eq!(
            name(&[
                "/home/you/.local/pipx/venvs/aider-chat/bin/python",
                "/home/you/.local/bin/aider"
            ])
            .as_deref(),
            Some("aider")
        );
        // macOS's framework Python is called `Python`.
        assert_eq!(
            name(&[
                "/usr/local/Cellar/python@3.12/3.12.7/Frameworks/Python.framework/Versions/3.12/Resources/Python.app/Contents/MacOS/Python",
                "/Users/you/.local/bin/aider",
                "--model",
                "x"
            ])
            .as_deref(),
            Some("aider")
        );
        assert_eq!(name(&["python3", "-m", "aider"]).as_deref(), Some("aider"));
        assert_eq!(
            name(&["python3.12", "-X", "utf8", "-m", "aider.main"]).as_deref(),
            Some("aider")
        );
    }

    #[test]
    fn code_on_the_command_line_and_a_prompt_are_the_interpreter() {
        assert_eq!(name(&["node", "-e", "1"]).as_deref(), Some("node"));
        assert_eq!(name(&["node"]).as_deref(), Some("node"));
        assert_eq!(name(&["python3", "-c", "x"]).as_deref(), Some("python3"));
        assert_eq!(name(&["-zsh"]).as_deref(), Some("zsh"));
        assert_eq!(name(&["bash", "-c", "claude"]).as_deref(), Some("bash"));
        assert_eq!(
            name(&["bash", "/home/you/bin/my-agent.sh"]).as_deref(),
            Some("my-agent")
        );
        assert_eq!(name(&[]), None);
        assert_eq!(name(&[""]), None);
    }

    #[test]
    fn only_a_name_that_can_hide_the_program_is_looked_into() {
        for hides in [
            "2.1.296",
            "grok-1.0.50-mac",
            "node",
            "bun",
            "Python",
            "python3.12",
            "abcdefghijklmno",
        ] {
            assert!(unreliable(hides), "{hides}");
        }
        for plain in [
            "zsh",
            "bash",
            "nvim",
            "cargo",
            "claude",
            "gemini",
            "gcc-13",
            "pythonista",
        ] {
            assert!(!unreliable(plain), "{plain}");
        }
    }

    #[test]
    fn a_pane_without_a_pid_is_not_looked_into() {
        assert_eq!(of(0, "node"), None);
    }

    #[test]
    fn procargs2_is_argc_the_path_padding_then_the_arguments() {
        let mut buf = 2i32.to_ne_bytes().to_vec();
        buf.extend_from_slice(b"/Users/you/.grok/downloads/grok-1.0.50-macos-x86_64\0\0\0\0");
        buf.extend_from_slice(b"grok\0--resume\0HOME=/Users/you\0");
        assert_eq!(parse_procargs2(&buf), Some(v(&["grok", "--resume"])));
        assert_eq!(parse_procargs2(&[1, 0]), None, "too short");
        let mut short = 3i32.to_ne_bytes().to_vec();
        short.extend_from_slice(b"/bin/x\0x\0");
        assert_eq!(parse_procargs2(&short), None, "fewer arguments than argc");
    }

    #[test]
    fn proc_stat_is_counted_from_the_last_parenthesis() {
        let stat = "4242 (my (odd) name) S 4000 4242 4000 34817 4242 4194560 0 0";
        assert_eq!(tpgid_from_stat(stat), Some(4242));
        let detached = "1 (init) S 0 1 1 0 -1 4194560";
        assert_eq!(tpgid_from_stat(detached), None);
        assert_eq!(
            split_nul(b"node\0/usr/bin/gemini\0"),
            v(&["node", "/usr/bin/gemini"])
        );
    }

    #[test]
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    fn this_process_reads_back_as_itself() {
        let me = std::process::id();
        let args = argv(me).expect("own argv");
        assert_eq!(args, std::env::args().collect::<Vec<_>>());
    }
}
