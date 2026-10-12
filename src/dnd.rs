//! The Linux desktop's do-not-disturb, followed over the session bus for
//! `[quiet] follow_focus`.
//!
//! One connection, a handful of signal subscriptions made once, and a look at
//! a source only when its signal says it changed: no process and no timer.
//! What each desktop exposes, and why these and not others, is in
//! `docs/dev/design-quiet-focus.md`.
//!
//! - GNOME: `org.gnome.desktop.notifications show-banners`, false while Do Not
//!   Disturb is on. dconf announces a write with a `Notify` signal, the
//!   subscription is narrowed to that one key the way GSettings narrows its
//!   own, and the value is read out of the user database file, which is how
//!   every GSettings reader gets it; dconf has no read over the bus.
//! - KDE Plasma: the `Inhibited` property on `org.freedesktop.Notifications`,
//!   which the notifications applet sets from its Do Not Disturb.
//! - dunst: its `paused` property, `dunstctl set-paused`.
//! - mako: a `do-not-disturb` mode, the name mako's own manual uses for it.
//! - SwayNotificationCenter: its `SubscribeV2` signal and `GetDnd`.
//!
//! The parts that take bytes and give an answer build on every system and are
//! tested everywhere; the bus half builds on Linux only.

/// The dconf key GNOME Shell's Do Not Disturb switch writes, inverted.
pub const SHOW_BANNERS: &str = "/org/gnome/desktop/notifications/show-banners";

/// What the Focus is called on Linux, where it has no name of its own: the
/// name of macOS's Do Not Disturb Focus, so `quiet` says both the same way.
pub const DND_NAME: &str = crate::quiet::DO_NOT_DISTURB;

/// The mode mako's manual uses for do-not-disturb, `makoctl mode -a
/// do-not-disturb`. mako has no do-not-disturb of its own; this is the
/// convention.
pub const MAKO_MODE: &str = "do-not-disturb";

/// The user's dconf database, `$XDG_CONFIG_HOME/dconf/user`, or under
/// `~/.config` when that is unset or not absolute, as GLib reads it.
pub fn dconf_user_db(home: &std::path::Path, xdg_config_home: Option<&str>) -> std::path::PathBuf {
    let config = match xdg_config_home {
        Some(dir) if dir.starts_with('/') => std::path::PathBuf::from(dir),
        _ => home.join(".config"),
    };
    config.join("dconf/user")
}

/// Which desktops say do-not-disturb is on right now.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Desktop {
    pub gnome: bool,
    pub kde: bool,
    pub dunst: bool,
    pub mako: bool,
    pub swaync: bool,
}

impl Desktop {
    /// The Focus this makes, for `ServerState::focus`: on when any says so.
    pub fn focus(&self) -> Option<String> {
        (self.gnome || self.kde || self.dunst || self.mako || self.swaync)
            .then(|| DND_NAME.to_string())
    }
}

/// Whether a dconf `Notify` is about show-banners: `prefix` joined to each of
/// `changes` names a key or a directory, and a directory counts when the key
/// is under it. The same test as a D-Bus `arg0path` match, done here because
/// zbus compares an `arg0path` rule by equality on its own side and would
/// drop the directory form dconf sends for a reset.
pub fn dconf_touches(prefix: &str, changes: &[String]) -> bool {
    let touches = |path: &str| {
        path == SHOW_BANNERS || (path.ends_with('/') && SHOW_BANNERS.starts_with(path))
    };
    if changes.is_empty() {
        return touches(prefix);
    }
    changes.iter().any(|c| touches(&format!("{prefix}{c}")))
}

/// Whether mako's modes include do-not-disturb.
pub fn mako_dnd(modes: &[String]) -> bool {
    modes.iter().any(|m| m == MAKO_MODE)
}

/// What a `PropertiesChanged` on `/org/freedesktop/Notifications` says, given
/// its interface and a changed property that is a boolean: KDE's
/// `Inhibited` or dunst's `paused`. Anything else is not about this.
pub fn notifications_property(desktop: &mut Desktop, interface: &str, name: &str, value: bool) {
    match (interface, name) {
        ("org.freedesktop.Notifications", "Inhibited") => desktop.kde = value,
        ("org.dunstproject.cmd0", "paused") => desktop.dunst = value,
        _ => {}
    }
}

/// GNOME's do-not-disturb from the bytes of the dconf user database, or none
/// when there is no database: a machine that never wrote a setting, or not
/// GNOME. A key never written is its default, banners shown.
pub fn gnome_dnd(db: Option<&[u8]>) -> Result<bool, String> {
    match db {
        None => Ok(false),
        Some(bytes) => Ok(gvdb_bool(bytes, SHOW_BANNERS)? == Some(false)),
    }
}

// ── GVDB ───────────────────────────────────────────────────────────────────
//
// The file format dconf keeps its databases in, from GLib's gvdb-format.h and
// gvdb-reader.c: a 24-byte header (two signature words, version, options, a
// pointer to the root table), and the root table as a hash header, its bloom
// words and buckets, then 24-byte items. Each item holds its own fragment of
// the key and the index of its parent, so a key is the fragments up the chain.
// Only reading one boolean is needed here, so the items are walked in order
// rather than hashed: a user database holds hundreds of keys, not millions.

const GVDB_SIGNATURE: &[u8; 8] = b"GVariant";
const GVDB_SWAPPED: &[u8; 8] = b"raVGtnai";
const ITEM_SIZE: usize = 24;
const NO_PARENT: u32 = u32::MAX;

/// The boolean at `key` in a GVDB file, none when the key is absent, an error
/// when the bytes are not a GVDB file or the value is not a boolean.
pub fn gvdb_bool(data: &[u8], key: &str) -> Result<Option<bool>, String> {
    let swapped = match data.get(..8) {
        Some(sig) if sig == GVDB_SIGNATURE => false,
        Some(sig) if sig == GVDB_SWAPPED => true,
        _ => return Err("not a dconf database (no GVDB signature)".into()),
    };
    let u32_at = |at: usize| -> Option<u32> {
        let b: [u8; 4] = data.get(at..at + 4)?.try_into().ok()?;
        Some(if swapped {
            u32::from_be_bytes(b)
        } else {
            u32::from_le_bytes(b)
        })
    };
    let u16_at = |at: usize| -> Option<u16> {
        let b: [u8; 2] = data.get(at..at + 2)?.try_into().ok()?;
        Some(if swapped {
            u16::from_be_bytes(b)
        } else {
            u16::from_le_bytes(b)
        })
    };
    let bad = || "a dconf database that does not hold together".to_string();
    let (root_start, root_end) = (u32_at(16).ok_or_else(bad)?, u32_at(20).ok_or_else(bad)?);
    let (root_start, root_end) = (root_start as usize, root_end as usize);
    if root_end < root_start || root_end > data.len() || root_end - root_start < 8 {
        return Err(bad());
    }
    let n_bloom = (u32_at(root_start).ok_or_else(bad)? & ((1 << 27) - 1)) as usize;
    let n_buckets = u32_at(root_start + 4).ok_or_else(bad)? as usize;
    let items_start = n_bloom
        .checked_add(n_buckets)
        .and_then(|n| n.checked_mul(4))
        .and_then(|n| n.checked_add(root_start + 8))
        .filter(|&s| s <= root_end)
        .ok_or_else(bad)?;
    if (root_end - items_start) % ITEM_SIZE != 0 {
        return Err(bad());
    }
    let n_items = (root_end - items_start) / ITEM_SIZE;

    struct Item {
        parent: u32,
        key: (usize, usize),
        kind: u8,
        value: (usize, usize),
    }
    let item = |i: usize| -> Option<Item> {
        let at = items_start + i * ITEM_SIZE;
        let key_start = u32_at(at + 8)? as usize;
        let key_size = u16_at(at + 12)? as usize;
        Some(Item {
            parent: u32_at(at + 4)?,
            key: (key_start, key_start.checked_add(key_size)?),
            kind: *data.get(at + 14)?,
            value: (u32_at(at + 16)? as usize, u32_at(at + 20)? as usize),
        })
    };
    let fragment = |it: &Item| data.get(it.key.0..it.key.1);

    // The key's fragments, matched from the end up the parent chain, as
    // gvdb_table_check_name does: no string is built.
    let matches = |it: &Item| -> bool {
        let mut rest = key.as_bytes();
        let mut cur = Some((it.parent, fragment(it)));
        for _ in 0..=n_items {
            let Some((parent, Some(frag))) = cur else {
                return false;
            };
            let Some(head) = rest.strip_suffix(frag) else {
                return false;
            };
            rest = head;
            if parent == NO_PARENT {
                return rest.is_empty();
            }
            cur = match item(parent as usize) {
                Some(p) if (parent as usize) < n_items => Some((p.parent, fragment(&p))),
                _ => None,
            };
        }
        false
    };

    for i in 0..n_items {
        let it = item(i).ok_or_else(bad)?;
        if it.kind != b'v' || !matches(&it) {
            continue;
        }
        let value = data
            .get(it.value.0..it.value.1)
            .filter(|_| it.value.0 <= it.value.1)
            .ok_or_else(bad)?;
        return variant_bool(value).map(Some);
    }
    Ok(None)
}

/// A serialised GVariant of type `v` holding a boolean: the child's byte, a
/// zero, and the child's type string `b`.
fn variant_bool(value: &[u8]) -> Result<bool, String> {
    let sep = value
        .iter()
        .rposition(|&b| b == 0)
        .ok_or("a dconf value with no type")?;
    match (&value[..sep], &value[sep + 1..]) {
        ([b], b"b") => Ok(*b != 0),
        (_, ty) => Err(format!(
            "show-banners is not a boolean in the dconf database (type {})",
            String::from_utf8_lossy(ty)
        )),
    }
}

/// A GVDB file holding one boolean at `key`, laid out the way dconf writes
/// one: a list item for the directory and a value item under it. For the
/// tests here and in `tests/`, which have no GNOME to write one.
pub fn gvdb_one_bool(key: &str, value: bool) -> Vec<u8> {
    let (dir, leaf) = key.split_at(key.rfind('/').map_or(0, |i| i + 1));
    let root_start = 24usize;
    let items_start = root_start + 8 + 4; // no bloom words, one bucket
    let strings = items_start + 2 * ITEM_SIZE;
    let dir_at = strings;
    let leaf_at = dir_at + dir.len();
    let list_at = (leaf_at + leaf.len()).next_multiple_of(4);
    let value_at = (list_at + 4).next_multiple_of(8);
    let value_bytes = [u8::from(value), 0, b'b'];

    let mut out = Vec::new();
    out.extend_from_slice(GVDB_SIGNATURE);
    out.extend_from_slice(&0u32.to_le_bytes()); // version
    out.extend_from_slice(&0u32.to_le_bytes()); // options
    out.extend_from_slice(&(root_start as u32).to_le_bytes());
    out.extend_from_slice(&((items_start + 2 * ITEM_SIZE) as u32).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // n_bloom_words
    out.extend_from_slice(&1u32.to_le_bytes()); // n_buckets
    out.extend_from_slice(&0u32.to_le_bytes()); // bucket 0 starts at item 0
    let mut push_item = |parent: u32, at: usize, len: usize, kind: u8, v: (usize, usize)| {
        out.extend_from_slice(&0u32.to_le_bytes()); // hash, unused by this reader
        out.extend_from_slice(&parent.to_le_bytes());
        out.extend_from_slice(&(at as u32).to_le_bytes());
        out.extend_from_slice(&(len as u16).to_le_bytes());
        out.push(kind);
        out.push(0);
        out.extend_from_slice(&(v.0 as u32).to_le_bytes());
        out.extend_from_slice(&(v.1 as u32).to_le_bytes());
    };
    push_item(NO_PARENT, dir_at, dir.len(), b'L', (list_at, list_at + 4));
    push_item(0, leaf_at, leaf.len(), b'v', (value_at, value_at + 3));
    out.extend_from_slice(dir.as_bytes());
    out.extend_from_slice(leaf.as_bytes());
    out.resize(list_at, 0);
    out.extend_from_slice(&1u32.to_le_bytes()); // the directory lists item 1
    out.resize(value_at, 0);
    out.extend_from_slice(&value_bytes);
    out
}

// ── The bus ────────────────────────────────────────────────────────────────

#[cfg(target_os = "linux")]
pub use bus::{follow, watch};

#[cfg(target_os = "linux")]
mod bus {
    use super::*;
    use futures::StreamExt;
    use std::time::Duration;
    use zbus::message::Type as MsgType;
    use zbus::{Connection, MatchRule, MessageStream};

    const NOTIFICATIONS: &str = "org.freedesktop.Notifications";
    const NOTIFICATIONS_PATH: &str = "/org/freedesktop/Notifications";
    const PROPERTIES: &str = "org.freedesktop.DBus.Properties";
    const MAKO_PATH: &str = "/fr/emersion/Mako";
    const MAKO: &str = "fr.emersion.Mako";
    const SWAYNC: &str = "org.erikreider.swaync.cc";
    const SWAYNC_PATH: &str = "/org/erikreider/swaync/cc";
    const DCONF_WRITER: &str = "ca.desrt.dconf.Writer";
    const DCONF_USER: &str = "/ca/desrt/dconf/Writer/user";

    /// How long one property read may take before the source counts as off:
    /// a notification daemon that hangs must not hang the watcher.
    const CALL_TIMEOUT: Duration = Duration::from_secs(2);

    /// What the watcher tells its owner: the Focus now, or why GNOME's
    /// setting could not be read.
    pub type Said = Result<Option<String>, String>;

    /// Whether `name` has an owner on the bus. Asked before every call to a
    /// notification service, so a call never starts one by bus activation.
    async fn has_owner(conn: &Connection, name: &str) -> bool {
        let body = (name,);
        let call = conn.call_method(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            Some("org.freedesktop.DBus"),
            "NameHasOwner",
            &body,
        );
        match tokio::time::timeout(CALL_TIMEOUT, call).await {
            Ok(Ok(reply)) => reply.body().deserialize::<bool>().unwrap_or(false),
            _ => false,
        }
    }

    async fn call<B, R>(
        conn: &Connection,
        dest: &str,
        path: &str,
        iface: &str,
        method: &str,
        body: &B,
    ) -> Option<R>
    where
        B: serde::Serialize + zbus::zvariant::DynamicType,
        R: for<'d> zbus::zvariant::DynamicDeserialize<'d>,
    {
        let call = conn.call_method(Some(dest), path, Some(iface), method, body);
        let reply = tokio::time::timeout(CALL_TIMEOUT, call).await.ok()?.ok()?;
        reply.body().deserialize::<R>().ok()
    }

    async fn get_bool(conn: &Connection, dest: &str, path: &str, iface: &str, prop: &str) -> bool {
        let v: Option<zbus::zvariant::OwnedValue> =
            call(conn, dest, path, PROPERTIES, "Get", &(iface, prop)).await;
        v.and_then(|v| bool::try_from(v).ok()).unwrap_or(false)
    }

    /// Ask every notification service on the bus what it says now: KDE,
    /// dunst and mako behind `org.freedesktop.Notifications`, swaync behind
    /// its own name. One that is not there is off.
    async fn read_services(conn: &Connection, d: &mut Desktop) {
        if has_owner(conn, NOTIFICATIONS).await {
            d.kde = get_bool(
                conn,
                NOTIFICATIONS,
                NOTIFICATIONS_PATH,
                NOTIFICATIONS,
                "Inhibited",
            )
            .await;
            d.dunst = get_bool(
                conn,
                NOTIFICATIONS,
                NOTIFICATIONS_PATH,
                "org.dunstproject.cmd0",
                "paused",
            )
            .await;
            d.mako = read_mako(conn).await;
        } else {
            (d.kde, d.dunst, d.mako) = (false, false, false);
        }
        d.swaync = has_owner(conn, SWAYNC).await
            && call::<_, bool>(conn, SWAYNC, SWAYNC_PATH, SWAYNC, "GetDnd", &())
                .await
                .unwrap_or(false);
    }

    /// mako's modes, by the method `makoctl mode` uses, which older makos
    /// have too.
    async fn read_mako(conn: &Connection) -> bool {
        call::<_, Vec<String>>(conn, NOTIFICATIONS, MAKO_PATH, MAKO, "ListModes", &())
            .await
            .is_some_and(|m| mako_dnd(&m))
    }

    fn read_gnome(path: &std::path::Path) -> Result<bool, String> {
        match std::fs::read(path) {
            Ok(bytes) => gnome_dnd(Some(&bytes)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => gnome_dnd(None),
            Err(e) => Err(format!("cannot read {}: {e}", path.display())),
        }
    }

    fn rules() -> zbus::Result<Vec<MatchRule<'static>>> {
        let signal = || MatchRule::builder().msg_type(MsgType::Signal);
        Ok(vec![
            // Every dconf write: one wakeup each, and `dconf_touches` decides
            // whether the file is read. Narrowing it with `arg0path` would
            // be cheaper, but zbus applies the rule again on its own side by
            // equality, which drops dconf's directory form.
            signal()
                .interface(DCONF_WRITER)?
                .member("Notify")?
                .path(DCONF_USER)?
                .build(),
            signal()
                .interface(PROPERTIES)?
                .member("PropertiesChanged")?
                .path(NOTIFICATIONS_PATH)?
                .build(),
            signal()
                .interface(PROPERTIES)?
                .member("PropertiesChanged")?
                .path(MAKO_PATH)?
                .build(),
            signal()
                .interface(SWAYNC)?
                .member("SubscribeV2")?
                .path(SWAYNC_PATH)?
                .build(),
            // A notification service starting or stopping: swaync owns this
            // name too, so one rule covers all four.
            signal()
                .sender("org.freedesktop.DBus")?
                .interface("org.freedesktop.DBus")?
                .member("NameOwnerChanged")?
                .arg(0, NOTIFICATIONS)?
                .build(),
        ])
    }

    /// Follow every source on `conn` and send the Focus each time it changes,
    /// and once at the start. Returns when the bus goes away.
    ///
    /// `dconf` is the user database file. A GNOME setting that cannot be read
    /// is sent as an error once, and its part stays as it was.
    pub async fn watch(
        conn: &Connection,
        dconf: std::path::PathBuf,
        tx: tokio::sync::mpsc::UnboundedSender<Said>,
    ) -> zbus::Result<()> {
        let mut streams = Vec::new();
        for rule in rules()? {
            streams.push(MessageStream::for_match_rule(rule, conn, None).await?);
        }
        let mut events = futures::stream::select_all(streams);

        let mut d = Desktop::default();
        let mut failing = false;
        let gnome = |d: &mut Desktop, failing: &mut bool| match read_gnome(&dconf) {
            Ok(on) => {
                d.gnome = on;
                *failing = false;
                None
            }
            Err(why) if !*failing => {
                *failing = true;
                Some(why)
            }
            Err(_) => None,
        };
        if let Some(why) = gnome(&mut d, &mut failing) {
            let _ = tx.send(Err(why));
        }
        read_services(conn, &mut d).await;
        let mut said = d.focus();
        if tx.send(Ok(said.clone())).is_err() {
            return Ok(());
        }

        while let Some(msg) = events.next().await {
            let Ok(msg) = msg else { continue };
            let header = msg.header();
            let (Some(path), Some(iface), Some(member)) =
                (header.path(), header.interface(), header.member())
            else {
                continue;
            };
            match (path.as_str(), iface.as_str(), member.as_str()) {
                (DCONF_USER, DCONF_WRITER, "Notify") => {
                    let Ok((prefix, changes, _tag)) =
                        msg.body().deserialize::<(String, Vec<String>, String)>()
                    else {
                        continue;
                    };
                    if !dconf_touches(&prefix, &changes) {
                        continue;
                    }
                    if let Some(why) = gnome(&mut d, &mut failing) {
                        let _ = tx.send(Err(why));
                    }
                }
                (NOTIFICATIONS_PATH, PROPERTIES, "PropertiesChanged") => {
                    type Changed = (
                        String,
                        std::collections::HashMap<String, zbus::zvariant::OwnedValue>,
                        Vec<String>,
                    );
                    if let Ok((interface, changed, _)) = msg.body().deserialize::<Changed>() {
                        for (name, value) in changed {
                            if let Ok(value) = bool::try_from(value) {
                                notifications_property(&mut d, &interface, &name, value);
                            }
                        }
                    }
                }
                // mako says only that Modes changed, not to what.
                (MAKO_PATH, PROPERTIES, "PropertiesChanged") => d.mako = read_mako(conn).await,
                (SWAYNC_PATH, SWAYNC, "SubscribeV2") => {
                    if let Ok((_count, dnd, _cc_open, _inhibited)) =
                        msg.body().deserialize::<(u32, bool, bool, bool)>()
                    {
                        d.swaync = dnd;
                    }
                }
                (_, _, "NameOwnerChanged") => read_services(conn, &mut d).await,
                _ => continue,
            }
            let now = d.focus();
            if now != said {
                said = now.clone();
                if tx.send(Ok(now)).is_err() {
                    return Ok(());
                }
            }
        }
        Ok(())
    }

    /// `[quiet] follow_focus` on Linux: the daemon's half.
    ///
    /// Connects to the session bus once and hands what `watch` sends to
    /// `ServerState`. A change sets the Focus, so the last to speak wins
    /// between the desktop and a hook calling `quiet --focus`; nothing is
    /// written while the desktop says the same as before. No session bus, or
    /// one that goes away, is one health reason; quiet keeps to the timer and
    /// `[quiet] daily`.
    pub async fn follow(
        state: std::sync::Arc<tokio::sync::Mutex<crate::server::state::ServerState>>,
        dconf: std::path::PathBuf,
    ) {
        use crate::quiet::FOCUS_SOURCE;
        let fallback = "quiet follows the timer and [quiet] daily only";
        let conn = match Connection::session().await {
            Ok(conn) => conn,
            Err(e) => {
                let mut st = state.lock().await;
                st.note_failure(
                    FOCUS_SOURCE,
                    format!("focus: no session bus to follow do-not-disturb on: {e}; {fallback}"),
                );
                st.health_cache = None;
                return;
            }
        };
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let watcher = tokio::spawn(async move {
            let ended = watch(&conn, dconf, tx.clone()).await;
            let why = match ended {
                Ok(()) => "the session bus went away".to_string(),
                Err(e) => format!("cannot follow do-not-disturb on the session bus: {e}"),
            };
            let _ = tx.send(Err(why));
        });
        while let Some(said) = rx.recv().await {
            let mut st = state.lock().await;
            match said {
                Ok(focus) => {
                    st.focus = focus;
                    st.note_ok(FOCUS_SOURCE);
                }
                Err(why) => st.note_failure(FOCUS_SOURCE, format!("focus: {why}; {fallback}")),
            }
            st.health_cache = None;
        }
        let _ = watcher.await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_database_dconf_wrote_reads_back_its_boolean() {
        for value in [true, false] {
            let db = gvdb_one_bool(SHOW_BANNERS, value);
            assert_eq!(gvdb_bool(&db, SHOW_BANNERS), Ok(Some(value)));
        }
    }

    #[test]
    fn a_key_the_database_does_not_hold_is_none() {
        let db = gvdb_one_bool("/org/gnome/desktop/interface/clock-format", true);
        assert_eq!(gvdb_bool(&db, SHOW_BANNERS), Ok(None));
        assert_eq!(
            gvdb_bool(&db, "/org/gnome/desktop/interface/"),
            Ok(None),
            "a directory is not a value"
        );
        let db = gvdb_one_bool(SHOW_BANNERS, true);
        assert_eq!(
            gvdb_bool(&db, "/x/org/gnome/desktop/notifications/show-banners"),
            Ok(None),
            "the key has to match all the way to the root"
        );
    }

    #[test]
    fn bytes_that_are_not_a_database_are_an_error_not_a_guess() {
        assert!(gvdb_bool(b"", SHOW_BANNERS).is_err());
        assert!(
            gvdb_bool(b"GVariant", SHOW_BANNERS).is_err(),
            "header cut short"
        );
        let mut db = gvdb_one_bool(SHOW_BANNERS, true);
        db.truncate(24 + 8 + 4 + ITEM_SIZE + 4);
        assert!(gvdb_bool(&db, SHOW_BANNERS).is_err(), "items cut short");
        let mut db = gvdb_one_bool(SHOW_BANNERS, true);
        let n = db.len();
        db[n - 1] = b's';
        assert!(
            gvdb_bool(&db, SHOW_BANNERS).is_err(),
            "a string is not a boolean"
        );
    }

    #[test]
    fn a_parent_chain_that_loops_ends_rather_than_spinning() {
        let mut db = gvdb_one_bool(SHOW_BANNERS, true);
        // The directory item's parent, at items_start + 4, set to itself.
        let items_start = 24 + 8 + 4;
        db[items_start + 4..items_start + 8].copy_from_slice(&0u32.to_le_bytes());
        assert_eq!(gvdb_bool(&db, SHOW_BANNERS), Ok(None));
    }

    #[test]
    fn gnome_is_do_not_disturb_when_banners_are_off() {
        assert_eq!(gnome_dnd(None), Ok(false), "no database, no setting");
        let off = gvdb_one_bool(SHOW_BANNERS, false);
        assert_eq!(gnome_dnd(Some(&off)), Ok(true));
        let on = gvdb_one_bool(SHOW_BANNERS, true);
        assert_eq!(gnome_dnd(Some(&on)), Ok(false));
        let other = gvdb_one_bool("/org/gnome/desktop/interface/clock-format", false);
        assert_eq!(
            gnome_dnd(Some(&other)),
            Ok(false),
            "never written is the default"
        );
        assert!(gnome_dnd(Some(b"nonsense")).is_err());
    }

    #[test]
    fn the_properties_that_count_are_kde_inhibited_and_dunst_paused() {
        let mut d = Desktop::default();
        notifications_property(&mut d, "org.freedesktop.Notifications", "Inhibited", true);
        assert!(d.kde);
        notifications_property(&mut d, "org.dunstproject.cmd0", "paused", true);
        assert!(d.dunst);
        notifications_property(&mut d, "org.dunstproject.cmd0", "displayedLength", true);
        notifications_property(&mut d, "org.example.Other", "Inhibited", false);
        assert!(d.kde && d.dunst, "other properties change nothing");
    }

    #[test]
    fn any_desktop_saying_so_is_the_focus() {
        assert_eq!(Desktop::default().focus(), None);
        for d in [
            Desktop {
                gnome: true,
                ..Default::default()
            },
            Desktop {
                kde: true,
                ..Default::default()
            },
            Desktop {
                dunst: true,
                ..Default::default()
            },
            Desktop {
                mako: true,
                ..Default::default()
            },
            Desktop {
                swaync: true,
                ..Default::default()
            },
        ] {
            assert_eq!(d.focus().as_deref(), Some(DND_NAME), "{d:?}");
        }
    }

    #[test]
    fn a_dconf_write_counts_when_it_names_show_banners_or_a_directory_above_it() {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert!(dconf_touches(SHOW_BANNERS, &s(&[""])), "the key itself");
        assert!(dconf_touches(
            "/org/gnome/desktop/notifications/",
            &s(&["show-banners"])
        ));
        assert!(
            dconf_touches("/org/gnome/desktop/notifications/", &s(&[""])),
            "a reset of its directory"
        );
        assert!(dconf_touches("/", &s(&[""])), "a reset of everything");
        assert!(dconf_touches(
            "/org/gnome/",
            &s(&["nautilus/x", "desktop/notifications/show-banners"])
        ));
        assert!(!dconf_touches(
            "/org/gnome/nautilus/window-state/",
            &s(&["geometry"])
        ));
        assert!(
            !dconf_touches(
                "/org/gnome/desktop/notifications/application/x/",
                &s(&["show-banners"])
            ),
            "an app's own switch is not Do Not Disturb"
        );
        assert!(!dconf_touches(
            "/org/gnome/desktop/notifications/show-banners-extra",
            &s(&[""])
        ));
        assert!(
            dconf_touches(SHOW_BANNERS, &[]),
            "no changes list names the prefix"
        );
    }

    #[test]
    fn mako_is_do_not_disturb_by_its_manual_s_mode_name() {
        assert!(mako_dnd(&["default".into(), "do-not-disturb".into()]));
        assert!(!mako_dnd(&["default".into()]));
        assert!(!mako_dnd(&["dnd".into()]), "only the name the manual uses");
    }

    #[test]
    fn the_user_database_sits_under_the_config_dir() {
        let home = std::path::Path::new("/home/u");
        assert_eq!(
            dconf_user_db(home, None),
            std::path::PathBuf::from("/home/u/.config/dconf/user")
        );
        assert_eq!(
            dconf_user_db(home, Some("/x/cfg")),
            std::path::PathBuf::from("/x/cfg/dconf/user")
        );
        assert_eq!(
            dconf_user_db(home, Some("relative")),
            std::path::PathBuf::from("/home/u/.config/dconf/user"),
            "GLib ignores a relative XDG_CONFIG_HOME"
        );
    }
}
