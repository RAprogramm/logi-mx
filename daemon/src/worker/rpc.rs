// SPDX-FileCopyrightText: 2025-2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! RPC handlers executed on the device worker thread: gesture configuration
//! requests, read-only device queries and device-bound writes.

use logi_mx_driver::prelude::*;

use super::pool::DevicePool;
use crate::{gesture::GestureRuntime, ipc};

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
pub(super) fn handle_gesture_request(
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

/// Copies one successful device setting from its request into the stored
/// configuration, returning whether the configuration changed.
///
/// Pure so the mapping can be tested without touching the config file.
pub(super) fn apply_setting(config: &mut Config, request: &ipc::Request) -> bool {
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

/// Executes a device-bound RPC request against the pooled device.
pub(super) fn handle_device_request(
    manager: &mut DevicePool,
    request: &ipc::Request
) -> ipc::Response {
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

#[cfg(test)]
mod tests {
    use super::*;

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
