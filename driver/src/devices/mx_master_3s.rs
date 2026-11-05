// SPDX-FileCopyrightText: 2025-2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Logitech MX Master 3S device implementation.
//!
//! Wraps the HID++ transport and maps device features onto the
//! [`MouseDevice`] trait. All wire-format parsing lives in pure helper
//! functions so it is unit tested without hardware.

use std::collections::HashMap;

use masterror::prelude::*;
use tracing::{debug, info};

use super::traits::{
    Action, BatteryInfo, ButtonId, HiResScrollConfig, MouseDevice, SmartShiftConfig
};
use crate::{
    error::{DeviceErrorKind, Result},
    hidpp::{
        DpiFunction, FEATURE_ADJUSTABLE_DPI, FEATURE_DEVICE_NAME, FEATURE_HIRES_WHEEL,
        FEATURE_SMART_SHIFT, HidppDevice, HiresWheelFunction, SmartShiftFunction
    }
};

mod battery;
mod codec;
mod hosts;
mod open;
mod reprog;

#[cfg(test)]
mod tests;

pub use codec::{
    append_name_chunk, encode_hires_mode, encode_smartshift, parse_battery_status,
    parse_hires_mode, parse_smartshift, validate_smartshift
};

/// Control IDs reported by the Reprogrammable Controls feature.
pub mod control_id {
    /// Left mouse button.
    pub const LEFT_BUTTON: u16 = 0x0050;
    /// Right mouse button.
    pub const RIGHT_BUTTON: u16 = 0x0051;
    /// Middle mouse button.
    pub const MIDDLE_BUTTON: u16 = 0x0052;
    /// Back button.
    pub const BACK_BUTTON: u16 = 0x0053;
    /// Forward button.
    pub const FORWARD_BUTTON: u16 = 0x0056;
    /// Gesture (thumb) button.
    pub const GESTURE_BUTTON: u16 = 0x00C3;
    /// `SmartShift` mode-shift toggle button.
    pub const SMARTSHIFT_TOGGLE: u16 = 0x00C4;
}

/// One reprogrammable control as reported by feature 0x1B04.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ReprogControl {
    /// HID++ control ID (CID).
    pub control_id:      u16,
    /// HID++ task ID assigned by the device.
    pub task_id:         u16,
    /// Control flags: mouse button, reprogrammable, divertable and so on.
    pub flags:           u8,
    /// Position byte (non-zero for F-key style controls).
    pub position:        u8,
    /// Group the control belongs to.
    pub group:           u8,
    /// Bitmask of groups this control can be remapped into.
    pub group_mask:      u8,
    /// Whether the control can act as a gesture button.
    pub gesture_capable: u8
}

/// Human-readable label for a control ID.
#[must_use]
pub const fn control_id_name(control_id: u16) -> &'static str {
    match control_id {
        control_id::LEFT_BUTTON => "Left Click",
        control_id::RIGHT_BUTTON => "Right Click",
        control_id::MIDDLE_BUTTON => "Middle Click",
        control_id::BACK_BUTTON => "Back",
        control_id::FORWARD_BUTTON => "Forward",
        control_id::GESTURE_BUTTON => "Gesture Button",
        control_id::SMARTSHIFT_TOGGLE => "SmartShift Toggle",
        _ => "Unknown Control"
    }
}

/// Logitech MX Master 3S connected via Bolt receiver, USB or Bluetooth.
///
/// Implements [`MouseDevice`] on top of the HID++ 2.0 protocol and keeps an
/// in-memory map of programmable button actions.
///
/// # Examples
///
/// ```no_run
/// use logi_mx_driver::devices::{MouseDevice, MxMaster3s};
///
/// let mut device = MxMaster3s::open_bolt_receiver_discovered()?;
/// println!("{}", device.device_name()?);
/// # Ok::<(), masterror::AppError>(())
/// ```
pub struct MxMaster3s {
    hidpp:           HidppDevice,
    button_mappings: HashMap<ButtonId, Action>
}

impl MxMaster3s {
    /// Returns the raw HID++ transport for diagnostics and extensions.
    #[must_use]
    pub const fn hidpp(&mut self) -> &mut HidppDevice {
        &mut self.hidpp
    }
}

impl MouseDevice for MxMaster3s {
    fn device_name(&mut self) -> Result<String> {
        let feature_index = self.hidpp.feature_index(FEATURE_DEVICE_NAME)?;

        let length_response = self
            .hidpp
            .send_command(feature_index, 0x00, &[0x00, 0x00, 0x00])?;

        let name_len = usize::from(length_response.parameters().first().copied().unwrap_or(0));

        let mut name = String::new();
        let mut offset = 0usize;

        while offset < name_len {
            let offset_byte =
                u8::try_from(offset).map_err(|_| DeviceErrorKind::InvalidResponse)?;
            let response =
                self.hidpp
                    .send_command(feature_index, 0x01, &[offset_byte, 0x00, 0x00])?;

            let remaining = name_len - offset;
            let consumed = append_name_chunk(&mut name, response.parameters(), remaining);

            if consumed == 0 {
                return Err(DeviceErrorKind::InvalidResponse.into());
            }

            offset += consumed;
        }

        if name.trim().is_empty() {
            name = "Logitech MX Master 3S".to_string();
        }

        debug!("Device name: {name}");
        Ok(name)
    }

    fn battery_info(&mut self) -> Result<BatteryInfo> {
        self.battery_unified().or_else(|_| self.battery_legacy())
    }

    fn set_dpi(&mut self, dpi: u16) -> Result<()> {
        let feature_index = self.hidpp.feature_index(FEATURE_ADJUSTABLE_DPI)?;

        let params = [0x00, (dpi >> 8) as u8, (dpi & 0xFF) as u8];

        self.hidpp
            .send_command(feature_index, DpiFunction::SetSensorDpi as u8, &params)?;

        info!("DPI set to {dpi}");
        Ok(())
    }

    fn dpi(&mut self) -> Result<u16> {
        let feature_index = self.hidpp.feature_index(FEATURE_ADJUSTABLE_DPI)?;

        let response = self.hidpp.send_command(
            feature_index,
            DpiFunction::GetSensorDpi as u8,
            &[0x00, 0x00, 0x00]
        )?;

        let params = response.parameters();
        let dpi = params
            .get(1..3)
            .and_then(|pair| <[u8; 2]>::try_from(pair).ok())
            .map_or(0, u16::from_be_bytes);

        debug!("Current DPI: {dpi}");
        Ok(dpi)
    }

    fn set_smartshift(&mut self, config: SmartShiftConfig) -> Result<()> {
        validate_smartshift(&config)?;
        let feature_index = self.hidpp.feature_index(FEATURE_SMART_SHIFT)?;

        let params = encode_smartshift(config);

        self.hidpp
            .send_command(feature_index, SmartShiftFunction::SetStatus as u8, &params)?;

        info!(
            "SmartShift configured: enabled={}, threshold={}",
            config.enabled, config.threshold
        );
        Ok(())
    }

    fn smartshift(&mut self) -> Result<SmartShiftConfig> {
        let feature_index = self.hidpp.feature_index(FEATURE_SMART_SHIFT)?;

        let response =
            self.hidpp
                .send_command(feature_index, SmartShiftFunction::GetStatus as u8, &[])?;

        Ok(parse_smartshift(response.parameters()))
    }

    fn set_hires_scroll(&mut self, config: HiResScrollConfig) -> Result<()> {
        let feature_index = self.hidpp.feature_index(FEATURE_HIRES_WHEEL)?;

        let current =
            self.hidpp
                .send_command(feature_index, HiresWheelFunction::GetWheelMode as u8, &[])?;

        let current_mode = current.parameters().first().copied().unwrap_or(0);
        let mode = encode_hires_mode(current_mode, config);
        let params = [mode, 0x00, 0x00];

        self.hidpp.send_command(
            feature_index,
            HiresWheelFunction::SetWheelMode as u8,
            &params
        )?;

        info!(
            "Hi-res scroll configured: enabled={}, inverted={}",
            config.enabled, config.inverted
        );
        Ok(())
    }

    fn hires_scroll(&mut self) -> Result<HiResScrollConfig> {
        let feature_index = self.hidpp.feature_index(FEATURE_HIRES_WHEEL)?;

        let response =
            self.hidpp
                .send_command(feature_index, HiresWheelFunction::GetWheelMode as u8, &[])?;

        let mode = response.parameters().first().copied().unwrap_or(0);

        Ok(parse_hires_mode(mode))
    }

    fn set_button_action(&mut self, button: ButtonId, action: Action) -> Result<()> {
        self.button_mappings.insert(button, action);
        debug!("Button {button:?} action configured");
        Ok(())
    }

    fn button_action(&mut self, button: ButtonId) -> Result<Action> {
        self.button_mappings
            .get(&button)
            .cloned()
            .ok_or_else(|| AppError::not_found("Button action not configured"))
    }

    fn ping(&mut self) -> Result<()> {
        self.hidpp.ping()
    }
}
