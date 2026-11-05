// SPDX-FileCopyrightText: 2025-2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Thumb button remap requests: the software remap of the two side buttons,
//! answered without touching the device directly.

use super::validate::{action_rejection, rejection, thumb_button_rejection};
use crate::{buttons::ButtonRuntime, ipc, worker::pool::DevicePool};

/// Pushes the stored thumb button remaps into the runtime and reconciles the
/// divert state of every connected device.
fn apply_buttons(manager: &DevicePool, buttons: &mut ButtonRuntime) {
    let (global, profiles) = manager.button_config();
    buttons.set_config(global, profiles);
    manager.for_each_cached_mut(|device| buttons.apply(device));
}

/// Handles the configuration-only thumb button remap requests that never
/// touch the device directly; returns `None` for every other request.
pub(in crate::worker) fn handle_button_request(
    manager: &mut DevicePool,
    buttons: &mut ButtonRuntime,
    request: &ipc::Request
) -> Option<ipc::Response> {
    let stored: std::result::Result<(), masterror::AppError> = match request {
        ipc::Request::GetButtons => {
            let (global, profiles) = manager.button_config();
            return Some(ipc::Response::ButtonsConfig {
                buttons: global,
                profiles
            });
        }
        ipc::Request::SetButton {
            button,
            action
        }
        | ipc::Request::SetAppButton {
            button,
            action,
            ..
        } => {
            if let Some(message) =
                thumb_button_rejection(*button).or_else(|| action_rejection(action))
            {
                return Some(rejection(message));
            }
            manager.set_button_request(request)
        }
        ipc::Request::ClearButton {
            ..
        }
        | ipc::Request::ClearAppButton {
            ..
        }
        | ipc::Request::RemoveAppButtonProfile {
            ..
        } => manager.set_button_request(request),
        _ => return None
    };

    Some(match stored {
        Ok(()) => {
            apply_buttons(manager, buttons);
            ipc::Response::Ok
        }
        Err(e) => ipc::Response::Error {
            message: e.to_string()
        }
    })
}
