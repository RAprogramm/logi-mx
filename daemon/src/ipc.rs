// SPDX-FileCopyrightText: 2025-2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! IPC protocol between daemon, CLI and UI.
//!
//! The daemon exclusively owns the HID++ transport. CLI and UI act as
//! clients that send [`Request`] over a Unix socket and receive
//! [`Response`]. When the daemon is not running, clients fall back to
//! direct HID access.

use std::{path::PathBuf, sync::mpsc, time::Duration};

use logi_mx_driver::prelude::{
    Action, ButtonId, Gesture, GestureDirection, GestureMode, HiResScrollConfig, ReprogControl,
    SmartShiftConfig
};
use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;

use crate::hyprbind::KeybindEntry;

/// RPC envelope forwarded to the worker thread.
pub struct RpcRequest {
    /// Client request.
    pub request:   Request,
    /// Channel to send the response.
    pub responder: oneshot::Sender<Response>
}

/// Channel for RPC requests.
pub type RpcSender = mpsc::Sender<RpcRequest>;

/// Fan-out channel carrying live input telemetry to subscribed clients.
pub type LiveHub = tokio::sync::broadcast::Sender<LiveFrame>;

/// Button state the daemon injects into live telemetry.
///
/// Diverted HID++ buttons (gesture, wheel mode) never reach the kernel, so
/// the worker thread raises their bits here and the evdev tap merges them
/// into every frame it publishes. `last_buttons` records the raw kernel
/// button mask of the newest frame so an immediate daemon-published frame
/// never drops physically held buttons.
#[derive(Debug, Default)]
pub struct LiveState {
    /// Bits for daemon-side (diverted) pressed buttons.
    pub virtual_bits: std::sync::atomic::AtomicU32,
    /// Kernel button mask of the most recently published frame.
    pub last_buttons: std::sync::atomic::AtomicU32
}

/// Shared handle to the daemon-injected button state.
pub type SharedLiveState = std::sync::Arc<LiveState>;

/// One coalesced live input snapshot pushed to `WatchLive` clients.
///
/// Motion and wheel values are deltas accumulated since the previous frame;
/// button bits are the absolute pressed state.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LiveFrame {
    /// Bitmask of pressed buttons, see `BTN_BIT_*` constants.
    pub buttons:    u32,
    /// Horizontal movement since the previous frame, in device units.
    pub dx:         i32,
    /// Vertical movement since the previous frame, in device units.
    pub dy:         i32,
    /// Milliseconds covered by this frame.
    pub dt_ms:      u32,
    /// Vertical wheel notches since the previous frame.
    pub wheel:      i32,
    /// High-resolution vertical wheel units since the previous frame
    /// (120 units per notch).
    pub wheel_hi:   i32,
    /// Horizontal (tilt) wheel notches since the previous frame.
    pub hwheel:     i32,
    /// High-resolution horizontal wheel units since the previous frame.
    pub hwheel_hi:  i32,
    /// Pressed key codes without a named button mapping, ascending.
    pub extra_keys: Vec<u16>
}

/// Bit for the primary left button in [`LiveFrame::buttons`].
pub const BTN_BIT_LEFT: u32 = 1 << 0;
/// Bit for the primary right button in [`LiveFrame::buttons`].
pub const BTN_BIT_RIGHT: u32 = 1 << 1;
/// Bit for the wheel click in [`LiveFrame::buttons`].
pub const BTN_BIT_MIDDLE: u32 = 1 << 2;
/// Bit for the front thumb button in [`LiveFrame::buttons`].
pub const BTN_BIT_BACK: u32 = 1 << 3;
/// Bit for the rear thumb button in [`LiveFrame::buttons`].
pub const BTN_BIT_FORWARD: u32 = 1 << 4;
/// Bit for the gesture button in [`LiveFrame::buttons`].
pub const BTN_BIT_GESTURE: u32 = 1 << 5;
/// Bit for the wheel-mode (DPI shift) button in [`LiveFrame::buttons`].
///
/// The button is a pure HID++ control: the daemon diverts it and raises this
/// bit itself, the kernel never reports it.
pub const BTN_BIT_DPI: u32 = 1 << 6;

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
    },
    /// Divert a programmable control to HID++ notifications.
    SetButtonDivert {
        /// HID++ control ID.
        control_id: u16,
        /// Whether to divert the control.
        divert:     bool,
        /// Whether to also stream raw XY deltas.
        raw_xy:     bool
    },
    /// Remap a programmable control to another native control on the device.
    SetButtonRemap {
        /// HID++ control ID to remap.
        control_id: u16,
        /// Target control ID, or `0x0000` to restore native behaviour.
        target_cid: u16
    },
    /// Switch to an Easy-Switch host.
    SetHost {
        /// Zero-based host index.
        host: u8
    },
    /// Read the gesture configuration: opt-in flag and bindings.
    GetGestures,
    /// Enable or disable software gesture recognition; persists to config.
    SetGesturesEnabled {
        /// Whether the daemon should own the gesture button.
        enabled: bool
    },
    /// Bind an action to a gesture direction; persists to config.
    SetGesture {
        /// Direction to bind.
        direction: GestureDirection,
        /// When the action fires.
        mode:      GestureMode,
        /// Action to run: a validated `KEY_*` sequence or a shell command.
        action:    Box<Action>
    },
    /// Remove the binding for a gesture direction; persists to config.
    ClearGesture {
        /// Direction to unbind.
        direction: GestureDirection
    },
    /// Bind an action inside a per-application profile; persists to config.
    SetAppGesture {
        /// Hyprland window class the profile applies to.
        app_id:    String,
        /// Direction to bind.
        direction: GestureDirection,
        /// When the action fires.
        mode:      GestureMode,
        /// Action to run: a validated `KEY_*` sequence or a shell command.
        action:    Box<Action>
    },
    /// Remove one direction from a per-application profile; persists.
    ClearAppGesture {
        /// Hyprland window class.
        app_id:    String,
        /// Direction to unbind.
        direction: GestureDirection
    },
    /// Delete a per-application profile entirely; persists.
    RemoveAppProfile {
        /// Hyprland window class.
        app_id: String
    },
    /// Read the software button remaps: global map and per-app profiles.
    GetButtons,
    /// Bind an action to a thumb side button globally; persists to config.
    SetButton {
        /// `ButtonId::Back` or `ButtonId::Forward`.
        button: ButtonId,
        /// Action to run: a validated `KEY_*` sequence or a shell command.
        action: Box<Action>
    },
    /// Restore a thumb side button to its native behaviour globally; persists.
    ClearButton {
        /// `ButtonId::Back` or `ButtonId::Forward`.
        button: ButtonId
    },
    /// Bind an action to a thumb side button inside a per-application
    /// profile; persists to config.
    SetAppButton {
        /// Hyprland window class the profile applies to.
        app_id: String,
        /// `ButtonId::Back` or `ButtonId::Forward`.
        button: ButtonId,
        /// Action to run: a validated `KEY_*` sequence or a shell command.
        action: Box<Action>
    },
    /// Remove one thumb button from a per-application profile; persists.
    ClearAppButton {
        /// Hyprland window class.
        app_id: String,
        /// `ButtonId::Back` or `ButtonId::Forward`.
        button: ButtonId
    },
    /// Delete a per-application button profile entirely; persists.
    RemoveAppButtonProfile {
        /// Hyprland window class.
        app_id: String
    },
    /// Report the currently focused Hyprland window class, if any.
    GetFocusedApp,
    /// List the active Hyprland keyboard bindings that can be replayed as a
    /// key chord; answered without touching the device.
    GetKeybinds,
    /// Subscribe to live input telemetry; the connection becomes a stream of
    /// [`Response::Live`] lines until it is closed.
    WatchLive
}

/// Daemon → client response.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
    /// `GetGestures` reply.
    Gestures {
        /// Whether software gesture recognition is enabled.
        enabled:  bool,
        /// Current global bindings.
        gestures: Vec<Gesture>,
        /// Per-application overrides keyed by window class.
        profiles: std::collections::HashMap<String, Vec<Gesture>>
    },
    /// `GetButtons` reply.
    ButtonsConfig {
        /// Global thumb button remaps, Back and Forward only.
        buttons:  std::collections::HashMap<ButtonId, Action>,
        /// Per-application overrides keyed by window class.
        profiles: std::collections::HashMap<String, std::collections::HashMap<ButtonId, Action>>
    },
    /// The focused window class reported by the desktop, if any.
    FocusedApp {
        /// Hyprland window class of the focused window.
        app_id: Option<String>
    },
    /// `GetKeybinds` reply.
    Keybinds {
        /// Replayable keyboard bindings reported by Hyprland.
        entries: Vec<KeybindEntry>
    },
    /// Generic success for setters.
    Ok,
    /// Acknowledgement that a live telemetry stream was accepted; further
    /// lines on this connection are [`Response::Live`] frames.
    LiveReady,
    /// Live input telemetry frame on a `WatchLive` connection.
    Live(LiveFrame),
    /// Human-readable error.
    Error {
        /// Error message.
        message: String
    }
}

/// Timeout for a single IPC round-trip.
pub const IPC_TIMEOUT: Duration = Duration::from_secs(2);

/// First file descriptor passed by systemd socket activation.
const SD_LISTEN_FDS_START: i32 = 3;

/// Reports whether systemd passed an inherited listening socket to this
/// process (`LISTEN_PID` matches and `LISTEN_FDS` is at least one).
#[must_use]
pub fn is_socket_activated() -> bool {
    inherited_listener_fd().is_some()
}

fn inherited_listener_fd() -> Option<i32> {
    let listen_pid: u32 = std::env::var("LISTEN_PID").ok()?.parse().ok()?;
    let listen_fds: i32 = std::env::var("LISTEN_FDS").ok()?.parse().ok()?;
    (listen_pid == std::process::id() && listen_fds >= 1).then_some(SD_LISTEN_FDS_START)
}

/// Attempts a single IPC request against the daemon.
///
/// Returns `None` when the daemon is not running or the socket is
/// unreachable — callers should fall back to direct HID access.
#[must_use]
#[allow(dead_code, reason = "IPC client used by CLI and UI crates")]
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

/// Opens a live telemetry stream against the daemon.
///
/// Sends [`Request::WatchLive`] and returns the connected socket positioned
/// after the handshake: every subsequent newline-delimited JSON line is a
/// [`Response::Live`] frame until the daemon or the client closes it.
///
/// # Errors
///
/// Returns an I/O error when the daemon is unreachable or rejects the
/// handshake.
#[allow(
    dead_code,
    reason = "client helper; the UI crate starts calling it in the HUD rework (pending task)"
)]
pub fn open_live_stream() -> std::io::Result<std::os::unix::net::UnixStream> {
    use std::{
        io::{BufRead as _, BufReader, Write as _},
        os::unix::net::UnixStream
    };

    let mut stream = UnixStream::connect(socket_path())?;
    stream.set_read_timeout(Some(IPC_TIMEOUT))?;
    stream.set_write_timeout(Some(IPC_TIMEOUT))?;

    let mut payload = serde_json::to_vec(&Request::WatchLive)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    payload.push(b'\n');
    stream.write_all(&payload)?;
    stream.flush()?;

    let mut line = String::new();
    BufReader::new(&stream).read_line(&mut line)?;
    match serde_json::from_str::<Response>(line.trim()) {
        Ok(Response::LiveReady) => Ok(stream),
        _ => Err(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "daemon refused live stream"
        ))
    }
}

/// Runs the async IPC server, forwarding requests to the worker.
///
/// With systemd socket activation the listening socket is inherited from
/// `LISTEN_FDS`; otherwise the socket file is created with `0o600` and stale
/// files are removed. A task is spawned per client. Each request is
/// forwarded via `rpc_tx` and the worker's reply is written back as a
/// single JSON line. `WatchLive` connections instead stream frames from
/// `live`.
///
/// # Errors
///
/// Returns an internal error if the socket cannot be created or an
/// accept fails.
pub async fn run_server(rpc_tx: RpcSender, live: LiveHub) -> Result<(), masterror::AppError> {
    use std::os::unix::{fs::PermissionsExt as _, io::FromRawFd as _};

    use tokio::net::UnixListener;
    use tracing::{error, info};

    let listener = if let Some(fd) = inherited_listener_fd() {
        // SAFETY: the sd_listen_fds protocol guarantees file descriptor 3
        // is an inherited, bound and listening Unix socket owned by this
        // process once `LISTEN_PID` and `LISTEN_FDS` have been validated.
        let std_listener = unsafe { std::os::unix::net::UnixListener::from_raw_fd(fd) };
        std_listener.set_nonblocking(true).map_err(|e| {
            masterror::AppError::internal("Failed to configure socket").with_source(e)
        })?;
        let listener = UnixListener::from_std(std_listener).map_err(|e| {
            masterror::AppError::internal("Failed to adopt IPC socket").with_source(e)
        })?;
        info!("IPC server using systemd socket activation");
        listener
    } else {
        let path = socket_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                masterror::AppError::internal("Failed to create IPC directory").with_source(e)
            })?;
        }
        let _ = std::fs::remove_file(&path);

        let listener = UnixListener::bind(&path).map_err(|e| {
            masterror::AppError::internal("Failed to bind IPC socket").with_source(e)
        })?;

        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).map_err(|e| {
            masterror::AppError::internal("Failed to set IPC socket permissions").with_source(e)
        })?;

        info!("IPC server listening on {}", path.display());
        listener
    };

    loop {
        let (stream, _) = listener.accept().await.map_err(|e| {
            masterror::AppError::internal("Failed to accept IPC connection").with_source(e)
        })?;

        let rpc_tx = rpc_tx.clone();
        let live = live.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_client(stream, rpc_tx, live).await {
                error!("IPC client error: {e}");
            }
        });
    }
}

/// Serializes one response and writes it as a newline-delimited JSON line.
async fn write_line<W: tokio::io::AsyncWriteExt + Unpin>(
    writer: &mut W,
    response: &Response
) -> std::io::Result<()> {
    let mut payload = serde_json::to_vec(response)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    payload.push(b'\n');
    writer.write_all(&payload).await?;
    writer.flush().await
}

/// Streams live telemetry to a `WatchLive` client until either side closes.
///
/// A background reader detects client disconnects even while the device is
/// idle: any read on the socket (EOF or error) ends the stream.
async fn stream_live(
    stream: tokio::net::UnixStream,
    hub: LiveHub
) -> Result<(), masterror::AppError> {
    use tokio::{io::AsyncReadExt as _, sync::oneshot};

    let (mut reader, mut writer) = stream.into_split();
    let mut frames = hub.subscribe();

    write_line(&mut writer, &Response::LiveReady)
        .await
        .map_err(|e| masterror::AppError::internal("Failed to ack live stream").with_source(e))?;

    let (gone_tx, gone_rx) = oneshot::channel();
    tokio::spawn(async move {
        let mut sink = [0u8; 64];
        loop {
            let read = reader.read(&mut sink).await.unwrap_or(0);
            if read == 0 {
                break;
            }
        }
        let _ = gone_tx.send(());
    });

    let mut gone_rx = gone_rx;
    loop {
        tokio::select! {
            _ = &mut gone_rx => break,
            received = frames.recv() => match received {
                Ok(frame) => {
                    if write_line(&mut writer, &Response::Live(frame)).await.is_err() {
                        break;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
            }
        }
    }
    Ok(())
}

async fn handle_client(
    mut stream: tokio::net::UnixStream,
    rpc_tx: RpcSender,
    hub: LiveHub
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

    if matches!(request, Request::WatchLive) {
        drop(reader);
        return stream_live(stream, hub).await;
    }

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
