// SPDX-FileCopyrightText: 2025-2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Blocking device worker thread.
//!
//! HID++ transport is synchronous and can block for seconds on retries, so
//! every device access is routed through this dedicated OS thread. The tokio
//! event loop only forwards events and never touches the device.
//!
//! The pooled device handle lives in `pool`; the RPC handlers that run on
//! this thread live in `rpc`.

mod pool;
mod rpc;

#[cfg(feature = "tray")]
use std::sync::Arc;
use std::{
    sync::{Mutex, mpsc},
    thread::JoinHandle,
    time::Duration
};

use logi_mx_driver::prelude::*;
#[cfg(feature = "tray")]
use tokio::runtime::Handle as TokioHandle;
use tracing::debug;

use self::{
    pool::DevicePool,
    rpc::{handle_button_request, handle_device_request, handle_gesture_request}
};
use crate::{
    buttons::ButtonRuntime,
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

/// Remembers the tokio runtime handle the tray updates are dispatched on.
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
    gestures.attach_live(live.clone(), hub.clone());
    let mut buttons = ButtonRuntime::from_config(&config);
    buttons.attach_live(live, hub);
    let focus = crate::focus::new_shared();
    crate::focus::spawn_watcher(crate::focus::SharedFocus::clone(&focus));
    gestures.attach_focus(focus.clone());
    buttons.attach_focus(focus);
    let mut manager = DevicePool::new(config);
    #[cfg(feature = "notify")]
    let mut policy = crate::notify::NoticePolicy::new();

    refresh_and_notify(&mut manager, status, tray, &mut policy);
    if gestures.should_divert() {
        manager.for_each_cached_mut(|device| gestures.apply(device));
    }
    if buttons.should_divert() {
        manager.for_each_cached_mut(|device| buttons.apply(device));
    }
    let mut last_refresh = std::time::Instant::now();

    loop {
        crate::systemd::mark_worker_beat();

        while let Ok(req) = rpc_rx.try_recv() {
            handle_rpc(req, &mut manager, &mut gestures, &mut buttons);
        }

        let poll = if gestures.is_holding() {
            GESTURE_POLL
        } else {
            IDLE_POLL
        };
        let should_refresh = match rx.recv_timeout(poll) {
            Ok(DeviceEvent::Added(path)) => {
                manager.handle_device_added(path);
                manager.for_each_cached_mut(|device| {
                    gestures.apply(device);
                    buttons.apply(device);
                });
                true
            }
            Ok(DeviceEvent::Removed(path)) => {
                manager.for_each_cached_mut(|device| {
                    gestures.detach(device);
                    buttons.detach(device);
                });
                manager.handle_device_removed(&path);
                true
            }
            Err(mpsc::RecvTimeoutError::Timeout) => last_refresh.elapsed() >= REFRESH_INTERVAL,
            Err(mpsc::RecvTimeoutError::Disconnected) => break
        };

        pump_notifications(&mut gestures, &mut buttons, &manager);

        if should_refresh {
            refresh_and_notify(&mut manager, status, tray, &mut policy);
            if gestures.should_divert() {
                manager.for_each_cached_mut(|device| gestures.apply(device));
            }
            if buttons.should_divert() {
                manager.for_each_cached_mut(|device| buttons.apply(device));
            }
            last_refresh = std::time::Instant::now();
        }
    }

    manager.for_each_cached_mut(|device| {
        gestures.detach(device);
        buttons.detach(device);
    });
    debug!("Device worker stopped");
}

/// Drains pending reprogrammable-controls notifications once and dispatches
/// them to both runtimes; the device is read a single time per tick, so the
/// two runtimes never compete for the same report.
fn pump_notifications(
    gestures: &mut GestureRuntime,
    buttons: &mut ButtonRuntime,
    manager: &DevicePool
) {
    if !gestures.wants_pump() && !buttons.wants_pump() {
        return;
    }
    manager.for_each_cached_mut(|device| {
        while let Some(note) = device.read_reprog_notification(0) {
            match note {
                ReprogNotification::DivertedButtons {
                    pressed
                } => {
                    gestures.on_diverted_buttons(&pressed, device);
                    buttons.on_diverted_buttons(&pressed);
                }
                ReprogNotification::RawXy {
                    dx,
                    dy
                } => gestures.on_raw_xy(dx, dy)
            }
        }
    });
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

fn handle_rpc(
    request: RpcRequest,
    manager: &mut DevicePool,
    gestures: &mut GestureRuntime,
    buttons: &mut ButtonRuntime
) {
    let response = handle_request(manager, gestures, buttons, &request.request);
    let _ = request.responder.send(response);
}

/// Executes one RPC request against the pooled device.
///
/// `WatchLive` connections are answered by the IPC server itself and never
/// reach this function; the arm exists to keep the protocol exhaustive.
/// `GetKeybinds` is answered from the desktop, not the device, so it is
/// resolved before any device routing.
fn handle_request(
    manager: &mut DevicePool,
    gestures: &mut GestureRuntime,
    buttons: &mut ButtonRuntime,
    request: &ipc::Request
) -> ipc::Response {
    if matches!(request, ipc::Request::GetKeybinds) {
        return match crate::hyprbind::fetch_keybinds() {
            Ok(entries) => ipc::Response::Keybinds {
                entries
            },
            Err(error) => ipc::Response::Error {
                message: error.to_string()
            }
        };
    }
    if let Some(response) = handle_gesture_request(manager, gestures, request) {
        return response;
    }
    if let Some(response) = handle_button_request(manager, buttons, request) {
        return response;
    }
    handle_device_request(manager, request)
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
}
