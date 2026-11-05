// SPDX-FileCopyrightText: 2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Daemon pie-stream client: a background pump with reconnect backoff.
//!
//! The thread owns the [`WatchPie`] socket, decodes newline-delimited
//! [`ipc::Response::Pie`] lines into [`PieEvent`]s and hands them to the GTK
//! main context through a queue plus a wake callback, mirroring the HUD live
//! pump. A missing or restarting daemon is not an error: the thread retries
//! with capped exponential backoff and the overlay simply draws nothing.
//!
//! [`WatchPie`]: logi_mx_daemon::ipc::Request::WatchPie

use std::{
    io::{BufRead as _, BufReader},
    os::unix::net::UnixStream,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering}
    },
    thread,
    time::Duration
};

use logi_mx_daemon::{ipc, pie::PieEvent};

/// First delay before a reconnect attempt.
pub const RECONNECT_MIN: Duration = Duration::from_millis(500);
/// Upper bound of the reconnect delay.
pub const RECONNECT_MAX: Duration = Duration::from_secs(15);
/// Read timeout bounding how long a dropped socket is noticed.
const POLL_TIMEOUT: Duration = Duration::from_millis(500);

/// Callback that wakes the GTK main context after queueing events.
pub type Wake = Arc<dyn Fn() + Send + Sync>;

/// Decodes one stream line into a pie event, skipping anything else.
///
/// Malformed JSON and non-pie responses (handshake leftovers, protocol
/// changes) are ignored rather than fatal, so a broken line never takes the
/// renderer down.
///
/// # Arguments
///
/// * `line` - One newline-delimited JSON line from the daemon.
///
/// # Returns
///
/// The decoded [`PieEvent`], or `None` for anything that is not one.
///
/// # Examples
///
/// ```
/// use logi_mx_overlay::stream::decode_line;
///
/// assert!(decode_line(r#"{"Pie":{"Close":{"committed":null}}}"#).is_some());
/// assert!(decode_line("not json").is_none());
/// assert!(decode_line("\"Pong\"").is_none());
/// ```
#[must_use]
pub fn decode_line(line: &str) -> Option<PieEvent> {
    serde_json::from_str::<ipc::Response>(line.trim())
        .ok()
        .and_then(|response| match response {
            ipc::Response::Pie(event) => Some(event),
            _ => None
        })
}

/// Spawns the background pump thread.
///
/// The thread never exits: it connects, pumps until the socket drops,
/// sleeps with backoff and reconnects, so daemon restarts are transparent.
///
/// # Arguments
///
/// * `queue` - Shared event queue the main thread drains on wake.
/// * `connected` - Flag the thread sets while a stream is alive.
/// * `wake` - Called after every queue change and connection flip.
///
/// # Examples
///
/// ```no_run
/// use std::sync::{Arc, Mutex, atomic::AtomicBool};
///
/// use logi_mx_overlay::stream::{Wake, spawn};
///
/// let wake: Wake = Arc::new(|| {});
/// spawn(
///     Arc::new(Mutex::new(Vec::new())),
///     Arc::new(AtomicBool::new(false)),
///     wake
/// );
/// ```
pub fn spawn(queue: Arc<Mutex<Vec<PieEvent>>>, connected: Arc<AtomicBool>, wake: Wake) {
    if let Err(e) = thread::Builder::new()
        .name("overlay-pie".to_string())
        .spawn(move || run(&queue, &connected, &wake))
    {
        eprintln!("overlay: failed to spawn the pie stream thread: {e}");
    }
}

/// Connect-pump-retry loop, run on the background thread.
fn run(queue: &Arc<Mutex<Vec<PieEvent>>>, connected: &Arc<AtomicBool>, wake: &Wake) {
    let mut delay = RECONNECT_MIN;
    let mut waiting = false;
    loop {
        if let Ok(socket) = ipc::open_pie_stream() {
            if waiting {
                eprintln!("overlay: pie stream reconnected");
            }
            waiting = false;
            delay = RECONNECT_MIN;
            connected.store(true, Ordering::Relaxed);
            wake();
            pump(socket, queue, wake);
            connected.store(false, Ordering::Relaxed);
            wake();
        } else {
            if !waiting {
                eprintln!("overlay: daemon pie stream unavailable, retrying");
                waiting = true;
            }
            connected.store(false, Ordering::Relaxed);
            thread::sleep(delay);
            delay = delay.mul_f32(2.0).min(RECONNECT_MAX);
        }
    }
}

/// Reads lines from one live socket into the queue until it drops.
fn pump(mut reader: BufReader<UnixStream>, queue: &Arc<Mutex<Vec<PieEvent>>>, wake: &Wake) {
    let _ = reader.get_ref().set_read_timeout(Some(POLL_TIMEOUT));
    loop {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {
                if let Some(event) = decode_line(&line) {
                    if let Ok(mut guard) = queue.lock() {
                        guard.push(event);
                    }
                    wake();
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(_) => break
        }
    }
}

#[cfg(test)]
#[path = "stream_tests.rs"]
mod stream_tests;
