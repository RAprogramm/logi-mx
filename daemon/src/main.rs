// SPDX-FileCopyrightText: 2025 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

#[cfg(feature = "tray")]
mod tray;

mod ipc;
mod status;
mod worker;

use std::{
    fs,
    io::Write,
    os::unix::fs::OpenOptionsExt as _,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration
};

use fslock::LockFile;
use logi_mx_driver::prelude::*;
use masterror::prelude::*;
use tokio::{
    select,
    signal::unix::{SignalKind, signal},
    sync::mpsc
};
use tracing::{error, info, warn};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};
use udev::MonitorBuilder;

#[cfg(feature = "tray")]
use crate::tray::spawn_tray;
use crate::{
    status::{DeviceStatus, SharedStatus},
    worker::{DeviceEvent, spawn_device_worker}
};

type Result<T> = std::result::Result<T, AppError>;

fn lock_file_path(suffix: Option<&str>) -> PathBuf {
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR")
        .or_else(|_| std::env::var("TMPDIR"))
        .unwrap_or_else(|_| std::env::temp_dir().to_string_lossy().into_owned());
    let filename = suffix.map_or_else(
        || "logi-mx-daemon.lock".to_string(),
        |s| format!("logi-mx-daemon-{s}.lock")
    );
    PathBuf::from(runtime_dir).join(filename)
}

/// Reports whether the PID read from the lock file belongs to this daemon.
///
/// Checks `/proc/<pid>/exe` and falls back to `/proc/<pid>/cmdline` to avoid
/// signalling an unrelated process that recycled the PID after a crash.
fn pid_belongs_to_daemon(pid: i32) -> bool {
    let exe_path = format!("/proc/{pid}/exe");
    if let Ok(target) = fs::read_link(&exe_path)
        && let Some(name) = target.file_name().and_then(|n| n.to_str())
    {
        return name.contains("logi-mx-daemon");
    }
    let cmdline_path = format!("/proc/{pid}/cmdline");
    if let Ok(bytes) = fs::read(&cmdline_path) {
        return String::from_utf8_lossy(&bytes).contains("logi-mx-daemon");
    }
    false
}

fn acquire_instance_lock(suffix: Option<&str>) -> Result<LockFile> {
    let lock_path = lock_file_path(suffix);

    if let Some(parent) = lock_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| AppError::internal("Failed to create lock directory").with_source(e))?;
    }

    let mut lockfile = LockFile::open(&lock_path)
        .map_err(|e| AppError::internal("Failed to open lock file").with_source(e))?;

    if !lockfile
        .try_lock()
        .map_err(|e| AppError::internal("Failed to acquire lock").with_source(e))?
        && let Ok(pid_str) = fs::read_to_string(&lock_path)
        && let Ok(pid) = pid_str.trim().parse::<i32>()
    {
        if pid == std::process::id().cast_signed() {
            info!("Lock already held by current process, reusing");
        } else if !pid_belongs_to_daemon(pid) {
            warn!(
                "PID {pid} from lock file does not belong to logi-mx-daemon, \
                 treating as stale lock"
            );
        } else {
            info!("Another instance detected (PID {pid}), requesting graceful shutdown");

            // SAFETY: `kill` only sends a signal to the PID that was verified
            // to belong to this daemon via `/proc/<pid>/exe`. The syscall
            // cannot introduce memory unsafety; delivery failure is tolerated
            // because the lock will never be released.
            unsafe {
                libc::kill(pid, libc::SIGTERM);
            }
            info!("Sent SIGTERM to process {pid}, waiting for shutdown");

            for attempt in 1..=10 {
                std::thread::sleep(Duration::from_millis(500));
                match lockfile.try_lock() {
                    Ok(true) => {
                        info!("Previous instance stopped, acquired lock");
                        break;
                    }
                    Ok(false) if attempt == 10 => {
                        return Err(AppError::internal(
                            "Previous instance did not stop within 5 seconds"
                        ));
                    }
                    Ok(false) => {}
                    Err(e) => {
                        return Err(AppError::internal("Failed to acquire lock").with_source(e));
                    }
                }
            }
        }
    }

    let pid_string = std::process::id().to_string();
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true).mode(0o600);
    let mut file = options
        .open(&lock_path)
        .map_err(|e| AppError::internal("Failed to open lock file for PID").with_source(e))?;
    file.write_all(pid_string.as_bytes())
        .map_err(|e| AppError::internal("Failed to write PID to lock file").with_source(e))?;
    file.flush()
        .map_err(|e| AppError::internal("Failed to flush PID file").with_source(e))?;

    Ok(lockfile)
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_default_env())
        .init();

    let _lock = acquire_instance_lock(None)?;
    info!("Acquired instance lock");

    info!("Starting logi-mx-daemon");

    let config = load_config().unwrap_or_else(|e| {
        warn!("Failed to load config: {e}. Using default.");
        Config::default()
    });

    let status: SharedStatus = Arc::new(Mutex::new(DeviceStatus::default()));
    let (device_tx, worker_handle) = spawn_device_worker(config, Arc::clone(&status))?;

    #[cfg(feature = "tray")]
    match spawn_tray(Arc::clone(&status)).await {
        Ok(()) => info!("System tray initialized"),
        Err(e) => warn!("Failed to initialize tray: {e}. Continuing without tray.")
    }

    let (tx, mut rx) = mpsc::channel::<UdevEvent>(32);

    std::thread::spawn(move || {
        if let Err(e) = monitor_udev_events_sync(&tx) {
            error!("Udev monitor error: {e}");
        }
    });

    let mut sigterm = signal(SignalKind::terminate())
        .map_err(|e| AppError::internal("Failed to setup SIGTERM handler").with_source(e))?;
    let mut sigint = signal(SignalKind::interrupt())
        .map_err(|e| AppError::internal("Failed to setup SIGINT handler").with_source(e))?;

    info!("Daemon started successfully");

    loop {
        select! {
            Some(event) = rx.recv() => {
                let forwarded = match event {
                    UdevEvent::Add(path) => device_tx.send(DeviceEvent::Added(path)),
                    UdevEvent::Remove(path) => device_tx.send(DeviceEvent::Removed(path))
                };

                if forwarded.is_err() {
                    error!("Device worker stopped, shutting down");
                    break;
                }
            }
            _ = sigterm.recv() => {
                info!("Received SIGTERM, shutting down...");
                break;
            }
            _ = sigint.recv() => {
                info!("Received SIGINT, shutting down...");
                break;
            }
        }
    }

    drop(device_tx);

    let _ = worker_handle.join();

    info!("Daemon stopped");
    Ok(())
}

#[derive(Debug)]
enum UdevEvent {
    Add(String),
    Remove(String)
}

fn monitor_udev_events_sync(tx: &mpsc::Sender<UdevEvent>) -> Result<()> {
    let monitor = MonitorBuilder::new()
        .map_err(|e| AppError::internal("Failed to create udev monitor").with_source(e))?
        .match_subsystem("hidraw")
        .map_err(|e| AppError::internal("Failed to match subsystem").with_source(e))?
        .listen()
        .map_err(|e| AppError::internal("Failed to start udev monitor").with_source(e))?;

    info!("Monitoring udev events for hidraw devices");

    let iter = monitor.iter();
    for event in iter {
        let device_path = event
            .device()
            .devnode()
            .and_then(|p| p.to_str())
            .map(String::from);

        if let Some(path) = device_path {
            let udev_event = match event.event_type() {
                udev::EventType::Add => Some(UdevEvent::Add(path)),
                udev::EventType::Remove => Some(UdevEvent::Remove(path)),
                _ => None
            };

            if let Some(evt) = udev_event
                && tx.blocking_send(evt).is_err()
            {
                error!("Failed to send udev event");
                break;
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lock_file_path() {
        let path = lock_file_path(None);
        assert!(path.to_str().unwrap().contains("logi-mx-daemon.lock"));

        let path_with_suffix = lock_file_path(Some("test"));
        assert!(
            path_with_suffix
                .to_str()
                .unwrap()
                .contains("logi-mx-daemon-test.lock")
        );
    }

    #[test]
    fn test_acquire_instance_lock_basic() {
        let lock_result = acquire_instance_lock(Some("test1"));
        assert!(lock_result.is_ok());
        drop(lock_result);
    }

    #[test]
    fn test_lock_file_created_with_pid() {
        let lock = acquire_instance_lock(Some("test2")).unwrap();
        let lock_path = lock_file_path(Some("test2"));
        assert!(lock_path.exists());

        let pid_str = fs::read_to_string(&lock_path).unwrap();
        let pid: u32 = pid_str.trim().parse().unwrap();
        assert_eq!(pid, std::process::id());

        drop(lock);
    }
}
