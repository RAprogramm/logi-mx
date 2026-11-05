// SPDX-FileCopyrightText: 2025-2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! The async IPC server: binds or inherits the socket, spawns a task per
//! client, forwards RPC requests to the worker and streams live and pie
//! frames to subscribers.

use tokio::net::UnixListener;
use tracing::{error, info};

use super::{
    IPC_TIMEOUT,
    live::LiveHub,
    protocol::{Request, Response},
    rpc::{RpcRequest, RpcSender},
    socket::{inherited_listener_fd, socket_path}
};
use crate::pie::{PieEvent, PieHub};

/// Runs the async IPC server, forwarding requests to the worker.
///
/// With systemd socket activation the listening socket is inherited from
/// `LISTEN_FDS`; otherwise the socket file is created with `0o600` and stale
/// files are removed. A task is spawned per client. Each request is
/// forwarded via `rpc_tx` and the worker's reply is written back as a
/// single JSON line. `WatchLive` connections instead stream frames from
/// `live`, `WatchPie` connections stream events from `pie`.
///
/// # Errors
///
/// Returns an internal error if the socket cannot be created or an
/// accept fails.
pub async fn run_server(
    rpc_tx: RpcSender,
    live: LiveHub,
    pie: PieHub
) -> Result<(), masterror::AppError> {
    use std::os::unix::{fs::PermissionsExt as _, io::FromRawFd as _};

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
        let pie = pie.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_client(stream, rpc_tx, live, pie).await {
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

/// Streams pie menu events to a `WatchPie` client until either side closes.
///
/// A background reader detects client disconnects even while the device is
/// idle: any read on the socket (EOF or error) ends the stream.
async fn stream_pie(
    stream: tokio::net::UnixStream,
    hub: PieHub
) -> Result<(), masterror::AppError> {
    use tokio::{io::AsyncReadExt as _, sync::oneshot};
    use tracing::debug;

    let (mut reader, mut writer) = stream.into_split();
    let mut events = hub.subscribe();
    debug!(subscribers = hub.receiver_count(), "pie: watcher connected");

    write_line(&mut writer, &Response::PieReady)
        .await
        .map_err(|e| masterror::AppError::internal("Failed to ack pie stream").with_source(e))?;

    if let Some(state) = hub.replay() {
        write_line(&mut writer, &Response::Pie(state))
            .await
            .map_err(|e| {
                masterror::AppError::internal("Failed to replay pie state").with_source(e)
            })?;
    }

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
            received = events.recv() => match received {
                Ok(event) => {
                    if write_line(&mut writer, &Response::Pie(event)).await.is_err() {
                        break;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                    let resync = match hub.replay() {
                        Some(state) => write_line(&mut writer, &Response::Pie(state)).await,
                        None => {
                            write_line(&mut writer, &Response::Pie(PieEvent::Close {
                                committed: None,
                            }))
                            .await
                        }
                    };
                    if resync.is_err() {
                        break;
                    }
                }
            }
        }
    }
    debug!("pie: watcher gone");
    Ok(())
}

/// Reads one request line and either dispatches it to the worker or hands
/// the connection to a streaming handler.
async fn handle_client(
    mut stream: tokio::net::UnixStream,
    rpc_tx: RpcSender,
    hub: LiveHub,
    pie: PieHub
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

    if matches!(request, Request::WatchPie) {
        drop(reader);
        return stream_pie(stream, pie).await;
    }

    let (tx, rx) = oneshot::channel();
    let rpc = RpcRequest {
        request,
        responder: tx
    };

    rpc_tx.dispatch(rpc)?;

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
