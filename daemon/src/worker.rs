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

use std::{
    sync::{Arc, Mutex, mpsc},
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
    Removed(String),
    /// An IPC request awaits the worker's device state; handling it wakes
    /// the poll instead of waiting for the next idle tick.
    Rpc(RpcRequest)
}

/// Handle of a running tray; an uninhabited stub without the `tray`
/// feature, so the slot type — and every signature using it — stays
/// uniform across feature combinations.
#[cfg(feature = "tray")]
pub type TrayHandle = ksni::Handle<crate::tray::LogiTrayIcon>;
#[cfg(not(feature = "tray"))]
#[derive(Clone)]
pub enum TrayHandle {}

/// Tray slot updated after every status refresh. The type is the same with
/// and without the `tray` feature — the slot simply never holds a handle
/// then — so every signature using it stays uniform.
pub type TraySlot = Arc<Mutex<Option<TrayHandle>>>;

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
/// * `pie_hub` - Pie event fan-out the radial menu publishes on.
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
    hub: ipc::LiveHub,
    pie_hub: crate::pie::PieHub,
    anchor: crate::live::SharedAnchor
) -> std::result::Result<(mpsc::Sender<DeviceEvent>, RpcSender, JoinHandle<()>), masterror::AppError>
{
    let (tx, rx) = mpsc::channel();

    let handle = std::thread::Builder::new()
        .name("logi-mx-device-worker".to_string())
        .spawn(move || {
            let telemetry = Telemetry {
                live,
                hub,
                pie_hub,
                anchor
            };
            run_device_worker(config, &rx, &status, &tray, telemetry);
        })
        .map_err(|e| {
            masterror::AppError::internal("Failed to spawn device worker thread").with_source(e)
        })?;

    Ok((tx.clone(), RpcSender::new(tx), handle))
}

/// Output sinks a device worker publishes telemetry to.
pub struct Telemetry {
    /// Shared daemon-side button state merged into live telemetry.
    pub live:    ipc::SharedLiveState,
    /// Live frame fan-out the worker publishes immediate frames on.
    pub hub:     ipc::LiveHub,
    /// Pie event fan-out the radial menu publishes on.
    pub pie_hub: crate::pie::PieHub,
    /// Hidraw path slot the live tap uses to pick the matching event node.
    pub anchor:  crate::live::SharedAnchor
}

fn run_device_worker(
    config: Config,
    rx: &mpsc::Receiver<DeviceEvent>,
    status: &SharedStatus,
    tray: &TraySlot,
    telemetry: Telemetry
) {
    let Telemetry {
        live,
        hub,
        pie_hub,
        anchor
    } = telemetry;
    let mut gestures = GestureRuntime::from_config(&config);
    gestures.attach_live(live.clone(), hub.clone());
    gestures.attach_pie(pie_hub);
    let mut buttons = ButtonRuntime::from_config(&config);
    buttons.attach_live(live, hub);
    let focus = crate::focus::new_shared();
    crate::focus::spawn_watcher(crate::focus::SharedFocus::clone(&focus));
    gestures.attach_focus(focus.clone());
    buttons.attach_focus(focus);
    let mut manager = DevicePool::new(config);
    #[cfg(feature = "notify")]
    let mut policy = crate::notify::NoticePolicy::new();

    #[cfg(feature = "notify")]
    refresh_and_notify(&mut manager, status, &mut policy);
    #[cfg(not(feature = "notify"))]
    refresh_and_notify(&mut manager, status);
    update_tray(tray);
    manager.for_each_cached_mut(|device| gestures.apply(device));
    if buttons.should_divert() {
        manager.for_each_cached_mut(|device| buttons.apply(device));
    }
    let mut last_refresh = std::time::Instant::now();

    loop {
        crate::systemd::mark_worker_beat();

        let poll = if gestures.is_holding() || gestures.has_pending_click() {
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
            Ok(DeviceEvent::Rpc(req)) => {
                handle_rpc(req, &mut manager, &mut gestures, &mut buttons);
                false
            }
            Err(mpsc::RecvTimeoutError::Timeout) => last_refresh.elapsed() >= REFRESH_INTERVAL,
            Err(mpsc::RecvTimeoutError::Disconnected) => break
        };

        pump_notifications(&mut gestures, &mut buttons, &manager);
        sync_anchor(&anchor, &manager);
        if let Some(smartshift) = gestures.take_smartshift_persist() {
            manager.persist_setting(&ipc::Request::SetSmartShift {
                enabled:   smartshift.enabled,
                threshold: smartshift.threshold
            });
        }
        if gestures.is_holding() {
            gestures.on_pie_tick(std::time::Instant::now());
        }
        gestures.on_clock(std::time::Instant::now());

        if should_refresh {
            #[cfg(feature = "notify")]
            refresh_and_notify(&mut manager, status, &mut policy);
            #[cfg(not(feature = "notify"))]
            refresh_and_notify(&mut manager, status);
            update_tray(tray);
            manager.for_each_cached_mut(|device| gestures.apply(device));
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

/// Publishes the tracked device's hidraw path so the live tap can prefer the
/// evdev node of the same physical device over any other Logitech pointer.
fn sync_anchor(anchor: &crate::live::SharedAnchor, manager: &DevicePool) {
    let path = manager.tracked_path().map(std::path::PathBuf::from);
    let mut guard = anchor
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    *guard = path;
}

/// Refreshes the shared status and publishes user notices.
///
/// Reads through the pool's single device handle: a second handle on the
/// same hidraw node would split the HID++ response stream and corrupt
/// concurrent exchanges. Discovery happens through the pool too, so a
/// device that appeared without a udev event still gets cached and
/// configured. Runs on the worker thread; blocking HID++ I/O is expected.
fn refresh_and_notify(
    manager: &mut DevicePool,
    status: &SharedStatus,
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
}

/// Mirrors the latest status into the tray icon. Without the `tray` feature
/// the slot never holds a handle, so the refresh is a no-op.
#[cfg(feature = "tray")]
fn update_tray(tray: &TraySlot) {
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

#[cfg(not(feature = "tray"))]
const fn update_tray(_tray: &TraySlot) {}

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
/// `WatchLive` and `WatchPie` connections are answered by the IPC server
/// itself and never reach this function; the arms exist to keep the
/// protocol exhaustive. `GetKeybinds` and `Ping` are answered without the
/// device, so they are resolved before any device routing and keep working
/// while no receiver is present.
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
    if matches!(request, ipc::Request::Ping) {
        return ipc::Response::Pong;
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
        drop(tx);
        let (live_hub, _live_sink) = tokio::sync::broadcast::channel(1);
        let (pie_hub, _pie_sink) = tokio::sync::broadcast::channel(1);

        std::thread::Builder::new()
            .name("test-worker".to_string())
            .spawn(move || {
                let tray: super::TraySlot = Arc::new(Mutex::new(None));
                run_device_worker(
                    Config::default(),
                    &rx,
                    &Arc::new(Mutex::new(DeviceStatus::default())),
                    &tray,
                    super::Telemetry {
                        live: ipc::SharedLiveState::default(),
                        hub: live_hub,
                        pie_hub,
                        anchor: crate::live::new_anchor()
                    }
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
        let (responder, _receiver) = tokio::sync::oneshot::channel();
        let rpc = DeviceEvent::Rpc(RpcRequest {
            request: ipc::Request::Ping,
            responder
        });

        match added {
            DeviceEvent::Added(path) => assert_eq!(path, "/dev/hidraw2"),
            DeviceEvent::Removed(_) | DeviceEvent::Rpc(_) => panic!("Expected Added event")
        }

        match removed {
            DeviceEvent::Removed(path) => assert_eq!(path, "/dev/hidraw2"),
            DeviceEvent::Added(_) | DeviceEvent::Rpc(_) => panic!("Expected Removed event")
        }

        match rpc {
            DeviceEvent::Rpc(req) => {
                assert!(matches!(req.request, ipc::Request::Ping));
            }
            _ => panic!("Expected Rpc event")
        }
    }
}
