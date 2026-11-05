// SPDX-FileCopyrightText: 2025-2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Mirroring successful device writes into the stored configuration: pure
//! mappings from a request onto the config file shape.

use logi_mx_driver::prelude::{
    Config, DeviceConfig, HiResScrollConfig, PieConfig, SmartShiftConfig
};

use super::validate::is_pie_request;
use crate::ipc;

/// Copies one successful pie setting into the stored device configuration,
/// returning whether the configuration changed.
fn apply_pie_setting(device: &mut DeviceConfig, request: &ipc::Request) -> bool {
    match request {
        ipc::Request::SetPie {
            enabled,
            activation_radius,
            dead_zone
        } => {
            device.pie = PieConfig {
                enabled: *enabled,
                activation_radius: *activation_radius,
                dead_zone: *dead_zone,
                ..device.pie.clone()
            };
        }
        ipc::Request::SetPieSectors {
            sectors
        } => device.pie.sectors.clone_from(sectors),
        ipc::Request::SetAppPieSectors {
            app_id,
            sectors
        } => {
            if sectors.is_empty() {
                device.pie_profiles.remove(app_id);
            } else {
                device
                    .pie_profiles
                    .entry(app_id.clone())
                    .or_default()
                    .sectors
                    .clone_from(sectors);
            }
        }
        ipc::Request::RemoveAppPieProfile {
            app_id
        } => {
            device.pie_profiles.remove(app_id);
        }
        _ => return false
    }
    true
}

/// Copies one successful device setting from its request into the stored
/// configuration, returning whether the configuration changed.
///
/// Pure so the mapping can be tested without touching the config file.
pub(in crate::worker) fn apply_setting(config: &mut Config, request: &ipc::Request) -> bool {
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
        ipc::Request::SetButton {
            button,
            action
        } => {
            device.buttons.insert(*button, *action.clone());
        }
        ipc::Request::ClearButton {
            button
        } => {
            device.buttons.remove(button);
        }
        ipc::Request::SetAppButton {
            app_id,
            button,
            action
        } => {
            device
                .button_profiles
                .entry(app_id.clone())
                .or_default()
                .insert(*button, *action.clone());
        }
        ipc::Request::ClearAppButton {
            app_id,
            button
        } => {
            if let Some(profile) = device.button_profiles.get_mut(app_id) {
                profile.remove(button);
                if profile.is_empty() {
                    device.button_profiles.remove(app_id);
                }
            }
        }
        ipc::Request::RemoveAppButtonProfile {
            app_id
        } => {
            device.button_profiles.remove(app_id);
        }
        _ if is_pie_request(request) => return apply_pie_setting(device, request),
        _ => return false
    }
    true
}
