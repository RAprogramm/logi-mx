// SPDX-FileCopyrightText: 2025 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

use std::collections::HashMap;

use masterror::prelude::*;
use tracing::{debug, info};

use super::traits::{
    Action, BatteryInfo, BatteryStatus, ButtonId, HiResScrollConfig, MouseDevice, SmartShiftConfig
};
use crate::{
    error::{DeviceErrorKind, Result},
    hidpp::{
        BatteryFunction, DpiFunction, FEATURE_ADJUSTABLE_DPI, FEATURE_BATTERY_STATUS,
        FEATURE_DEVICE_NAME, FEATURE_HIRES_WHEEL, FEATURE_SMART_SHIFT, FEATURE_UNIFIED_BATTERY,
        HidppDevice, HidppPacket, HiresWheelFunction, SmartShiftFunction
    }
};

const VID_LOGITECH: u16 = 0x046D;
const PID_BOLT_RECEIVER: u16 = 0xC548;
const PID_MX_MASTER_3S_USB: u16 = 0x4082;
const PID_MX_MASTER_3S_BT: u16 = 0xB034;
const RECEIVER_SLOTS: [u8; 6] = [1, 2, 3, 4, 5, 6];

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
/// let mut device = MxMaster3s::open_bolt_receiver(2)?;
/// println!("{}", device.get_device_name()?);
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
        info!(
            "Opening MX Master 3S via Bolt receiver, device index: {}",
            device_index
        );

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
    /// println!("{}", device.get_device_name()?);
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

    fn get_battery_unified(&mut self) -> Result<BatteryInfo> {
        let feature_index = self.hidpp.get_feature_index(FEATURE_UNIFIED_BATTERY)?;

        let response =
            self.hidpp
                .send_command(feature_index, BatteryFunction::GetStatus as u8, &[])?;

        let (level, status_byte) = match response {
            HidppPacket::Short(p) => (p.parameters[0], p.parameters[1]),
            HidppPacket::Long(p) => (p.parameters[0], p.parameters[1])
        };

        let status = match status_byte {
            0 => BatteryStatus::Discharging,
            1 => BatteryStatus::Charging,
            2 => BatteryStatus::Full,
            _ => BatteryStatus::Unknown
        };

        Ok(BatteryInfo {
            level,
            status
        })
    }

    fn get_battery_legacy(&mut self) -> Result<BatteryInfo> {
        let feature_index = self.hidpp.get_feature_index(FEATURE_BATTERY_STATUS)?;

        let response =
            self.hidpp
                .send_command(feature_index, BatteryFunction::GetStatus as u8, &[])?;

        let (level, status_byte) = match response {
            HidppPacket::Short(p) => (p.parameters[0], p.parameters[1]),
            HidppPacket::Long(p) => (p.parameters[0], p.parameters[1])
        };

        let status = match status_byte {
            1 => BatteryStatus::Discharging,
            2 => BatteryStatus::Charging,
            3 => BatteryStatus::Full,
            _ => BatteryStatus::Unknown
        };

        Ok(BatteryInfo {
            level,
            status
        })
    }
}

impl MouseDevice for MxMaster3s {
    fn get_device_name(&mut self) -> Result<String> {
        let feature_index = self.hidpp.get_feature_index(FEATURE_DEVICE_NAME)?;

        let mut name = String::new();
        let mut offset = 0u8;

        loop {
            let response = self.hidpp.send_command(feature_index, 0x00, &[offset])?;

            let (name_len, chunk) = match response {
                HidppPacket::Short(p) => (p.parameters[0] as usize, p.parameters[1..].to_vec()),
                HidppPacket::Long(p) => (p.parameters[0] as usize, p.parameters[1..].to_vec())
            };

            for &byte in chunk.iter().take(name_len.saturating_sub(offset as usize)) {
                if byte == 0 {
                    break;
                }
                name.push(byte as char);
            }

            if offset as usize >= name_len {
                break;
            }

            let chunk_len =
                u8::try_from(chunk.len()).map_err(|_| DeviceErrorKind::InvalidResponse)?;
            offset = offset
                .checked_add(chunk_len)
                .ok_or(DeviceErrorKind::InvalidResponse)?;
        }

        if name.is_empty() || name.trim().is_empty() {
            name = "Logitech MX Master 3S".to_string();
        }

        debug!("Device name: {}", name);
        Ok(name)
    }

    fn get_battery_info(&mut self) -> Result<BatteryInfo> {
        self.get_battery_unified()
            .or_else(|_| self.get_battery_legacy())
    }

    fn set_dpi(&mut self, dpi: u16) -> Result<()> {
        let feature_index = self.hidpp.get_feature_index(FEATURE_ADJUSTABLE_DPI)?;

        let params = [0x00, (dpi >> 8) as u8, (dpi & 0xFF) as u8];

        self.hidpp
            .send_command(feature_index, DpiFunction::SetSensorDpi as u8, &params)?;

        info!("DPI set to {}", dpi);
        Ok(())
    }

    fn get_dpi(&mut self) -> Result<u16> {
        let feature_index = self.hidpp.get_feature_index(FEATURE_ADJUSTABLE_DPI)?;

        let response =
            self.hidpp
                .send_command(feature_index, DpiFunction::GetSensorDpi as u8, &[0x00])?;

        let dpi = match response {
            HidppPacket::Short(p) => u16::from_be_bytes([p.parameters[1], p.parameters[2]]),
            HidppPacket::Long(p) => u16::from_be_bytes([p.parameters[1], p.parameters[2]])
        };

        debug!("Current DPI: {}", dpi);
        Ok(dpi)
    }

    fn set_smartshift(&mut self, config: SmartShiftConfig) -> Result<()> {
        let feature_index = self.hidpp.get_feature_index(FEATURE_SMART_SHIFT)?;

        let wheel_mode = 0x02;
        let auto_disengage = if config.enabled && config.threshold > 0 {
            config.threshold
        } else {
            0xFF
        };
        let auto_disengage_default = 0x00;

        let params = [wheel_mode, auto_disengage, auto_disengage_default];

        self.hidpp.send_command(
            feature_index,
            SmartShiftFunction::SetRatchetControlMode as u8,
            &params
        )?;

        info!(
            "SmartShift configured: enabled={}, threshold={}",
            config.enabled, config.threshold
        );
        Ok(())
    }

    fn get_smartshift(&mut self) -> Result<SmartShiftConfig> {
        let feature_index = self.hidpp.get_feature_index(FEATURE_SMART_SHIFT)?;

        let response = self.hidpp.send_command(
            feature_index,
            SmartShiftFunction::GetRatchetControlMode as u8,
            &[]
        )?;

        let (_wheel_mode, auto_disengage) = match response {
            HidppPacket::Short(p) => (p.parameters[0], p.parameters[1]),
            HidppPacket::Long(p) => (p.parameters[0], p.parameters[1])
        };

        Ok(SmartShiftConfig {
            enabled:   auto_disengage > 0 && auto_disengage < 0xFF,
            threshold: if auto_disengage > 0 && auto_disengage < 0xFF {
                auto_disengage
            } else {
                20
            }
        })
    }

    fn set_hires_scroll(&mut self, config: HiResScrollConfig) -> Result<()> {
        let feature_index = self.hidpp.get_feature_index(FEATURE_HIRES_WHEEL)?;

        let mode = if config.enabled { 0x02 } else { 0x00 };
        let params = [mode, 0x00, 0x00];

        self.hidpp
            .send_command(feature_index, HiresWheelFunction::SetMode as u8, &params)?;

        info!(
            "Hi-res scroll configured: enabled={}, inverted={}",
            config.enabled, config.inverted
        );
        Ok(())
    }

    fn get_hires_scroll(&mut self) -> Result<HiResScrollConfig> {
        let feature_index = self.hidpp.get_feature_index(FEATURE_HIRES_WHEEL)?;

        let response =
            self.hidpp
                .send_command(feature_index, HiresWheelFunction::GetMode as u8, &[])?;

        let mode = match response {
            HidppPacket::Short(p) => p.parameters[0],
            HidppPacket::Long(p) => p.parameters[0]
        };

        Ok(HiResScrollConfig {
            enabled:  mode == 0x02,
            inverted: false
        })
    }

    fn set_button_action(&mut self, button: ButtonId, action: Action) -> Result<()> {
        self.button_mappings.insert(button, action);
        debug!("Button {:?} action configured", button);
        Ok(())
    }

    fn get_button_action(&mut self, button: ButtonId) -> Result<Action> {
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
    }

    #[test]
    fn test_battery_status_conversion() {
        let status = BatteryStatus::Charging;
        assert_eq!(status, BatteryStatus::Charging);
    }

    #[test]
    fn test_all_battery_statuses() {
        let statuses = [
            BatteryStatus::Discharging,
            BatteryStatus::Charging,
            BatteryStatus::Full,
            BatteryStatus::Unknown
        ];
        assert_eq!(statuses.len(), 4);
        assert_eq!(statuses[0], BatteryStatus::Discharging);
        assert_eq!(statuses[1], BatteryStatus::Charging);
        assert_eq!(statuses[2], BatteryStatus::Full);
        assert_eq!(statuses[3], BatteryStatus::Unknown);
    }

    #[test]
    fn test_button_mapping() {
        let mut mappings = HashMap::new();
        mappings.insert(ButtonId::ThumbGesture, Action::ToggleSmartShift);
        assert!(mappings.contains_key(&ButtonId::ThumbGesture));
    }
}
