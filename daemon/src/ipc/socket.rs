// SPDX-FileCopyrightText: 2025-2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Socket location: where the daemon listens and how a systemd-activated
//! daemon discovers its inherited listening socket.

use std::path::PathBuf;

/// Socket filename inside the runtime directory.
const SOCKET_NAME: &str = "logi-mx.sock";

/// Returns the Unix socket path used for daemon IPC.
///
/// Resolves the directory the same way the daemon's instance lock does:
/// `XDG_RUNTIME_DIR`, falling back to `TMPDIR` and then the system temp
/// directory.
#[must_use]
pub fn socket_path() -> PathBuf {
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR")
        .or_else(|_| std::env::var("TMPDIR"))
        .unwrap_or_else(|_| std::env::temp_dir().to_string_lossy().into_owned());
    PathBuf::from(runtime_dir).join(SOCKET_NAME)
}

/// First file descriptor passed by systemd socket activation.
const SD_LISTEN_FDS_START: i32 = 3;

/// Reports whether systemd passed an inherited listening socket to this
/// process (`LISTEN_PID` matches and `LISTEN_FDS` is at least one).
#[must_use]
pub fn is_socket_activated() -> bool {
    inherited_listener_fd().is_some()
}

/// Returns the inherited listening file descriptor when systemd socket
/// activation applies to this process.
pub(super) fn inherited_listener_fd() -> Option<i32> {
    let listen_pid: u32 = std::env::var("LISTEN_PID").ok()?.parse().ok()?;
    let listen_fds: i32 = std::env::var("LISTEN_FDS").ok()?.parse().ok()?;
    (listen_pid == std::process::id() && listen_fds >= 1).then_some(SD_LISTEN_FDS_START)
}
