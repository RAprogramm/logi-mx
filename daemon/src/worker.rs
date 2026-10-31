// SPDX-FileCopyrightText: 2025 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Blocking device worker thread.
//!
//! HID++ transport is synchronous and can block for seconds on retries, so
//! every device access is routed through this dedicated OS thread. The tokio
//! event loop only forwards events and never touches the device.

#[cfg(feature = "tray")]
use std::sync::Arc;
use std::{
    collections::HashMap,
    sync::{Mutex, mpsc},
    thread::JoinHandle,
    time::Duration
};

use logi_mx_driver::prelude::*;
#[cfg(feature = "tray")]
use tokio::runtime::Handle as TokioHandle;
use tracing::{debug, error, info, warn};

use crate::{
    ipc::{self, RpcRequest, RpcSender},
    status::SharedStatus
};

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
/// Device event sender, RPC sender and worker thread handle.
///
/// # Errors
///
/// Returns an internal error if the worker thread cannot be spawned.
#[cfg(feature = "tray")]
type TrayHandle = ksni::Handle<crate::tray::LogiTrayIcon>;

#[cfg(feature = "tray")]
static RUNTIME_HANDLE: Mutex<Option<TokioHandle>> = Mutex::new(None);

#[cfg(feature = "tray")]
pub fn set_runtime_handle(handle: TokioHandle) {
    *RUNTIME_HANDLE.lock().unwrap() = Some(handle);
}

#[cfg(feature = "tray")]
pub fn spawn_device_worker(
    config: Config,
    status: SharedStatus,
    tray_handle: Arc<Mutex<Option<TrayHandle>>>
) -> std::result::Result<(mpsc::Sender<DeviceEvent>, RpcSender, JoinHandle<()>), masterror::AppError>
{
    let (tx, rx) = mpsc::channel();
    let (rpc_tx, rpc_rx) = mpsc::channel();

    let handle = std::thread::Builder::new()
        .name("logi-mx-device-worker".to_string())
        .spawn(move || run_device_worker(config, &rx, &rpc_rx, &status, &tray_handle))
        .map_err(|e| {
            masterror::AppError::internal("Failed to spawn device worker thread").with_source(e)
        })?;

    Ok((tx, rpc_tx, handle))
}

#[cfg(not(feature = "tray"))]
pub fn spawn_device_worker(
    config: Config,
    status: SharedStatus
) -> std::result::Result<(mpsc::Sender<DeviceEvent>, RpcSender, JoinHandle<()>), masterror::AppError>
{
    let (tx, rx) = mpsc::channel();
    let (rpc_tx, rpc_rx) = mpsc::channel();

    let handle = std::thread::Builder::new()
        .name("logi-mx-device-worker".to_string())
        .spawn(move || run_device_worker(config, &rx, &rpc_rx, &status))
        .map_err(|e| {
            masterror::AppError::internal("Failed to spawn device worker thread").with_source(e)
        })?;

    Ok((tx, rpc_tx, handle))
}

#[cfg(feature = "tray")]
fn run_device_worker(
    config: Config,
    rx: &mpsc::Receiver<DeviceEvent>,
    rpc_rx: &mpsc::Receiver<RpcRequest>,
    status: &SharedStatus,
    tray_handle: &Arc<Mutex<Option<TrayHandle>>>
) {
    let mut manager = DevicePool::new(config);

    #[cfg(feature = "tray")]
    refresh_tray_status(status, tray_handle);

    let mut last_refresh = std::time::Instant::now();

    loop {
        while let Ok(req) = rpc_rx.try_recv() {
            handle_rpc(req, &mut manager);
        }

        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(DeviceEvent::Added(path)) => manager.handle_device_added(path),
            Ok(DeviceEvent::Removed(path)) => manager.handle_device_removed(&path),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if last_refresh.elapsed() >= REFRESH_INTERVAL {
                    #[cfg(feature = "tray")]
                    refresh_tray_status(status, tray_handle);

                    #[cfg(not(feature = "tray"))]
                    let _ = status;

                    last_refresh = std::time::Instant::now();
                }

                while let Ok(req) = rpc_rx.try_recv() {
                    handle_rpc(req, &mut manager);
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break
        }
    }

    debug!("Device worker stopped");
}

#[cfg(not(feature = "tray"))]
fn run_device_worker(
    config: Config,
    rx: &mpsc::Receiver<DeviceEvent>,
    rpc_rx: &mpsc::Receiver<RpcRequest>,
    status: &SharedStatus
) {
    let mut manager = DevicePool::new(config);

    let mut last_refresh = std::time::Instant::now();

    loop {
        while let Ok(req) = rpc_rx.try_recv() {
            handle_rpc(req, &mut manager);
        }

        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(DeviceEvent::Added(path)) => manager.handle_device_added(path),
            Ok(DeviceEvent::Removed(path)) => manager.handle_device_removed(&path),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if last_refresh.elapsed() >= REFRESH_INTERVAL {
                    let _ = status;
                    last_refresh = std::time::Instant::now();
                }

                while let Ok(req) = rpc_rx.try_recv() {
                    handle_rpc(req, &mut manager);
                }
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
#[cfg(feature = "tray")]
fn refresh_tray_status(status: &SharedStatus, tray_handle: &Arc<Mutex<Option<TrayHandle>>>) {
    crate::status::refresh_from_device(status);
    if let Some(handle) = tray_handle.lock().unwrap().as_ref() {
        let h = handle.clone();
        if let Some(rt) = RUNTIME_HANDLE.lock().unwrap().as_ref() {
            rt.block_on(h.update(|_| {}));
        }
    }
}

#[cfg(not(feature = "tray"))]
fn refresh_tray_status(status: &SharedStatus) {
    crate::status::refresh_from_device(status);
}

fn handle_rpc(request: RpcRequest, manager: &mut DevicePool) {
    let response = handle_request(manager, &request.request);
    let _ = request.responder.send(response);
}

fn handle_request(manager: &mut DevicePool, request: &ipc::Request) -> ipc::Response {
    match manager.with_device(
        |device| -> std::result::Result<ipc::Response, masterror::AppError> {
            Ok(match request {
                ipc::Request::Ping => ipc::Response::Pong,
                ipc::Request::Info => handle_info(device),
                ipc::Request::Battery => handle_battery(device),
                ipc::Request::Hosts => {
                    let (hosts, current) = device.host_info()?;
                    ipc::Response::Hosts {
                        hosts,
                        current
                    }
                }
                ipc::Request::Buttons => {
                    let controls = device.list_reprog_controls()?;
                    ipc::Response::Buttons {
                        controls
                    }
                }
                ipc::Request::Dpi => {
                    let value = device.dpi()?;
                    ipc::Response::Dpi {
                        value
                    }
                }
                ipc::Request::SmartShift => {
                    let config = device.smartshift()?;
                    ipc::Response::SmartShift {
                        config
                    }
                }
                ipc::Request::HiresScroll => {
                    let config = device.hires_scroll()?;
                    ipc::Response::HiresScroll {
                        config
                    }
                }
                ipc::Request::SetDpi {
                    value
                } => {
                    device.set_dpi(*value)?;
                    ipc::Response::Ok
                }
                ipc::Request::SetSmartShift {
                    enabled,
                    threshold
                } => {
                    let cfg = SmartShiftConfig {
                        enabled:   *enabled,
                        threshold: *threshold
                    };
                    device.set_smartshift(cfg)?;
                    ipc::Response::Ok
                }
                ipc::Request::SetHires {
                    enabled,
                    inverted
                } => {
                    let cfg = HiResScrollConfig {
                        enabled:  *enabled,
                        inverted: *inverted
                    };
                    device.set_hires_scroll(cfg)?;
                    ipc::Response::Ok
                }
                ipc::Request::SetButtonDivert {
                    control_id,
                    divert,
                    raw_xy
                } => {
                    device.set_control_divert(*control_id, *divert, *raw_xy)?;
                    ipc::Response::Ok
                }
                ipc::Request::SetHost {
                    host
                } => {
                    device.set_host(*host)?;
                    ipc::Response::Ok
                }
            })
        }
    ) {
        Ok(response) => response,
        Err(e) => ipc::Response::Error {
            message: e.to_string()
        }
    }
}

fn handle_info(device: &mut MxMaster3s) -> ipc::Response {
    ipc::Response::Info {
        name:       device
            .device_name()
            .unwrap_or_else(|_| "MX Master 3S".to_string()),
        dpi:        device.dpi().unwrap_or(1000),
        smartshift: device.smartshift().unwrap_or_default(),
        hires:      device.hires_scroll().unwrap_or_default()
    }
}

fn handle_battery(device: &mut MxMaster3s) -> ipc::Response {
    match device.battery_info() {
        Ok(info) => ipc::Response::Battery {
            level:  info.level,
            status: format!("{:?}", info.status)
        },
        Err(e) => ipc::Response::Error {
            message: e.to_string()
        }
    }
}

struct DevicePool {
    devices:        Mutex<HashMap<String, MxMaster3s>>,
    config:         Config,
    preferred_slot: Option<u8>
}

impl DevicePool {
    fn new(config: Config) -> Self {
        Self {
            devices: Mutex::new(HashMap::new()),
            config,
            preferred_slot: None
        }
    }

    /// Opens the discovered device, applies its configuration and stores the
    /// handle keyed by hidraw path.
    fn handle_device_added(&mut self, device_path: String) {
        info!("Device added: {device_path}");

        match self.open_device() {
            Ok(mut device) => {
                if let Ok(name) = device.device_name() {
                    info!("Detected: {name}");

                    if let Some(device_config) = self
                        .config
                        .devices
                        .iter()
                        .find(|d| name.starts_with(&d.name))
                    {
                        info!("Applying configuration for {name}");
                        Self::apply_config(&mut device, device_config);
                    }
                } else {
                    debug!("Device opened but name read failed; storing handle anyway");
                }

                self.devices
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .insert(device_path, device);
            }
            Err(e) => {
                warn!("Failed to open device: {e}");
            }
        }
    }

    /// Opens a device, preferring the last successful receiver slot.
    fn open_device(&mut self) -> std::result::Result<MxMaster3s, masterror::AppError> {
        if let Some(slot) = self.preferred_slot
            && let Ok(device) = MxMaster3s::open_bolt_receiver(slot)
        {
            return Ok(device);
        }

        for slot in 1u8..=6 {
            if let Ok(device) = MxMaster3s::open_bolt_receiver(slot) {
                self.preferred_slot = Some(slot);
                return Ok(device);
            }
        }

        Err(DeviceErrorKind::NotFound.into())
    }

    /// Runs a closure against a cached device, discovering one if needed.
    fn with_device<R, F>(&mut self, f: F) -> std::result::Result<R, masterror::AppError>
    where
        F: FnOnce(&mut MxMaster3s) -> std::result::Result<R, masterror::AppError>
    {
        // Reuse a cached handle when available.
        {
            let mut guard = self
                .devices
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(device) = guard.values_mut().next() {
                return f(device);
            }
        }

        let mut device = self.open_device()?;
        if let Ok(name) = device.device_name() {
            info!("Detected: {name}");
            if let Some(device_config) = self
                .config
                .devices
                .iter()
                .find(|d| name.starts_with(&d.name))
            {
                info!("Applying configuration for {name}");
                Self::apply_config(&mut device, device_config);
            }
        }

        let mut guard = self
            .devices
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let device = guard.entry("discovered".to_string()).or_insert(device);
        f(device)
    }

    /// Drops the stored device handle for a removed hidraw path.
    ///
    /// The cached handles are keyed by hidraw path, but RPC discovery stores
    /// under a synthetic key, so any removal invalidates the whole cache to
    /// avoid talking to a detached device.
    fn handle_device_removed(&mut self, device_path: &str) {
        info!("Device removed: {device_path}");
        self.devices
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
        self.preferred_slot = None;
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
        let (_rpc_tx, rpc_rx) = mpsc::channel::<RpcRequest>();
        drop(tx);

        std::thread::Builder::new()
            .name("test-worker".to_string())
            .spawn(move || {
                run_device_worker(
                    Config::default(),
                    &rx,
                    &rpc_rx,
                    &Arc::new(Mutex::new(DeviceStatus::default())),
                    #[cfg(feature = "tray")]
                    &Arc::new(Mutex::new(None))
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
        let manager = DevicePool::new(Config::default());
        assert!(
            manager
                .devices
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .is_empty()
        );
    }
}
