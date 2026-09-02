// SPDX-FileCopyrightText: 2025 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! IPC protocol between daemon, CLI and UI.
//!
//! The daemon exclusively owns the HID++ transport. CLI and UI act as
//! clients that send [`Request`] over a Unix socket and receive
//! [`Response`]. When the daemon is not running, clients fall back to
//! direct HID access.

use std::{path::PathBuf, sync::mpsc, time::Duration};

use logi_mx_driver::prelude::{BatteryInfo, HiResScrollConfig, ReprogControl, SmartShiftConfig};
use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;

/// RPC envelope forwarded to the worker thread.
pub struct RpcRequest {
    /// Client request.
    pub request:   Request,
    /// Channel to send the response.
    pub responder: oneshot::Sender<Response>
}

/// Channel for RPC requests.
pub type RpcSender = mpsc::Sender<RpcRequest>;

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
#[must_use]
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

/// Runs the async IPC server, forwarding requests to the worker.
///
/// Creates the socket with `0o600`, removes stale files, and spawns a
/// task per client. Each request is forwarded via `rpc_tx` and the
/// worker's reply is written back as a single JSON line.
///
/// # Errors
///
/// Returns an internal error if the socket cannot be created or an
/// accept fails.
pub async fn run_server(rpc_tx: RpcSender) -> Result<(), masterror::AppError> {
    use std::os::unix::fs::PermissionsExt as _;

    use tokio::net::UnixListener;
    use tracing::{error, info};

    let path = socket_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            masterror::AppError::internal("Failed to create IPC directory").with_source(e)
        })?;
    }
    let _ = std::fs::remove_file(&path);

    let listener = UnixListener::bind(&path)
        .map_err(|e| masterror::AppError::internal("Failed to bind IPC socket").with_source(e))?;

    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).map_err(|e| {
        masterror::AppError::internal("Failed to set IPC socket permissions").with_source(e)
    })?;

    info!("IPC server listening on {}", path.display());

    loop {
        let (stream, _) = listener.accept().await.map_err(|e| {
            masterror::AppError::internal("Failed to accept IPC connection").with_source(e)
        })?;

        let rpc_tx = rpc_tx.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_client(stream, rpc_tx).await {
                error!("IPC client error: {e}");
            }
        });
    }
}

async fn handle_client(
    mut stream: tokio::net::UnixStream,
    rpc_tx: RpcSender
) -> Result<(), masterror::AppError> {
    use tokio::{
        io::{AsyncBufReadExt as _, AsyncWriteExt as _, BufReader},
        sync::oneshot,
        time::timeout
    };

    let mut reader = BufReader::new(&mut stream);
    let mut line = String::new();
    let n = reader
        .read_line(&mut line)
        .await
        .map_err(|e| masterror::AppError::internal("Failed to read IPC request").with_source(e))?;

    if n == 0 {
        return Ok(());
    }

    let request: Request = serde_json::from_str(line.trim())
        .map_err(|e| masterror::AppError::bad_request("Invalid IPC request").with_source(e))?;

    let (tx, rx) = oneshot::channel();
    let rpc = RpcRequest {
        request,
        responder: tx
    };

    rpc_tx
        .send(rpc)
        .map_err(|_| masterror::AppError::internal("IPC worker channel closed"))?;

    let response = timeout(IPC_TIMEOUT, rx)
        .await
        .map_err(|_| masterror::AppError::internal("IPC request timed out"))?
        .map_err(|_| masterror::AppError::internal("IPC worker dropped response"))?;

    let mut payload = serde_json::to_vec(&response).map_err(|e| {
        masterror::AppError::internal("Failed to serialize IPC response").with_source(e)
    })?;
    payload.push(b'\n');

    let writer = &mut stream;
    writer.write_all(&payload).await.map_err(|e| {
        masterror::AppError::internal("Failed to write IPC response").with_source(e)
    })?;
    writer.flush().await.map_err(|e| {
        masterror::AppError::internal("Failed to flush IPC response").with_source(e)
    })?;

    Ok(())
}
