// SPDX-FileCopyrightText: 2025 RAprogramm <andrey.rozanov.vl@gmail.com>
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
    Action, BatteryInfo, BatteryStatus, ButtonId, HiResScrollConfig, MouseDevice, SmartShiftConfig
};
use crate::{
    error::{DeviceErrorKind, Result},
    hidpp::{
        BatteryStatusFunction, BatteryUnifiedFunction, ChangeHostFunction, DpiFunction,
        FEATURE_ADJUSTABLE_DPI, FEATURE_BATTERY_STATUS, FEATURE_CHANGE_HOST, FEATURE_DEVICE_NAME,
        FEATURE_HIRES_WHEEL, FEATURE_REPROG_CONTROLS, FEATURE_SMART_SHIFT,
        FEATURE_UNIFIED_BATTERY, HidppDevice, HiresWheelFunction, ReprogControlsFunction,
        SmartShiftFunction
    }
};

const VID_LOGITECH: u16 = 0x046D;
const PID_BOLT_RECEIVER: u16 = 0xC548;
const PID_MX_MASTER_3S_USB: u16 = 0x4082;
const PID_MX_MASTER_3S_BT: u16 = 0xB034;
const RECEIVER_SLOTS: [u8; 6] = [1, 2, 3, 4, 5, 6];

/// Hi-res wheel mode byte: high-resolution reporting bit.
const HIRES_MODE_HIRES_BIT: u8 = 0x02;
/// Hi-res wheel mode byte: inverted direction bit.
const HIRES_MODE_INVERT_BIT: u8 = 0x04;

/// `SmartShift` ratchet mode with automatic disengage.
const SMARTSHIFT_MODE_RATCHET_AUTO: u8 = 0x02;

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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

/// Maps a battery status byte to the [`BatteryStatus`] enum.
///
/// Unified battery (0x1004) encodes `0` discharging, `1-3` charging and
/// `4` fully charged; the legacy 0x1000 feature uses the same low values.
#[must_use]
pub const fn parse_battery_status(status_byte: u8) -> BatteryStatus {
    match status_byte {
        0 => BatteryStatus::Discharging,
        1 | 2 => BatteryStatus::Charging,
        3 | 4 => BatteryStatus::Full,
        _ => BatteryStatus::Unknown
    }
}

/// Parses a `SmartShift` status response into a [`SmartShiftConfig`].
///
/// Response layout: `[mode, autoDisengage, default]`; mode `2` means the
/// ratchet mode with automatic disengage, and a non-zero auto-disengage
/// threshold enables the automatic switching behaviour.
#[must_use]
pub fn parse_smartshift(parameters: &[u8]) -> SmartShiftConfig {
    let mode = parameters.first().copied().unwrap_or(0);
    let auto_disengage = parameters.get(1).copied().unwrap_or(0);

    if mode != SMARTSHIFT_MODE_RATCHET_AUTO || auto_disengage == 0 {
        return SmartShiftConfig {
            enabled:   false,
            threshold: 0
        };
    }

    SmartShiftConfig {
        enabled:   true,
        threshold: auto_disengage
    }
}

/// Encodes a `SmartShift` `SetStatus` payload.
///
/// The wheel always stays in ratchet mode; `enabled` toggles the automatic
/// disengage threshold.
#[must_use]
pub const fn encode_smartshift(config: SmartShiftConfig) -> [u8; 3] {
    let auto_disengage = if config.enabled && config.threshold > 0 {
        config.threshold
    } else {
        0
    };

    [SMARTSHIFT_MODE_RATCHET_AUTO, auto_disengage, 0x00]
}

/// Parses a hi-res wheel mode byte into a [`HiResScrollConfig`].
#[must_use]
pub const fn parse_hires_mode(mode: u8) -> HiResScrollConfig {
    HiResScrollConfig {
        enabled:  mode & HIRES_MODE_HIRES_BIT != 0,
        inverted: mode & HIRES_MODE_INVERT_BIT != 0
    }
}

/// Encodes a hi-res wheel mode byte from the current mode and a config.
///
/// The target and analytics bits of the current mode are preserved so the
/// change only touches resolution and inversion.
#[must_use]
pub const fn encode_hires_mode(current: u8, config: HiResScrollConfig) -> u8 {
    let mut mode = current & !(HIRES_MODE_HIRES_BIT | HIRES_MODE_INVERT_BIT);

    if config.enabled {
        mode |= HIRES_MODE_HIRES_BIT;
    }

    if config.inverted {
        mode |= HIRES_MODE_INVERT_BIT;
    }

    mode
}

/// Appends one device-name chunk to `name` and returns bytes consumed.
///
/// The chunk is truncated to the remaining name length; a NUL byte inside
/// the slice ends the name early, matching how devices pad the payload.
///
/// # Arguments
///
/// * `name` - Accumulated name to extend.
/// * `chunk` - Raw name bytes returned by the device.
/// * `remaining` - Bytes still expected according to the reported length.
///
/// # Returns
///
/// Number of chunk bytes consumed (before truncation at the remaining
/// budget, and never past a NUL terminator).
///
/// # Examples
///
/// ```
/// use logi_mx_driver::devices::mx_master_3s::append_name_chunk;
///
/// let mut name = String::new();
/// let consumed = append_name_chunk(&mut name, b"MX Master 3S For", 25);
/// assert_eq!(consumed, 16);
/// assert_eq!(name, "MX Master 3S For");
/// ```
pub fn append_name_chunk(name: &mut String, chunk: &[u8], remaining: usize) -> usize {
    let take = chunk.len().min(remaining);

    for (index, &byte) in chunk[..take].iter().enumerate() {
        if byte == 0 {
            name.push_str(&String::from_utf8_lossy(&chunk[..index]));
            return index;
        }
    }

    name.push_str(&String::from_utf8_lossy(&chunk[..take]));
    take
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
    /// Opens the mouse paired with a Logi Bolt receiver.
    ///
    /// # Arguments
    ///
    /// * `device_index` - Receiver slot of the paired device, `1`-`6`.
    ///
    /// # Errors
    ///
    /// Returns [`masterror::AppError`] when the receiver is absent, the slot
    /// is empty, or the device does not answer a ping.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use logi_mx_driver::devices::MxMaster3s;
    ///
    /// let device = MxMaster3s::open_bolt_receiver(2)?;
    /// # Ok::<(), masterror::AppError>(())
    /// ```
    pub fn open_bolt_receiver(device_index: u8) -> Result<Self> {
        info!("Opening MX Master 3S via Bolt receiver, device index: {device_index}");

        let mut hidpp = HidppDevice::open_vid_pid(VID_LOGITECH, PID_BOLT_RECEIVER, device_index)?;

        hidpp.ping()?;

        Ok(Self {
            hidpp,
            button_mappings: HashMap::new()
        })
    }

    /// Opens the mouse connected over USB cable.
    ///
    /// # Errors
    ///
    /// Returns [`masterror::AppError`] when no wired device is found or the
    /// device does not answer a ping.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use logi_mx_driver::devices::MxMaster3s;
    ///
    /// let device = MxMaster3s::open_usb()?;
    /// # Ok::<(), masterror::AppError>(())
    /// ```
    pub fn open_usb() -> Result<Self> {
        info!("Opening MX Master 3S via USB");

        let mut hidpp = HidppDevice::open_vid_pid(VID_LOGITECH, PID_MX_MASTER_3S_USB, 0xFF)?;

        hidpp.ping()?;

        Ok(Self {
            hidpp,
            button_mappings: HashMap::new()
        })
    }

    /// Opens the mouse paired over Bluetooth.
    ///
    /// # Errors
    ///
    /// Returns [`masterror::AppError`] when the Bluetooth HID interface is
    /// absent or the device does not answer a ping.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use logi_mx_driver::devices::MxMaster3s;
    ///
    /// let device = MxMaster3s::open_bluetooth()?;
    /// # Ok::<(), masterror::AppError>(())
    /// ```
    pub fn open_bluetooth() -> Result<Self> {
        info!("Opening MX Master 3S via Bluetooth");

        let mut hidpp = HidppDevice::open_vid_pid(VID_LOGITECH, PID_MX_MASTER_3S_BT, 0xFF)?;

        hidpp.ping()?;

        Ok(Self {
            hidpp,
            button_mappings: HashMap::new()
        })
    }

    /// Opens the first MX Master 3S paired to a Logi Bolt receiver.
    ///
    /// Probes every receiver slot in order and returns the first paired
    /// device that answers a ping, so callers do not need to know which slot
    /// the mouse occupies.
    ///
    /// # Errors
    ///
    /// Returns [`masterror::AppError`] when no receiver is present or no
    /// paired device responds in any slot.
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
    pub fn open_bolt_receiver_discovered() -> Result<Self> {
        info!("Discovering MX Master 3S on Bolt receiver slots");

        for index in RECEIVER_SLOTS {
            if let Ok(device) = Self::open_bolt_receiver(index) {
                return Ok(device);
            }
        }

        Err(DeviceErrorKind::NotFound.into())
    }

    /// Returns the raw HID++ transport for diagnostics and extensions.
    #[must_use]
    pub const fn hidpp(&mut self) -> &mut HidppDevice {
        &mut self.hidpp
    }

    /// Reads the host count and current host index from the `ChangeHost`
    /// feature (Easy-Switch).
    ///
    /// # Errors
    ///
    /// Returns [`masterror::AppError`] when the feature is unsupported or
    /// the response cannot be parsed.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use logi_mx_driver::devices::{MouseDevice, MxMaster3s};
    ///
    /// let mut device = MxMaster3s::open_bolt_receiver_discovered()?;
    /// let (hosts, current) = device.host_info()?;
    /// println!("host {current} of {hosts}");
    /// # Ok::<(), masterror::AppError>(())
    /// ```
    pub fn host_info(&mut self) -> Result<(u8, u8)> {
        let feature_index = self.hidpp.feature_index(FEATURE_CHANGE_HOST)?;

        let response =
            self.hidpp
                .send_command(feature_index, ChangeHostFunction::GetHostInfo as u8, &[])?;

        let params = response.parameters();
        let host_count = params.first().copied().unwrap_or(0);
        let current_host = params.get(1).copied().unwrap_or(0);

        debug!("Host info: {current_host} of {host_count}");
        Ok((host_count, current_host))
    }

    /// Enumerates all reprogrammable controls reported by feature 0x1B04.
    ///
    /// # Errors
    ///
    /// Returns [`masterror::AppError`] when the feature is unsupported or a
    /// control info request fails.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use logi_mx_driver::devices::{MouseDevice, MxMaster3s};
    ///
    /// let mut device = MxMaster3s::open_bolt_receiver_discovered()?;
    /// for control in device.list_reprog_controls()? {
    ///     println!("{:?} -> {}", control.control_id, control.control_id);
    /// }
    /// # Ok::<(), masterror::AppError>(())
    /// ```
    pub fn list_reprog_controls(&mut self) -> Result<Vec<ReprogControl>> {
        let feature_index = self.hidpp.feature_index(FEATURE_REPROG_CONTROLS)?;

        let count_response = self.hidpp.send_command(
            feature_index,
            ReprogControlsFunction::GetControlCount as u8,
            &[]
        )?;

        let count = count_response.parameters().first().copied().unwrap_or(0);

        let mut controls = Vec::with_capacity(usize::from(count));

        for index in 0..count {
            let response = self.hidpp.send_command(
                feature_index,
                ReprogControlsFunction::GetControlInfo as u8,
                &[index, 0x00, 0x00]
            )?;

            let params = response.parameters();
            let control = ReprogControl {
                control_id:      params
                    .get(..2)
                    .and_then(|pair| <[u8; 2]>::try_from(pair).ok())
                    .map_or(0, u16::from_be_bytes),
                task_id:         params
                    .get(2..4)
                    .and_then(|pair| <[u8; 2]>::try_from(pair).ok())
                    .map_or(0, u16::from_be_bytes),
                flags:           params.get(4).copied().unwrap_or(0),
                position:        params.get(5).copied().unwrap_or(0),
                group:           params.get(6).copied().unwrap_or(0),
                group_mask:      params.get(7).copied().unwrap_or(0),
                gesture_capable: params.get(8).copied().unwrap_or(0)
            };

            debug!(
                "Control {} ({}) task {:04X} flags {:#04x} group {}",
                control.control_id,
                control_id_name(control.control_id),
                control.task_id,
                control.flags,
                control.group
            );

            controls.push(control);
        }

        Ok(controls)
    }

    /// Reads the reporting configuration of one control by CID.
    ///
    /// Returns `(divert_flags, remap_control_id)`; a zero `divert_flags`
    /// value means the control currently behaves natively.
    ///
    /// # Arguments
    ///
    /// * `control_id` - HID++ control ID, e.g. `0x00C3` for the gesture button.
    ///
    /// # Errors
    ///
    /// Returns [`masterror::AppError`] when the feature is unsupported or
    /// the request fails.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use logi_mx_driver::devices::{MouseDevice, MxMaster3s};
    ///
    /// let mut device = MxMaster3s::open_bolt_receiver_discovered()?;
    /// let (flags, remap) = device.control_divert(0x00C3)?;
    /// println!("flags {flags:#04x}, remap {remap:#06x}");
    /// # Ok::<(), masterror::AppError>(())
    /// ```
    pub fn control_divert(&mut self, control_id: u16) -> Result<(u8, u16)> {
        let feature_index = self.hidpp.feature_index(FEATURE_REPROG_CONTROLS)?;

        let response = self.hidpp.send_command(
            feature_index,
            ReprogControlsFunction::GetControlReporting as u8,
            &[(control_id >> 8) as u8, (control_id & 0xFF) as u8, 0x00]
        )?;

        let params = response.parameters();
        let flags = params.get(2).copied().unwrap_or(0);
        let remap = params
            .get(3..5)
            .and_then(|pair| <[u8; 2]>::try_from(pair).ok())
            .map_or(0, u16::from_be_bytes);

        debug!("Control {control_id:04X} reporting: flags {flags:#04x}, remap {remap:04X}");
        Ok((flags, remap))
    }

    /// Applies a reporting configuration to one control by CID.
    ///
    /// With `divert` set, the device stops performing the native action and
    /// emits diverted button events to the host instead. Clearing `divert`
    /// restores native behaviour. Raw XY diversion additionally streams raw
    /// pointer deltas for gesture support.
    ///
    /// Diverted controls produce no normal HID events until the diversion is
    /// cleared; callers must own the event pump that consumes them.
    ///
    /// # Arguments
    ///
    /// * `control_id` - HID++ control ID.
    /// * `divert` - Whether to divert the control to HID++ notifications.
    /// * `raw_xy` - Whether to also stream raw pointer deltas.
    ///
    /// # Errors
    ///
    /// Returns [`masterror::AppError`] when the feature is unsupported or
    /// the request fails.
    pub fn set_control_divert(
        &mut self,
        control_id: u16,
        divert: bool,
        raw_xy: bool
    ) -> Result<()> {
        let feature_index = self.hidpp.feature_index(FEATURE_REPROG_CONTROLS)?;

        let mut flags = 0u8;
        if divert {
            flags |= 0x03;
        }
        if raw_xy {
            flags |= 0x30;
        }

        self.hidpp.send_command(
            feature_index,
            ReprogControlsFunction::SetControlReporting as u8,
            &[
                (control_id >> 8) as u8,
                (control_id & 0xFF) as u8,
                flags,
                0x00,
                0x00
            ]
        )?;

        info!(
            "Control {control_id:04X} divert: {}, raw_xy: {raw_xy}",
            if divert { "enabled" } else { "disabled" }
        );
        Ok(())
    }

    fn battery_unified(&mut self) -> Result<BatteryInfo> {
        let feature_index = self.hidpp.feature_index(FEATURE_UNIFIED_BATTERY)?;

        let response = self.hidpp.send_command(
            feature_index,
            BatteryUnifiedFunction::GetStatus as u8,
            &[]
        )?;

        let params = response.parameters();
        let level = params.first().copied().unwrap_or(0);
        let status_byte = params.get(2).copied().unwrap_or(0);

        Ok(BatteryInfo {
            level,
            status: parse_battery_status(status_byte)
        })
    }

    fn battery_legacy(&mut self) -> Result<BatteryInfo> {
        let feature_index = self.hidpp.feature_index(FEATURE_BATTERY_STATUS)?;

        let response =
            self.hidpp
                .send_command(feature_index, BatteryStatusFunction::GetStatus as u8, &[])?;

        let params = response.parameters();
        let level = params.first().copied().unwrap_or(0);
        let status_byte = params.get(2).copied().unwrap_or(0);

        Ok(BatteryInfo {
            level,
            status: parse_battery_status(status_byte)
        })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_constants() {
        assert_eq!(VID_LOGITECH, 0x046D);
        assert_eq!(PID_BOLT_RECEIVER, 0xC548);
        assert_eq!(PID_MX_MASTER_3S_USB, 0x4082);
        assert_eq!(PID_MX_MASTER_3S_BT, 0xB034);
        assert_eq!(RECEIVER_SLOTS.len(), 6);
    }

    #[test]
    fn test_parse_battery_status() {
        assert_eq!(parse_battery_status(0), BatteryStatus::Discharging);
        assert_eq!(parse_battery_status(1), BatteryStatus::Charging);
        assert_eq!(parse_battery_status(2), BatteryStatus::Charging);
        assert_eq!(parse_battery_status(3), BatteryStatus::Full);
        assert_eq!(parse_battery_status(4), BatteryStatus::Full);
        assert_eq!(parse_battery_status(5), BatteryStatus::Unknown);
        assert_eq!(parse_battery_status(0xFF), BatteryStatus::Unknown);
    }

    #[test]
    fn test_parse_smartshift() {
        let config = parse_smartshift(&[0x02, 0x0A, 0x0A]);
        assert!(config.enabled);
        assert_eq!(config.threshold, 10);

        let disabled = parse_smartshift(&[0x02, 0x00, 0x0A]);
        assert!(!disabled.enabled);

        let freespin = parse_smartshift(&[0x01, 0x0A, 0x0A]);
        assert!(!freespin.enabled);

        let empty = parse_smartshift(&[]);
        assert!(!empty.enabled);
    }

    #[test]
    fn test_encode_smartshift() {
        let params = encode_smartshift(SmartShiftConfig {
            enabled:   true,
            threshold: 10
        });
        assert_eq!(params, [0x02, 0x0A, 0x00]);

        let disabled = encode_smartshift(SmartShiftConfig {
            enabled:   false,
            threshold: 10
        });
        assert_eq!(disabled, [0x02, 0x00, 0x00]);

        let zero_threshold = encode_smartshift(SmartShiftConfig {
            enabled:   true,
            threshold: 0
        });
        assert_eq!(zero_threshold, [0x02, 0x00, 0x00]);
    }

    #[test]
    fn test_hires_mode_roundtrip() {
        let config = parse_hires_mode(0x06);
        assert!(config.enabled);
        assert!(config.inverted);

        let native = parse_hires_mode(0x00);
        assert!(!native.enabled);
        assert!(!native.inverted);

        let encoded = encode_hires_mode(0x00, config);
        assert_eq!(encoded, 0x06);

        let target_preserved = encode_hires_mode(0x01, config);
        assert_eq!(target_preserved, 0x07);
    }

    #[test]
    fn test_hires_invert_only() {
        let inverted = encode_hires_mode(
            0x02,
            HiResScrollConfig {
                enabled:  true,
                inverted: true
            }
        );
        assert_eq!(inverted, 0x06);

        let restored = encode_hires_mode(
            0x06,
            HiResScrollConfig {
                enabled:  true,
                inverted: false
            }
        );
        assert_eq!(restored, 0x02);
    }

    #[test]
    fn test_append_name_chunk() {
        let mut name = String::new();
        let consumed = append_name_chunk(&mut name, b"MX Master 3S For", 25);
        assert_eq!(consumed, 16);
        assert_eq!(name, "MX Master 3S For");

        let consumed = append_name_chunk(&mut name, b" Business\0\0\0\0\0\0\0\0", 9);
        assert_eq!(consumed, 9);
        assert_eq!(name, "MX Master 3S For Business");
    }

    #[test]
    fn test_append_name_chunk_padding() {
        let mut name = String::new();
        let consumed = append_name_chunk(&mut name, b"MX Master 3S\0\0\0\0", 12);
        assert_eq!(consumed, 12);
        assert_eq!(name, "MX Master 3S");
    }

    #[test]
    fn test_append_name_chunk_empty() {
        let mut name = String::new();
        let consumed = append_name_chunk(&mut name, &[], 10);
        assert_eq!(consumed, 0);
        assert!(name.is_empty());
    }

    #[test]
    fn test_control_id_names() {
        assert_eq!(control_id_name(0x0050), "Left Click");
        assert_eq!(control_id_name(0x0051), "Right Click");
        assert_eq!(control_id_name(0x0052), "Middle Click");
        assert_eq!(control_id_name(0x0053), "Back");
        assert_eq!(control_id_name(0x0056), "Forward");
        assert_eq!(control_id_name(0x00C3), "Gesture Button");
        assert_eq!(control_id_name(0x00C4), "SmartShift Toggle");
        assert_eq!(control_id_name(0x00D7), "Unknown Control");
    }

    #[test]
    fn test_button_mapping() {
        let mut mappings = HashMap::new();
        mappings.insert(ButtonId::ThumbGesture, Action::ToggleSmartShift);
        assert!(mappings.contains_key(&ButtonId::ThumbGesture));
    }
}
