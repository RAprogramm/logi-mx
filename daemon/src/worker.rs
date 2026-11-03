// SPDX-FileCopyrightText: 2025-2026 RAprogramm <andrey.rozanov.vl@gmail.com>
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
    gesture::GestureRuntime,
    ipc::{self, RpcRequest, RpcSender},
    status::SharedStatus
};

/// Interval between periodic status refreshes.
const REFRESH_INTERVAL: Duration = Duration::from_secs(30);

/// Notification poll interval while a gesture hold is in progress; raw-XY
/// reports stream every few milliseconds, so the pump must keep up.
const GESTURE_POLL: Duration = Duration::from_millis(5);

/// Notification poll interval while idle.
const IDLE_POLL: Duration = Duration::from_millis(100);

/// Map key for a handle opened by discovery rather than a udev event.
const DISCOVERED_KEY: &str = "discovered";

/// Commands accepted by the device worker.
#[derive(Debug)]
pub enum DeviceEvent {
    /// A hidraw device appeared; open and configure it.
    Added(String),
    /// A hidraw device disappeared; drop its handle.
    Removed(String)
}

#[cfg(feature = "tray")]
type TrayHandle = ksni::Handle<crate::tray::LogiTrayIcon>;

/// Tray slot updated after every status refresh; a unit slot without the
/// tray feature.
#[cfg(feature = "tray")]
pub type TraySlot = Arc<Mutex<Option<TrayHandle>>>;
#[cfg(not(feature = "tray"))]
pub type TraySlot = ();

#[cfg(feature = "tray")]
static RUNTIME_HANDLE: Mutex<Option<TokioHandle>> = Mutex::new(None);

#[cfg(feature = "tray")]
pub fn set_runtime_handle(handle: TokioHandle) {
    *RUNTIME_HANDLE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(handle);
}

/// Starts the device worker thread.
///
/// # Arguments
///
/// * `config` - Configuration applied to every discovered device.
/// * `status` - Shared status handle the worker refreshes periodically.
/// * `tray` - Tray slot refreshed alongside the shared status.
/// * `live` - Shared daemon-side button state merged into live telemetry.
/// * `hub` - Live frame fan-out the worker publishes immediate frames on.
///
/// # Returns
///
/// Device event sender, RPC sender and worker thread handle.
///
/// # Errors
///
/// Returns an internal error if the worker thread cannot be spawned.
pub fn spawn_device_worker(
    config: Config,
    status: SharedStatus,
    tray: TraySlot,
    live: ipc::SharedLiveState,
    hub: ipc::LiveHub
) -> std::result::Result<(mpsc::Sender<DeviceEvent>, RpcSender, JoinHandle<()>), masterror::AppError>
{
    let (tx, rx) = mpsc::channel();
    let (rpc_tx, rpc_rx) = mpsc::channel();

    let handle = std::thread::Builder::new()
        .name("logi-mx-device-worker".to_string())
        .spawn(move || run_device_worker(config, &rx, &rpc_rx, &status, &tray, live, hub))
        .map_err(|e| {
            masterror::AppError::internal("Failed to spawn device worker thread").with_source(e)
        })?;

    Ok((tx, rpc_tx, handle))
}

fn run_device_worker(
    config: Config,
    rx: &mpsc::Receiver<DeviceEvent>,
    rpc_rx: &mpsc::Receiver<RpcRequest>,
    status: &SharedStatus,
    tray: &TraySlot,
    live: ipc::SharedLiveState,
    hub: ipc::LiveHub
) {
    let mut gestures = GestureRuntime::from_config(&config);
    gestures.attach_live(live, hub);
    let focus = crate::focus::new_shared();
    crate::focus::spawn_watcher(crate::focus::SharedFocus::clone(&focus));
    gestures.attach_focus(focus);
    let mut manager = DevicePool::new(config);
    #[cfg(feature = "notify")]
    let mut policy = crate::notify::NoticePolicy::new();

    refresh_and_notify(&mut manager, status, tray, &mut policy);
    if gestures.should_divert() {
        manager.for_each_cached_mut(|device| gestures.apply(device));
    }
    let mut last_refresh = std::time::Instant::now();

    loop {
        crate::systemd::mark_worker_beat();

        while let Ok(req) = rpc_rx.try_recv() {
            handle_rpc(req, &mut manager, &mut gestures);
        }

        let poll = if gestures.is_holding() {
            GESTURE_POLL
        } else {
            IDLE_POLL
        };
        let should_refresh = match rx.recv_timeout(poll) {
            Ok(DeviceEvent::Added(path)) => {
                manager.handle_device_added(path);
                manager.for_each_cached_mut(|device| gestures.apply(device));
                true
            }
            Ok(DeviceEvent::Removed(path)) => {
                manager.for_each_cached_mut(|device| gestures.detach(device));
                manager.handle_device_removed(&path);
                true
            }
            Err(mpsc::RecvTimeoutError::Timeout) => last_refresh.elapsed() >= REFRESH_INTERVAL,
            Err(mpsc::RecvTimeoutError::Disconnected) => break
        };

        manager.for_each_cached_mut(|device| gestures.pump(device));

        if should_refresh {
            refresh_and_notify(&mut manager, status, tray, &mut policy);
            if gestures.should_divert() {
                manager.for_each_cached_mut(|device| gestures.apply(device));
            }
            last_refresh = std::time::Instant::now();
        }
    }

    manager.for_each_cached_mut(|device| gestures.detach(device));
    debug!("Device worker stopped");
}

/// Refreshes the shared status, publishes user notices and updates the tray.
///
/// Reads through the pool's single device handle: a second handle on the
/// same hidraw node would split the HID++ response stream and corrupt
/// concurrent exchanges. Discovery happens through the pool too, so a
/// device that appeared without a udev event still gets cached and
/// configured. Runs on the worker thread; blocking HID++ I/O is expected.
fn refresh_and_notify(
    manager: &mut DevicePool,
    status: &SharedStatus,
    tray: &TraySlot,
    #[cfg(feature = "notify")] policy: &mut crate::notify::NoticePolicy
) {
    let readings = manager
        .with_device(|device| {
            Ok(crate::status::DeviceReadings {
                battery:    device.battery_info().ok(),
                dpi:        device.dpi().ok(),
                smartshift: device.smartshift().ok()
            })
        })
        .ok();
    crate::status::refresh(status, readings);

    {
        let guard = status
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        #[cfg(feature = "notify")]
        for notice in policy.on_status(&guard, std::time::Instant::now()) {
            crate::notify::publish(&notice);
        }

        let summary = if guard.connected {
            format!(
                "device connected | battery {}% {} | dpi {}",
                guard.battery_level, guard.battery_status, guard.dpi
            )
        } else {
            "device disconnected".to_string()
        };
        drop(guard);

        crate::systemd::notify_status(&summary);
    }

    #[cfg(feature = "tray")]
    {
        let handle = tray
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let runtime = RUNTIME_HANDLE
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        if let (Some(handle), Some(runtime)) = (handle, runtime) {
            runtime.block_on(handle.update(|_| {}));
        }
    }
}

fn handle_rpc(request: RpcRequest, manager: &mut DevicePool, gestures: &mut GestureRuntime) {
    let response = handle_request(manager, gestures, &request.request);
    let _ = request.responder.send(response);
}

/// Pushes the stored gesture configuration into the runtime and applies the
/// divert state to every connected device.
fn apply_gestures(manager: &DevicePool, gestures: &mut GestureRuntime) {
    let (enabled, bindings, profiles) = manager.gesture_config();
    gestures.set_config(enabled, bindings, profiles);
    manager.for_each_cached_mut(|device| gestures.apply(device));
}

/// Validates a gesture action, returning the rejection reason if invalid.
fn gesture_action_rejection(action: &Action) -> Option<String> {
    match action {
        Action::Keypress {
            keys
        } => crate::inject::resolve_sequence(keys)
            .map_err(|e| e.to_string())
            .err(),
        Action::Command {
            command
        } => (command.trim().is_empty()).then(|| "gesture command must not be empty".to_string()),
        _ => Some("gesture action must be a key sequence or a command".to_string())
    }
}

/// Replaces or inserts one binding in a per-app profile list.
fn upsert_binding(
    bindings: &mut Vec<logi_mx_driver::prelude::Gesture>,
    direction: GestureDirection,
    mode: GestureMode,
    action: &Action
) {
    bindings.retain(|gesture| gesture.direction != direction);
    bindings.push(Gesture {
        direction,
        mode,
        action: Box::new(action.clone())
    });
}

/// Handles the configuration-only gesture requests that never touch the
/// device; returns `None` for every other request.
fn handle_gesture_request(
    manager: &mut DevicePool,
    gestures: &mut GestureRuntime,
    request: &ipc::Request
) -> Option<ipc::Response> {
    let stored: std::result::Result<(), masterror::AppError> = match request {
        ipc::Request::GetGestures => {
            let (enabled, bindings, profiles) = manager.gesture_config();
            return Some(ipc::Response::Gestures {
                enabled,
                gestures: bindings,
                profiles
            });
        }
        ipc::Request::GetFocusedApp => {
            return Some(ipc::Response::FocusedApp {
                app_id: gestures.focused_app()
            });
        }
        ipc::Request::SetGesturesEnabled {
            enabled
        } => {
            let (_, bindings, profiles) = manager.gesture_config();
            manager.set_gestures(*enabled, bindings, profiles)
        }
        ipc::Request::SetGesture {
            direction,
            mode,
            action
        } => {
            if let Some(message) = gesture_action_rejection(action) {
                return Some(ipc::Response::Error {
                    message
                });
            }
            let (enabled, mut bindings, profiles) = manager.gesture_config();
            upsert_binding(&mut bindings, *direction, *mode, action);
            manager.set_gestures(enabled, bindings, profiles)
        }
        ipc::Request::ClearGesture {
            direction
        } => {
            let (enabled, mut bindings, profiles) = manager.gesture_config();
            bindings.retain(|gesture| gesture.direction != *direction);
            manager.set_gestures(enabled, bindings, profiles)
        }
        ipc::Request::SetAppGesture {
            app_id,
            direction,
            mode,
            action
        } => {
            if let Some(message) = gesture_action_rejection(action) {
                return Some(ipc::Response::Error {
                    message
                });
            }
            let (enabled, bindings, mut profiles) = manager.gesture_config();
            let entry = profiles.entry(app_id.clone()).or_default();
            upsert_binding(entry, *direction, *mode, action);
            manager.set_gestures(enabled, bindings, profiles)
        }
        ipc::Request::ClearAppGesture {
            app_id,
            direction
        } => {
            let (enabled, bindings, mut profiles) = manager.gesture_config();
            if let Some(entry) = profiles.get_mut(app_id) {
                entry.retain(|gesture| gesture.direction != *direction);
                if entry.is_empty() {
                    profiles.remove(app_id);
                }
            }
            manager.set_gestures(enabled, bindings, profiles)
        }
        ipc::Request::RemoveAppProfile {
            app_id
        } => {
            let (enabled, bindings, mut profiles) = manager.gesture_config();
            profiles.remove(app_id);
            manager.set_gestures(enabled, bindings, profiles)
        }
        _ => return None
    };

    Some(match stored {
        Ok(()) => {
            apply_gestures(manager, gestures);
            ipc::Response::Ok
        }
        Err(e) => ipc::Response::Error {
            message: e.to_string()
        }
    })
}

/// Executes one RPC request against the pooled device.
///
/// `WatchLive` connections are answered by the IPC server itself and never
/// reach this function; the arm exists to keep the protocol exhaustive.
fn handle_request(
    manager: &mut DevicePool,
    gestures: &mut GestureRuntime,
    request: &ipc::Request
) -> ipc::Response {
    if let Some(response) = handle_gesture_request(manager, gestures, request) {
        return response;
    }
    handle_device_request(manager, request)
}

/// Answers read-only device requests; `Ok(None)` for every other request.
fn handle_device_read(
    device: &mut MxMaster3s,
    request: &ipc::Request
) -> std::result::Result<Option<ipc::Response>, masterror::AppError> {
    Ok(Some(match request {
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
        _ => return Ok(None)
    }))
}

/// Executes a device-bound RPC request against the pooled device.
/// Copies one successful device setting from its request into the stored
/// configuration, returning whether the configuration changed.
///
/// Pure so the mapping can be tested without touching the config file.
fn apply_setting(config: &mut Config, request: &ipc::Request) -> bool {
    let Some(device) = config.devices.first_mut() else {
        return false;
    };
    match request {
        ipc::Request::SetDpi {
            value
        } => device.dpi = *value,
        ipc::Request::SetSmartShift {
            enabled,
            threshold
        } => {
            device.smartshift = SmartShiftConfig {
                enabled:   *enabled,
                threshold: *threshold
            }
        }
        ipc::Request::SetHires {
            enabled,
            inverted
        } => {
            device.hiresscroll = HiResScrollConfig {
                enabled:  *enabled,
                inverted: *inverted
            }
        }
        _ => return false
    }
    true
}

fn handle_device_request(manager: &mut DevicePool, request: &ipc::Request) -> ipc::Response {
    match manager.with_device(
        |device| -> std::result::Result<ipc::Response, masterror::AppError> {
            if let Some(response) = handle_device_read(device, request)? {
                return Ok(response);
            }
            Ok(match request {
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
                ipc::Request::SetButtonRemap {
                    control_id,
                    target_cid
                } => {
                    device.set_control_remap(*control_id, *target_cid)?;
                    ipc::Response::Ok
                }
                ipc::Request::SetHost {
                    host
                } => {
                    device.set_host(*host)?;
                    #[cfg(feature = "notify")]
                    crate::notify::host_switched(*host);
                    ipc::Response::Ok
                }
                ipc::Request::WatchLive => ipc::Response::Error {
                    message: "live streaming is handled by the IPC server".to_string()
                },
                _ => ipc::Response::Error {
                    message: "request is answered before device writes".to_string()
                }
            })
        }
    ) {
        Ok(response) => {
            if matches!(response, ipc::Response::Ok) {
                manager.persist_setting(request);
            }
            response
        }
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
    ///
    /// The pool tracks exactly one device: a second handle on the same
    /// receiver would split the HID++ response stream. A handle cached by
    /// discovery is adopted under the real path instead of being reopened.
    fn handle_device_added(&mut self, device_path: String) {
        info!("Device added: {device_path}");

        {
            let mut guard = self
                .devices
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if guard.contains_key(&device_path) {
                debug!("Device at {device_path} is already tracked");
                return;
            }
            if guard.len() == 1
                && let Some(device) = guard.remove(DISCOVERED_KEY)
            {
                debug!("Adopted the discovered handle for {device_path}");
                guard.insert(device_path, device);
                return;
            }
            if !guard.is_empty() {
                debug!("A device is already tracked; ignoring extra node {device_path}");
                return;
            }
        }

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
        let device = guard.entry(DISCOVERED_KEY.to_string()).or_insert(device);
        let result = f(device);
        drop(guard);
        result
    }

    /// Runs a closure against every cached device handle, if any.
    ///
    /// Never discovers a device: used by the periodic gesture pump, which
    /// must stay cheap when no device is connected.
    fn for_each_cached_mut(&self, mut f: impl FnMut(&mut MxMaster3s)) {
        let mut guard = self
            .devices
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for device in guard.values_mut() {
            f(device);
        }
    }

    /// Reads the gesture opt-in, bindings and per-app profiles from the
    /// stored configuration.
    fn gesture_config(
        &self
    ) -> (
        bool,
        Vec<Gesture>,
        std::collections::HashMap<String, Vec<Gesture>>
    ) {
        let Some(device) = self.config.devices.first() else {
            return (false, Vec::new(), std::collections::HashMap::new());
        };
        let bindings = match device.buttons.get(&ButtonId::ThumbGesture) {
            Some(Action::Gestures {
                gestures
            }) => gestures.clone(),
            _ => Vec::new()
        };
        (
            device.gestures_enabled,
            bindings,
            device.gesture_profiles.clone()
        )
    }

    /// Writes the gesture opt-in, bindings and profiles into the
    /// configuration and persists it to disk.
    ///
    /// # Errors
    ///
    /// Returns an error when no device entry exists or the file cannot be
    /// written.
    fn set_gestures(
        &mut self,
        enabled: bool,
        gestures: Vec<Gesture>,
        profiles: std::collections::HashMap<String, Vec<Gesture>>
    ) -> std::result::Result<(), masterror::AppError> {
        let Some(device) = self.config.devices.first_mut() else {
            return Err(masterror::AppError::bad_request("No configured device"));
        };
        device.gestures_enabled = enabled;
        device.gesture_profiles = profiles;
        device.buttons.insert(
            ButtonId::ThumbGesture,
            Action::Gestures {
                gestures
            }
        );
        save_config(&self.config)
    }

    /// Mirrors a successful device setting into the stored configuration and
    /// persists it, so DPI, `SmartShift` and hi-res choices survive a daemon
    /// restart instead of being rolled back by the next `apply_config`.
    fn persist_setting(&mut self, request: &ipc::Request) {
        if apply_setting(&mut self.config, request)
            && let Err(e) = save_config(&self.config)
        {
            warn!("Failed to persist configuration: {e}");
        }
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
        let (live_hub, _live_sink) = tokio::sync::broadcast::channel(1);

        std::thread::Builder::new()
            .name("test-worker".to_string())
            .spawn(move || {
                #[cfg(feature = "tray")]
                let tray: super::TraySlot = Arc::new(Mutex::new(None));
                #[cfg(not(feature = "tray"))]
                let tray: super::TraySlot = ();
                run_device_worker(
                    Config::default(),
                    &rx,
                    &rpc_rx,
                    &Arc::new(Mutex::new(DeviceStatus::default())),
                    &tray,
                    ipc::SharedLiveState::default(),
                    live_hub
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

    #[test]
    fn test_apply_setting_mirrors_device_writes_into_config() {
        let mut config = Config::default();
        assert!(apply_setting(
            &mut config,
            &ipc::Request::SetHires {
                enabled:  true,
                inverted: true
            }
        ));
        assert!(config.devices[0].hiresscroll.enabled);
        assert!(config.devices[0].hiresscroll.inverted);

        assert!(apply_setting(
            &mut config,
            &ipc::Request::SetDpi {
                value: 1600
            }
        ));
        assert_eq!(config.devices[0].dpi, 1600);

        assert!(apply_setting(
            &mut config,
            &ipc::Request::SetSmartShift {
                enabled:   false,
                threshold: 13
            }
        ));
        assert!(!config.devices[0].smartshift.enabled);
        assert_eq!(config.devices[0].smartshift.threshold, 13);
    }

    #[test]
    fn test_apply_setting_ignores_non_config_requests() {
        let mut config = Config::default();
        let before_dpi = config.devices[0].dpi;
        assert!(
            !apply_setting(&mut config, &ipc::Request::Ping),
            "reads must not touch the configuration"
        );
        assert_eq!(config.devices[0].dpi, before_dpi);

        let mut empty = Config {
            devices: Vec::new()
        };
        assert!(
            !apply_setting(
                &mut empty,
                &ipc::Request::SetDpi {
                    value: 800
                }
            ),
            "without a device entry there is nothing to mirror into"
        );
    }
}
