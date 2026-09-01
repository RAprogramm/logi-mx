// SPDX-FileCopyrightText: 2025 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Blocking device worker thread.
//!
//! HID++ transport is synchronous and can block for seconds on retries, so
//! every device access is routed through this dedicated OS thread. The tokio
//! event loop only forwards events and never touches the device.

use std::{
    collections::HashMap,
    sync::{Mutex, mpsc},
    thread::JoinHandle,
    time::Duration
};

use logi_mx_driver::prelude::*;
use tracing::{debug, error, info, warn};

use crate::status::SharedStatus;

/// Interval between periodic tray status refreshes.
const REFRESH_INTERVAL: Duration = Duration::from_secs(30);

/// Commands accepted by the device worker.
#[derive(Debug)]
pub enum DeviceEvent {
    /// A hidraw device appeared; open and configure it.
    Added(String),
    /// A hidraw device disappeared; drop its handle.
    Removed(String)
}

/// Starts the device worker thread.
///
/// # Arguments
///
/// * `config` - Configuration applied to every discovered device.
/// * `status` - Shared status handle the worker refreshes periodically.
///
/// # Returns
///
/// Command sender plus the worker thread handle for shutdown coordination.
#[must_use]
pub fn spawn_device_worker(
    config: Config,
    status: SharedStatus
) -> (mpsc::Sender<DeviceEvent>, JoinHandle<()>) {
    let (tx, rx) = mpsc::channel();

    let handle = std::thread::Builder::new()
        .name("logi-mx-device-worker".to_string())
        .spawn(move || run_device_worker(config, &rx, &status))
        .unwrap_or_else(|e| panic!("Failed to spawn device worker thread: {e}"));

    (tx, handle)
}

/// Runs the device worker event loop until the command channel closes.
///
/// With the `tray` feature the shared status is refreshed on startup and
/// every [`REFRESH_INTERVAL`]; without it the status handle is accepted but
/// left untouched because nothing subscribes to it.
#[cfg_attr(
    not(feature = "tray"),
    allow(
        unused_variables,
        reason = "the status handle is only consumed by the tray refresh"
    )
)]
fn run_device_worker(config: Config, rx: &mpsc::Receiver<DeviceEvent>, status: &SharedStatus) {
    let manager = DeviceManager::new(config);

    #[cfg(feature = "tray")]
    refresh_tray_status(status);

    loop {
        match rx.recv_timeout(REFRESH_INTERVAL) {
            Ok(DeviceEvent::Added(path)) => manager.handle_device_added(path),
            Ok(DeviceEvent::Removed(path)) => manager.handle_device_removed(&path),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                #[cfg(feature = "tray")]
                refresh_tray_status(status);

                #[cfg(not(feature = "tray"))]
                {}
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break
        }
    }

    debug!("Device worker stopped");
}

/// Queries the device and republishes the shared status snapshot.
///
/// Runs on the worker thread; blocking HID++ I/O is expected here.
#[cfg(feature = "tray")]
fn refresh_tray_status(status: &SharedStatus) {
    match MxMaster3s::open_bolt_receiver_discovered() {
        Ok(mut device) => {
            let (battery_level, dpi) = {
                let mut guard = status
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                guard.connected = true;

                if let Ok(battery) = device.get_battery_info() {
                    guard.battery_level = battery.level;
                    guard.battery_status = format!("{:?}", battery.status);
                }

                if let Ok(current_dpi) = device.get_dpi() {
                    guard.dpi = current_dpi;
                }

                if let Ok(ss_config) = device.get_smartshift() {
                    guard.smartshift = ss_config.enabled;
                    guard.smartshift_threshold = ss_config.threshold;
                }

                (guard.battery_level, guard.dpi)
            };

            debug!("Tray status refreshed: battery={battery_level}%, dpi={dpi}");
        }
        Err(e) => {
            {
                let mut guard = status
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                guard.connected = false;
            }
            debug!("Device not connected: {e}");
        }
    }
}

struct DeviceManager {
    devices: Mutex<HashMap<String, MxMaster3s>>,
    config:  Config
}

impl DeviceManager {
    fn new(config: Config) -> Self {
        Self {
            devices: Mutex::new(HashMap::new()),
            config
        }
    }

    /// Opens the discovered device, applies its configuration and stores the
    /// handle keyed by hidraw path.
    fn handle_device_added(&self, device_path: String) {
        info!("Device added: {device_path}");

        match MxMaster3s::open_bolt_receiver_discovered() {
            Ok(mut device) => {
                if let Ok(name) = device.get_device_name() {
                    info!("Detected: {name}");

                    if let Some(device_config) =
                        self.config.devices.iter().find(|d| d.name == name)
                    {
                        info!("Applying configuration for {name}");
                        Self::apply_config(&mut device, device_config);
                    }

                    self.devices
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .insert(device_path, device);
                }
            }
            Err(e) => {
                warn!("Failed to open device: {e}");
            }
        }
    }

    /// Drops the stored device handle for a removed hidraw path.
    fn handle_device_removed(&self, device_path: &str) {
        info!("Device removed: {device_path}");
        self.devices
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(device_path);
    }

    /// Writes DPI, `SmartShift` and hi-res settings onto the device.
    ///
    /// Individual failures are logged and skipped so one unsupported feature
    /// does not block the remaining settings.
    fn apply_config(device: &mut MxMaster3s, config: &DeviceConfig) {
        debug!("Setting DPI to {}", config.dpi);
        if let Err(e) = device.set_dpi(config.dpi) {
            error!("Failed to set DPI: {e}");
        }

        debug!(
            "Setting SmartShift: enabled={}, threshold={}",
            config.smartshift.enabled, config.smartshift.threshold
        );
        if let Err(e) = device.set_smartshift(config.smartshift) {
            error!("Failed to set SmartShift: {e}");
        }

        debug!(
            "Setting hi-res scroll: enabled={}, inverted={}",
            config.hiresscroll.enabled, config.hiresscroll.inverted
        );
        if let Err(e) = device.set_hires_scroll(config.hiresscroll) {
            error!("Failed to set hi-res scroll: {e}");
        }

        for (button, action) in &config.buttons {
            debug!("Setting button {button:?} to action {action:?}");
            if let Err(e) = device.set_button_action(*button, action.clone()) {
                error!("Failed to set button action: {e}");
            }
        }

        info!("Configuration applied successfully");
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::status::DeviceStatus;

    #[test]
    fn test_worker_exits_on_channel_disconnect() {
        let (tx, rx) = mpsc::channel::<DeviceEvent>();
        drop(tx);

        std::thread::Builder::new()
            .name("test-worker".to_string())
            .spawn(move || {
                run_device_worker(
                    Config::default(),
                    &rx,
                    &Arc::new(Mutex::new(DeviceStatus::default()))
                );
            })
            .unwrap_or_else(|e| panic!("Failed to spawn test worker: {e}"))
            .join()
            .unwrap_or_else(|_| panic!("Worker thread panicked"));
    }

    #[test]
    fn test_device_event_variants() {
        let added = DeviceEvent::Added("/dev/hidraw2".to_string());
        let removed = DeviceEvent::Removed("/dev/hidraw2".to_string());

        match added {
            DeviceEvent::Added(path) => assert_eq!(path, "/dev/hidraw2"),
            DeviceEvent::Removed(_) => panic!("Expected Added event")
        }

        match removed {
            DeviceEvent::Removed(path) => assert_eq!(path, "/dev/hidraw2"),
            DeviceEvent::Added(_) => panic!("Expected Removed event")
        }
    }

    #[test]
    fn test_manager_new_empty() {
        let manager = DeviceManager::new(Config::default());
        assert!(
            manager
                .devices
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .is_empty()
        );
    }
}
