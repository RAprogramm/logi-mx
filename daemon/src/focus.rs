// SPDX-FileCopyrightText: 2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Hyprland focus watcher feeding the per-application gesture profiles.
//!
//! Hyprland pushes every focus change over its event socket as
//! `activewindow>>class,title`; the watcher thread follows that stream and
//! keeps the focused window class in a shared slot the gesture runtime reads
//! when resolving bindings. The initial window is queried once through the
//! request socket so profiles apply without waiting for the first switch.
//! Outside Hyprland the watcher stays inactive and the slot stays `None`,
//! which means "no app focused" and keeps the global bindings in charge.

use std::{
    io::{BufRead, BufReader, Read, Write},
    os::unix::net::UnixStream,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    thread,
    time::Duration
};

use tracing::{debug, info};

/// Delay before reconnecting after the event stream drops.
const RECONNECT_DELAY: Duration = Duration::from_secs(1);

/// Read timeout for the one-shot request socket query.
const QUERY_TIMEOUT: Duration = Duration::from_secs(2);

/// Shared slot holding the class of the focused window, `None` when the
/// desktop or the compositor does not report one.
pub type SharedFocus = Arc<Mutex<Option<String>>>;

/// Creates an empty focus slot.
#[must_use]
pub fn new_shared() -> SharedFocus {
    Arc::new(Mutex::new(None))
}

/// Resolves the Hyprland socket directory for the current session.
///
/// First from the environment, then by discovering the newest live instance on
/// disk. The fallback matters for systemd user services, which do not inherit
/// `HYPRLAND_INSTANCE_SIGNATURE`.
///
/// Shared with [`crate::hyprbind`], which talks to the request socket of the
/// same directory.
#[must_use]
pub fn instance_dir() -> Option<PathBuf> {
    let runtime = std::env::var("XDG_RUNTIME_DIR").ok()?;
    env_dir(&runtime).or_else(|| discover_dir(&runtime))
}

/// The instance directory named by `HYPRLAND_INSTANCE_SIGNATURE`, if it
/// exists.
fn env_dir(runtime: &str) -> Option<PathBuf> {
    let signature = std::env::var("HYPRLAND_INSTANCE_SIGNATURE").ok()?;
    let dir = PathBuf::from(runtime).join("hypr").join(signature);
    dir.is_dir().then_some(dir)
}

/// The newest `hypr/<signature>` directory that carries an event socket.
fn discover_dir(runtime: &str) -> Option<PathBuf> {
    let root = Path::new(runtime).join("hypr");
    let entries = std::fs::read_dir(root).ok()?;
    entries
        .flatten()
        .filter(|entry| entry.path().join(".socket2.sock").is_file())
        .max_by_key(|entry| entry.metadata().ok().and_then(|m| m.modified().ok()))
        .map(|entry| entry.path())
}

/// Extracts the `class:` field from an `activewindow` text reply.
fn text_class(reply: &str) -> Option<&str> {
    reply
        .lines()
        .find_map(|line| line.trim().strip_prefix("class:"))
        .map(str::trim)
        .filter(|class| !class.is_empty())
}

/// Parses one event-socket line, returning the class of an `activewindow`
/// event.
fn event_class(line: &str) -> Option<&str> {
    let payload = line.strip_prefix("activewindow>>")?;
    payload.split(',').next().filter(|class| !class.is_empty())
}

/// Queries the currently active window class through the request socket.
fn initial_class(dir: &Path) -> Option<String> {
    let mut stream = UnixStream::connect(dir.join(".socket.sock")).ok()?;
    stream.set_read_timeout(Some(QUERY_TIMEOUT)).ok()?;
    stream.write_all(b"activewindow").ok()?;
    stream.flush().ok()?;
    let mut reply = String::new();
    stream.read_to_string(&mut reply).ok()?;
    text_class(&reply).map(str::to_owned)
}

/// Stores a focus class, replacing the previous value.
fn store(shared: &SharedFocus, class: Option<&str>) {
    let mut guard = shared
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    *guard = class.map(str::to_owned);
}

/// Consumes event lines until the socket closes.
fn follow(stream: &UnixStream, shared: &SharedFocus) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "focus event stream closed"
            ));
        }
        let trimmed = line.trim_end();
        if let Some(class) = event_class(trimmed) {
            store(shared, Some(class));
        }
    }
}

/// Spawns the watcher thread; outside Hyprland it logs and exits.
pub fn spawn_watcher(shared: SharedFocus) {
    if let Err(e) = thread::Builder::new()
        .name("hypr-focus".to_string())
        .spawn(move || {
            let Some(dir) = instance_dir() else {
                debug!("focus watcher: no Hyprland sockets; per-app profiles stay inactive");
                return;
            };
            info!("focus watcher: following {}", dir.display());
            if let Some(class) = initial_class(&dir) {
                info!("focus watcher: active window class {class:?}");
                store(&shared, Some(&class));
            }
            loop {
                match UnixStream::connect(dir.join(".socket2.sock"))
                    .and_then(|stream| follow(&stream, &shared))
                {
                    Ok(()) => {}
                    Err(e) => info!("focus watcher: event stream ended: {e}")
                }
                store(&shared, None);
                thread::sleep(RECONNECT_DELAY);
            }
        })
    {
        debug!("focus watcher: cannot spawn thread: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_class_extracts_the_class_field() {
        assert_eq!(
            event_class("activewindow>>firefox,Some Page"),
            Some("firefox")
        );
    }

    #[test]
    fn event_class_keeps_commas_in_the_title() {
        assert_eq!(
            event_class("activewindow>>org.wezfurlong.wezterm[a],b, c"),
            Some("org.wezfurlong.wezterm[a]")
        );
    }

    #[test]
    fn event_class_rejects_other_events_and_empty_classes() {
        assert_eq!(event_class("workspace>>2,web"), None);
        assert_eq!(event_class("activewindow>>"), None);
        assert_eq!(event_class("activewindow>>,title"), None);
    }

    #[test]
    fn text_class_reads_the_class_line() {
        let reply = "Window 0x1 -> [1/2] Dota 2:\n\tmapped: 1\n\tclass: dota2\n\ttitle: Dota 2\n";
        assert_eq!(text_class(reply), Some("dota2"));
    }

    #[test]
    fn text_class_rejects_replies_without_a_class() {
        assert_eq!(text_class("Window 0x1 -> [1/2] x:\n"), None);
        assert_eq!(text_class("\tclass:\n"), None);
        assert_eq!(text_class(""), None);
    }

    #[test]
    fn store_roundtrips_the_shared_slot() {
        let shared = new_shared();
        store(&shared, Some("firefox"));
        assert_eq!(
            shared
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .as_deref(),
            Some("firefox")
        );
        store(&shared, None);
        assert!(
            shared
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .is_none()
        );
    }

    #[test]
    fn env_dir_requires_the_signature_to_exist() {
        assert!(env_dir("/nonexistent-logi-mx").is_none());
    }

    #[test]
    fn discover_dir_picks_the_instance_with_a_live_socket() {
        let root = std::env::temp_dir().join(format!("logi-mx-focus-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let dead = root.join("hypr").join("dead");
        let live = root.join("hypr").join("live");
        std::fs::create_dir_all(&dead).unwrap();
        std::fs::create_dir_all(&live).unwrap();
        std::fs::write(live.join(".socket2.sock"), b"").unwrap();
        assert_eq!(discover_dir(&root.display().to_string()), Some(live));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn discover_dir_is_none_without_the_hypr_root() {
        assert!(discover_dir("/nonexistent-logi-mx").is_none());
    }
}
