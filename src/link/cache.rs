//! Each host's last `sessions` answer, kept on disk as `hosts/<name>.bin` in
//! the state directory, so the first picker after a restart lists a host
//! before any ssh has run and says when it was last heard from.
//!
//! The file is a Cap'n Proto `HostCache` message from the link's own schema.
//! It is read back through the same cleaning as an answer off the wire: a
//! file on disk is as trusted as what was written into it, which came from
//! the host.

use std::path::{Path, PathBuf};

use super::{Session, read_sessions, reader_options, write_sessions};
use crate::link_capnp::host_cache;

/// What the file holds.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Cached {
    /// Unix seconds the answer arrived.
    pub heard_at: u64,
    /// Unix seconds the host's daemon read it.
    pub polled_at: u64,
    /// The host's build, from its hello.
    pub build: String,
    /// The sessions.
    pub sessions: Vec<Session>,
}

/// `<state dir>/hosts`.
pub fn dir() -> Option<PathBuf> {
    crate::server::state_dir().map(|d| d.join("hosts"))
}

/// The file for one host in `dir`. The name is checked before any host gets
/// this far, so it is a plain file name.
pub fn path_in(dir: &Path, name: &str) -> PathBuf {
    dir.join(format!("{name}.bin"))
}

/// Read one host's file; `None` when there is none or it doesn't parse.
pub fn read(path: &Path) -> Option<Cached> {
    let bytes = std::fs::read(path).ok()?;
    let msg = capnp::serialize::read_message(&mut bytes.as_slice(), reader_options()).ok()?;
    let root = msg.get_root::<host_cache::Reader>().ok()?;
    Some(Cached {
        heard_at: root.get_heard_at(),
        polled_at: root.get_polled_at(),
        build: super::clean(&String::from_utf8_lossy(root.get_build().ok()?.as_bytes())),
        sessions: read_sessions(root.get_sessions().ok()?).ok()?,
    })
}

/// Write one host's file, through a temporary file and a rename so a reader
/// never sees half of one.
pub fn write(path: &Path, c: &Cached) -> std::io::Result<()> {
    let mut msg = capnp::message::Builder::new_default();
    {
        let mut root = msg.init_root::<host_cache::Builder>();
        root.set_heard_at(c.heard_at);
        root.set_polled_at(c.polled_at);
        root.set_build(&c.build);
        write_sessions(root.init_sessions(c.sessions.len() as u32), &c.sessions);
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension(format!("bin.{}.tmp", std::process::id()));
    let mut bytes = Vec::new();
    capnp::serialize::write_message(&mut bytes, &msg).map_err(std::io::Error::other)?;
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

/// Take one host's file away.
pub fn remove(path: &Path) {
    let _ = std::fs::remove_file(path);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cache_round_trips() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = path_in(dir.path(), "vm");
        let c = Cached {
            heard_at: 1_700_000_000,
            polled_at: 1_699_999_999,
            build: "0.10.0+1.abc".into(),
            sessions: vec![
                Session {
                    name: "api".into(),
                    id: "$1".into(),
                    windows: 3,
                    attached: 1,
                    last_attached: 5,
                    activity: 6,
                    used: 7,
                    path: "/src/api".into(),
                },
                Session {
                    name: "x'; touch pwned; '".into(),
                    ..Session::default()
                },
            ],
        };
        write(&path, &c).expect("writes");
        assert_eq!(read(&path), Some(c));
        assert!(
            std::fs::read_dir(dir.path()).expect("dir").all(|e| !e
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .contains("tmp")),
            "no temporary file left behind"
        );
    }

    #[test]
    fn a_file_that_is_not_a_cache_reads_as_none() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = path_in(dir.path(), "vm");
        std::fs::write(&path, b"not a message at all, just bytes").expect("writes");
        assert_eq!(read(&path), None);
        assert_eq!(read(&path_in(dir.path(), "absent")), None);
    }
}
