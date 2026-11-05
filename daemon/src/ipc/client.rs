// SPDX-FileCopyrightText: 2025-2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Client-side helpers: a single-shot request and the two streaming
//! handshakes, used by the CLI, the HUD and the overlay.

use std::time::Duration;

use super::{Request, Response, socket_path};

/// Timeout for a single IPC round-trip.
pub const IPC_TIMEOUT: Duration = Duration::from_secs(2);

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

/// Opens a radial pie menu event stream against the daemon.
///
/// Sends [`Request::WatchPie`] and returns the connected socket positioned
/// after the handshake: every subsequent newline-delimited JSON line is a
/// [`Response::Pie`] event until the daemon or the client closes it.
///
/// # Errors
///
/// Returns an I/O error when the daemon is unreachable or rejects the
/// handshake.
///
/// # Examples
///
/// ```no_run
/// let socket = logi_mx_daemon::ipc::open_pie_stream()?;
/// drop(socket);
/// # Ok::<(), std::io::Error>(())
/// ```
#[allow(
    dead_code,
    reason = "client helper; the logi-mx-overlay crate consumes the pie stream"
)]
pub fn open_pie_stream() -> std::io::Result<std::io::BufReader<std::os::unix::net::UnixStream>> {
    use std::{
        io::{BufRead as _, BufReader, Write as _},
        os::unix::net::UnixStream
    };

    let mut stream = UnixStream::connect(socket_path())?;
    stream.set_read_timeout(Some(IPC_TIMEOUT))?;
    stream.set_write_timeout(Some(IPC_TIMEOUT))?;

    let mut payload = serde_json::to_vec(&Request::WatchPie)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    payload.push(b'\n');
    stream.write_all(&payload)?;
    stream.flush()?;

    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    match serde_json::from_str::<Response>(line.trim()) {
        Ok(Response::PieReady) => Ok(reader),
        _ => Err(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "daemon refused pie stream"
        ))
    }
}
