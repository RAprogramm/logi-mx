// SPDX-FileCopyrightText: 2025-2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! systemd service-state notifications and runtime watchdog.
//!
//! Under `Type=notify` the daemon reports `READY=1` once its services are
//! accepting work and keeps the `WatchdogSec` timer alive with `WATCHDOG=1`
//! pings derived from the device-worker heartbeat. A hung worker stops the
//! pings, so systemd restarts the service instead of trusting a live but
//! useless process. Outside systemd (`NOTIFY_SOCKET` unset) every entry
//! point is a silent no-op, which keeps manual runs and tests unaffected.

use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::Duration
};

use sd_notify::NotifyState;
use tokio::time::{MissedTickBehavior, interval};
use tracing::{debug, warn};

/// Device-worker heartbeat as milliseconds since the Unix epoch; zero until
/// the first beat is recorded.
static WORKER_BEAT_MS: AtomicU64 = AtomicU64::new(0);

/// Records a device-worker heartbeat.
///
/// Called from the worker loop on every iteration; the watchdog task treats
/// a stale heartbeat as a hung worker.
pub fn mark_worker_beat() {
    WORKER_BEAT_MS.store(unix_now_ms(), Ordering::Relaxed);
}

/// Reports whether the last worker heartbeat is fresher than `max_age`.
#[must_use]
pub fn worker_alive(max_age: Duration) -> bool {
    let last = WORKER_BEAT_MS.load(Ordering::Relaxed);
    last != 0 && u128::from(unix_now_ms().saturating_sub(last)) < max_age.as_millis()
}

/// Sends `READY=1`, telling systemd startup finished.
pub fn notify_ready() {
    send(&[NotifyState::Ready]);
}

/// Publishes a human-readable service state line for `systemctl status`.
pub fn notify_status(text: &str) {
    send(&[NotifyState::Status(text)]);
}

/// Runs the watchdog ping loop until the process exits.
///
/// Pings at half the interval systemd advertised through `WATCHDOG_USEC`,
/// and only while the device-worker heartbeat is fresh. Without a watchdog
/// configured the task returns immediately.
pub async fn run_watchdog() {
    let Some(period) = sd_notify::watchdog_enabled() else {
        debug!("systemd watchdog is not enabled");
        return;
    };

    debug!("systemd watchdog enabled, pinging every {:?}", period / 2);
    let mut ticker = interval((period / 2).max(Duration::from_secs(1)));
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);

    loop {
        ticker.tick().await;
        if worker_alive(period) {
            send(&[NotifyState::Watchdog]);
        } else {
            warn!("device worker heartbeat is stale, withholding the watchdog ping");
        }
    }
}

fn send(states: &[NotifyState<'_>]) {
    if let Err(e) = sd_notify::notify(states) {
        debug!("systemd notification failed: {e}");
    }
}

fn unix_now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| {
            u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_beat_keeps_worker_alive() {
        mark_worker_beat();
        assert!(worker_alive(Duration::from_secs(5)));
    }

    #[test]
    fn zero_beat_is_never_alive() {
        WORKER_BEAT_MS.store(0, Ordering::Relaxed);
        assert!(!worker_alive(Duration::from_secs(5)));
    }

    #[test]
    fn stale_beat_marks_worker_dead() {
        let long_ago = unix_now_ms().saturating_sub(60_000);
        WORKER_BEAT_MS.store(long_ago, Ordering::Relaxed);
        assert!(!worker_alive(Duration::from_secs(30)));
    }

    #[test]
    fn notifications_are_noops_without_notify_socket() {
        notify_ready();
        notify_status("test");
    }
}
