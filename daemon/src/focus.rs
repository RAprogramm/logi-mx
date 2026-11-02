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

/// Resolves the Hyprland socket directory from the environment.
fn instance_dir() -> Option<PathBuf> {
    let signature = std::env::var("HYPRLAND_INSTANCE_SIGNATURE").ok()?;
    let runtime = std::env::var("XDG_RUNTIME_DIR").ok()?;
    let dir = PathBuf::from(runtime).join("hypr").join(signature);
    dir.is_dir().then_some(dir)
}

/// Extracts the `class` field from an `activewindow` JSON reply.
fn extract_class(json: &str) -> Option<&str> {
    let rest = json.split("\"class\":\"").nth(1)?;
    let end = rest.find('"')?;
    let class = &rest[..end];
    (!class.is_empty()).then_some(class)
}

/// Parses one event-socket line, returning the class of an `activewindow`
/// event.
fn event_class(line: &str) -> Option<&str> {
    let payload = line.strip_prefix("activewindow>>")?;
    payload.split(',').next().filter(|class| !class.is_empty())
}

/// Queries the currently active window class through the request socket.
fn initial_class(dir: &Path) -> Option<String> {
    let mut stream = UnixStream::connect(dir.join(".socket1.sock")).ok()?;
    stream.set_read_timeout(Some(QUERY_TIMEOUT)).ok()?;
    stream.write_all(b"activewindow").ok()?;
    stream.flush().ok()?;
    let mut reply = String::new();
    stream.read_to_string(&mut reply).ok()?;
    extract_class(&reply).map(str::to_owned)
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
    fn extract_class_reads_the_json_field() {
        let json = r#"{"address":"0x1","class":"dota2","title":"Dota 2"}"#;
        assert_eq!(extract_class(json), Some("dota2"));
    }

    #[test]
    fn extract_class_rejects_malformed_replies() {
        assert_eq!(extract_class("{}"), None);
        assert_eq!(extract_class(r#"{"class":""}"#), None);
        assert_eq!(extract_class(r#"{"class":"unterminated"#), None);
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
}
