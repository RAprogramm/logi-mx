// SPDX-FileCopyrightText: 2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Hyprland focus watcher feeding the per-application gesture profiles.
//!
//! Hyprland pushes every focus change over its event socket as
//! `activewindow>>class,title`; the watcher thread follows that stream and
//! keeps the focused window class in a shared slot the gesture runtime reads
//! when resolving bindings. The active window is queried through the request
//! socket on start and after every reconnect, so profiles apply without
//! waiting for the first switch.
//! Outside Hyprland the watcher keeps retrying discovery and the slot stays
//! `None`, which means "no app focused" and keeps the global bindings in
//! charge.

use std::{
    io::{BufRead, BufReader, Read, Write},
    os::unix::{fs::FileTypeExt as _, net::UnixStream},
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
        .filter(|entry| socket_present(&entry.path().join(".socket2.sock")))
        .max_by_key(|entry| entry.metadata().ok().and_then(|m| m.modified().ok()))
        .map(|entry| entry.path())
}

/// Whether `path` is a live socket (or a regular file standing in for one in
/// tests); `Path::is_file` rejects Unix sockets, so the metadata file type is
/// checked explicitly.
fn socket_present(path: &Path) -> bool {
    path.metadata()
        .is_ok_and(|meta| meta.file_type().is_socket() || meta.file_type().is_file())
}

/// Extracts the `class:` field from an `activewindow` text reply.
fn text_class(reply: &str) -> Option<&str> {
    reply
        .lines()
        .find_map(|line| line.trim().strip_prefix("class:"))
        .map(str::trim)
        .filter(|class| !class.is_empty())
}

/// The focus change reported by one `activewindow` event-socket line.
#[derive(Debug, PartialEq)]
enum FocusEvent<'a> {
    /// The event names the class of the newly focused window.
    Focused(&'a str),
    /// The event reports no class, so focus left every window.
    Unfocused
}

/// Parses one event-socket line into the focus change of an `activewindow`
/// event, or `None` for every other event.
fn event_focus(line: &str) -> Option<FocusEvent<'_>> {
    let payload = line.strip_prefix("activewindow>>")?;
    Some(
        payload
            .split(',')
            .next()
            .filter(|class| !class.is_empty())
            .map_or(FocusEvent::Unfocused, FocusEvent::Focused)
    )
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
        if let Some(event) = event_focus(trimmed) {
            match event {
                FocusEvent::Focused(class) => store(shared, Some(class)),
                FocusEvent::Unfocused => store(shared, None)
            }
        }
    }
}

/// Spawns the watcher thread; outside Hyprland it retries discovery until the
/// compositor's sockets appear, and re-queries the active window after every
/// event-stream reconnect.
pub fn spawn_watcher(shared: SharedFocus) {
    if let Err(e) = thread::Builder::new()
        .name("hypr-focus".to_string())
        .spawn(move || {
            loop {
                let Some(dir) = instance_dir() else {
                    debug!("focus watcher: no Hyprland sockets; per-app profiles stay inactive");
                    store(&shared, None);
                    thread::sleep(RECONNECT_DELAY);
                    continue;
                };
                info!("focus watcher: following {}", dir.display());
                match initial_class(&dir) {
                    Some(class) => {
                        info!("focus watcher: active window class {class:?}");
                        store(&shared, Some(&class));
                    }
                    None => store(&shared, None)
                }
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
    fn event_focus_extracts_the_class_field() {
        assert_eq!(
            event_focus("activewindow>>firefox,Some Page"),
            Some(FocusEvent::Focused("firefox"))
        );
    }

    #[test]
    fn event_focus_keeps_commas_in_the_title() {
        assert_eq!(
            event_focus("activewindow>>org.wezfurlong.wezterm[a],b, c"),
            Some(FocusEvent::Focused("org.wezfurlong.wezterm[a]"))
        );
    }

    #[test]
    fn event_focus_clears_on_empty_class_and_ignores_other_events() {
        assert_eq!(event_focus("workspace>>2,web"), None);
        assert_eq!(event_focus("activewindow>>"), Some(FocusEvent::Unfocused));
        assert_eq!(
            event_focus("activewindow>>,title"),
            Some(FocusEvent::Unfocused)
        );
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
    fn socket_present_accepts_unix_sockets_and_regular_files() {
        let dir = std::env::temp_dir().join(format!("logi-mx-focus-sock-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let sock = dir.join(".socket2.sock");
        let listener = std::os::unix::net::UnixListener::bind(&sock).unwrap();
        assert!(socket_present(&sock));
        let regular = dir.join("plain");
        std::fs::write(&regular, b"").unwrap();
        assert!(socket_present(&regular));
        assert!(!socket_present(&dir.join("missing")));
        drop(listener);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn discover_dir_is_none_without_the_hypr_root() {
        assert!(discover_dir("/nonexistent-logi-mx").is_none());
    }
}
