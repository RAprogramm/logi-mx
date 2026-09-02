// SPDX-FileCopyrightText: 2025 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! IPC protocol between daemon, CLI and UI.
//!
//! The daemon exclusively owns the HID++ transport. CLI and UI act as
//! clients that send [`Request`] over a Unix socket and receive
//! [`Response`]. When the daemon is not running, clients fall back to
//! direct HID access.

use std::{path::PathBuf, time::Duration};

use logi_mx_driver::prelude::{BatteryInfo, HiResScrollConfig, ReprogControl, SmartShiftConfig};
use serde::{Deserialize, Serialize};

/// Socket filename inside the runtime directory.
const SOCKET_NAME: &str = "logi-mx.sock";

/// Returns the Unix socket path used for daemon IPC.
///
/// Mirrors [`crate::lock_file_path`] directory resolution.
#[must_use]
pub fn socket_path() -> PathBuf {
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR")
        .or_else(|_| std::env::var("TMPDIR"))
        .unwrap_or_else(|_| std::env::temp_dir().to_string_lossy().into_owned());
    PathBuf::from(runtime_dir).join(SOCKET_NAME)
}

/// Client → daemon request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Request {
    /// Liveness probe.
    Ping,
    /// Device name, DPI, `SmartShift` and hi-res state.
    Info,
    /// Battery level and status.
    Battery,
    /// Easy-Switch host info.
    Hosts,
    /// Reprogrammable controls.
    Buttons,
    /// Current DPI value.
    Dpi,
    /// Current `SmartShift` configuration.
    SmartShift,
    /// Current hi-res scroll configuration.
    HiresScroll,
    /// Set DPI.
    SetDpi {
        /// Target DPI.
        value: u16
    },
    /// Set `SmartShift`.
    SetSmartShift {
        /// Enable flag.
        enabled:   bool,
        /// Threshold.
        threshold: u8
    },
    /// Set hi-res scroll.
    SetHires {
        /// Enable flag.
        enabled:  bool,
        /// Inverted flag.
        inverted: bool
    }
}

/// Daemon → client response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code, reason = "IPC response used after server wiring")]
pub enum Response {
    /// Liveness probe reply.
    Pong,
    /// `Info` reply.
    Info {
        /// Marketing name.
        name:       String,
        /// Current DPI.
        dpi:        u16,
        /// `SmartShift` config.
        smartshift: SmartShiftConfig,
        /// Hi-res config.
        hires:      HiResScrollConfig
    },
    /// `Battery` reply.
    Battery {
        /// Remaining charge.
        level:  u8,
        /// Status string.
        status: String
    },
    /// `Hosts` reply.
    Hosts {
        /// Supported host count.
        hosts:   u8,
        /// Current host index.
        current: u8
    },
    /// `Buttons` reply.
    Buttons {
        /// All controls.
        controls: Vec<ReprogControl>
    },
    /// `Dpi` reply.
    Dpi {
        /// Current value.
        value: u16
    },
    /// `SmartShift` reply.
    SmartShift {
        /// Current config.
        config: SmartShiftConfig
    },
    /// `HiresScroll` reply.
    HiresScroll {
        /// Current config.
        config: HiResScrollConfig
    },
    /// `Battery` with typed info.
    BatteryInfo(BatteryInfo),
    /// Generic success for setters.
    Ok,
    /// Human-readable error.
    Error {
        /// Error message.
        message: String
    }
}

/// Timeout for a single IPC round-trip.
#[allow(dead_code, reason = "used after server wiring")]
pub const IPC_TIMEOUT: Duration = Duration::from_secs(2);

/// Attempts a single IPC request against the daemon.
///
/// Returns `None` when the daemon is not running or the socket is
/// unreachable — callers should fall back to direct HID access.
#[allow(dead_code, reason = "IPC client used after CLI migration")]
pub fn try_request(request: &Request) -> Option<Response> {
    use std::{
        io::{BufRead as _, BufReader, Write as _},
        os::unix::net::UnixStream
    };

    let path = socket_path();
    let mut stream = UnixStream::connect(&path).ok()?;
    stream.set_read_timeout(Some(IPC_TIMEOUT)).ok()?;
    stream.set_write_timeout(Some(IPC_TIMEOUT)).ok()?;

    let mut payload = serde_json::to_vec(request).ok()?;
    payload.push(b'\n');
    stream.write_all(&payload).ok()?;
    stream.flush().ok()?;

    let mut reader = BufReader::new(&stream);
    let mut line = String::new();
    reader.read_line(&mut line).ok()?;
    if line.is_empty() {
        return None;
    }
    serde_json::from_str(line.trim()).ok()
}
