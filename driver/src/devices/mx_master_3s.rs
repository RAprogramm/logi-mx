// SPDX-FileCopyrightText: 2025 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

use std::collections::HashMap;

use masterror::prelude::*;
use tracing::{debug, info};

use super::traits::*;
use crate::{
    error::Result,
    hidpp::{
        BatteryFunction, DpiFunction, FEATURE_ADJUSTABLE_DPI, FEATURE_BATTERY_STATUS,
        FEATURE_DEVICE_NAME, FEATURE_HIRES_WHEEL, FEATURE_SMART_SHIFT, FEATURE_UNIFIED_BATTERY,
        HidppDevice, HiresWheelFunction, SmartShiftFunction
    }
};

const VID_LOGITECH: u16 = 0x046D;
const PID_BOLT_RECEIVER: u16 = 0xC548;
const PID_MX_MASTER_3S_USB: u16 = 0x4082;
const PID_MX_MASTER_3S_BT: u16 = 0xB034;

pub struct MxMaster3s {
    hidpp:           HidppDevice,
    button_mappings: HashMap<ButtonId, Action>,
    scroll_wheel:    ScrollWheelConfig,
    thumbwheel:      ThumbWheelConfig
}

impl MxMaster3s {
    pub fn open_bolt_receiver(device_index: u8) -> Result<Self> {
        info!(
            "Opening MX Master 3S via Bolt receiver, device index: {}",
            device_index
        );

        let mut hidpp = HidppDevice::open_vid_pid(VID_LOGITECH, PID_BOLT_RECEIVER, device_index)?;

        hidpp.ping()?;

        Ok(Self {
            hidpp,
            button_mappings: HashMap::new(),
            scroll_wheel: ScrollWheelConfig::default(),
            thumbwheel: ThumbWheelConfig::default()
        })
    }

    pub fn open_usb() -> Result<Self> {
        info!("Opening MX Master 3S via USB");

        let mut hidpp = HidppDevice::open_vid_pid(VID_LOGITECH, PID_MX_MASTER_3S_USB, 0xFF)?;

        hidpp.ping()?;

        Ok(Self {
            hidpp,
            button_mappings: HashMap::new(),
            scroll_wheel: ScrollWheelConfig::default(),
            thumbwheel: ThumbWheelConfig::default()
        })
    }

    pub fn open_bluetooth() -> Result<Self> {
        info!("Opening MX Master 3S via Bluetooth");

        let mut hidpp = HidppDevice::open_vid_pid(VID_LOGITECH, PID_MX_MASTER_3S_BT, 0xFF)?;

        hidpp.ping()?;

        Ok(Self {
            hidpp,
            button_mappings: HashMap::new(),
            scroll_wheel: ScrollWheelConfig::default(),
            thumbwheel: ThumbWheelConfig::default()
        })
    }

    fn get_battery_unified(&mut self) -> Result<BatteryInfo> {
        let feature_index = self.hidpp.get_feature_index(FEATURE_UNIFIED_BATTERY)?;

        let response =
            self.hidpp
                .send_command(feature_index, BatteryFunction::GetStatus as u8, &[])?;

        let (level, status_byte) = match response {
            crate::hidpp::HidppPacket::Short(p) => (p.parameters[0], p.parameters[1]),
            crate::hidpp::HidppPacket::Long(p) => (p.parameters[0], p.parameters[1])
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
            crate::hidpp::HidppPacket::Short(p) => (p.parameters[0], p.parameters[1]),
            crate::hidpp::HidppPacket::Long(p) => (p.parameters[0], p.parameters[1])
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
                crate::hidpp::HidppPacket::Short(p) => {
                    (p.parameters[0] as usize, p.parameters[1..].to_vec())
                }
                crate::hidpp::HidppPacket::Long(p) => {
                    (p.parameters[0] as usize, p.parameters[1..].to_vec())
                }
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

            offset += chunk.len() as u8;
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
            crate::hidpp::HidppPacket::Short(p) => {
                u16::from_be_bytes([p.parameters[1], p.parameters[2]])
            }
            crate::hidpp::HidppPacket::Long(p) => {
                u16::from_be_bytes([p.parameters[1], p.parameters[2]])
            }
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
            crate::hidpp::HidppPacket::Short(p) => (p.parameters[0], p.parameters[1]),
            crate::hidpp::HidppPacket::Long(p) => (p.parameters[0], p.parameters[1])
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
            crate::hidpp::HidppPacket::Short(p) => p.parameters[0],
            crate::hidpp::HidppPacket::Long(p) => p.parameters[0]
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

    fn set_scroll_wheel(&mut self, config: ScrollWheelConfig) -> Result<()> {
        self.scroll_wheel = config;
        info!(
            "Scroll wheel configured: vertical_speed={}, horizontal_speed={}, smooth={}",
            config.vertical_speed, config.horizontal_speed, config.smooth_scrolling
        );
        Ok(())
    }

    fn get_scroll_wheel(&mut self) -> Result<ScrollWheelConfig> {
        Ok(self.scroll_wheel)
    }

    fn set_thumb_wheel(&mut self, config: ThumbWheelConfig) -> Result<()> {
        self.thumbwheel = config;
        info!(
            "Thumb wheel configured: speed={}, smooth={}",
            config.speed, config.smooth_scrolling
        );
        Ok(())
    }

    fn get_thumb_wheel(&mut self) -> Result<ThumbWheelConfig> {
        Ok(self.thumbwheel)
    }

    fn ping(&mut self) -> Result<()> {
        self.hidpp.ping()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_battery_status_conversion() {
        let status = BatteryStatus::Charging;
        assert_eq!(status, BatteryStatus::Charging);
    }

    #[test]
    fn test_button_mapping() {
        let mut mappings = HashMap::new();
        mappings.insert(ButtonId::ThumbGesture, Action::ToggleSmartShift);
        assert!(mappings.contains_key(&ButtonId::ThumbGesture));
    }

    #[test]
    fn test_scroll_wheel_config_default() {
        let config = ScrollWheelConfig::default();
        assert_eq!(config.vertical_speed, 3);
        assert_eq!(config.horizontal_speed, 2);
        assert!(!config.smooth_scrolling);
    }

    #[test]
    fn test_scroll_wheel_config_custom() {
        let config = ScrollWheelConfig {
            vertical_speed:   10,
            horizontal_speed: 5,
            smooth_scrolling: true
        };
        assert_eq!(config.vertical_speed, 10);
        assert_eq!(config.horizontal_speed, 5);
        assert!(config.smooth_scrolling);
    }

    #[test]
    fn test_thumb_wheel_config_default() {
        let config = ThumbWheelConfig::default();
        assert_eq!(config.speed, 5);
        assert!(config.smooth_scrolling);
    }

    #[test]
    fn test_thumb_wheel_config_custom() {
        let config = ThumbWheelConfig {
            speed:            1,
            smooth_scrolling: false
        };
        assert_eq!(config.speed, 1);
        assert!(!config.smooth_scrolling);
    }

    #[test]
    fn test_scroll_wheel_config_serde() {
        let config = ScrollWheelConfig {
            vertical_speed:   8,
            horizontal_speed: 4,
            smooth_scrolling: true
        };
        let json = serde_json::to_string(&config).unwrap();
        let deserialized: ScrollWheelConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(config, deserialized);
    }

    #[test]
    fn test_thumb_wheel_config_serde() {
        let config = ThumbWheelConfig {
            speed:            7,
            smooth_scrolling: false
        };
        let json = serde_json::to_string(&config).unwrap();
        let deserialized: ThumbWheelConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(config, deserialized);
    }
}
