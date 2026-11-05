// SPDX-FileCopyrightText: 2025-2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Gesture and pie configuration requests: everything the gesture runtime
//! owns, answered without touching the device.

use logi_mx_driver::prelude::{Action, Gesture, GestureDirection, GestureMode};

use super::validate::{action_rejection, is_pie_request, pie_edit_rejection, rejection};
use crate::{gesture::GestureRuntime, ipc, worker::pool::DevicePool};

/// Pushes the stored gesture configuration into the runtime and applies the
/// divert state to every connected device.
fn apply_gestures(manager: &DevicePool, gestures: &mut GestureRuntime) {
    let (enabled, bindings, profiles) = manager.gesture_config();
    gestures.set_config(enabled, bindings, profiles);
    gestures.set_pie_config(manager.pie_config());
    gestures.set_pie_profiles(manager.pie_profiles());
    manager.for_each_cached_mut(|device| gestures.apply(device));
}

/// Replaces or inserts one binding in a per-app profile list.
fn upsert_binding(
    bindings: &mut Vec<Gesture>,
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
pub(in crate::worker) fn handle_gesture_request(
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
                profiles,
                pie: manager.pie_config()
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
            if let Some(message) = action_rejection(action) {
                return Some(rejection(message));
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
            if let Some(message) = action_rejection(action) {
                return Some(rejection(message));
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
        ipc::Request::GetPie => {
            return Some(ipc::Response::PieSettings {
                config:   manager.pie_config(),
                profiles: manager.pie_profiles()
            });
        }
        _ if is_pie_request(request) => {
            if let Some(message) = pie_edit_rejection(manager, request) {
                return Some(rejection(message));
            }
            manager.set_pie_request(request)
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
