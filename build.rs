//! Stamps a build identifier into the binary.
//!
//! The version alone cannot answer "is this daemon from the build I just
//! installed": during development every build is 0.1.0, and an upgraded binary
//! on disk changes nothing until the old process exits. The stamp makes the
//! mismatch visible, which is what lets a client restart a stale daemon
//! instead of quietly getting an older answer.

use std::time::{SystemTime, UNIX_EPOCH};

fn main() {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    println!("cargo:rustc-env=TMUX_COMPANION_BUILD={secs}");

    // Without this the stamp would be baked once and never refreshed, which is
    // the opposite of what it is for.
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=Cargo.toml");
}
