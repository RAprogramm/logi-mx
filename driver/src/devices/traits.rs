// SPDX-FileCopyrightText: 2025 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

use serde::{Deserialize, Serialize};

use crate::error::Result;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatteryInfo {
    pub level:  u8,
    pub status: BatteryStatus
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BatteryStatus {
    Discharging,
    Charging,
    Full,
    Unknown
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SmartShiftConfig {
    pub enabled:   bool,
    pub threshold: u8
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct HiResScrollConfig {
    pub enabled:  bool,
    pub inverted: bool
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ScrollWheelConfig {
    #[serde(default = "default_scroll_speed")]
    pub vertical_speed: f32,

    #[serde(default = "default_horizontal_speed")]
    pub horizontal_speed: f32,

    #[serde(default)]
    pub smooth_scrolling: bool
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ThumbWheelConfig {
    #[serde(default = "default_thumbwheel_speed")]
    pub speed: f32,

    #[serde(default)]
    pub smooth_scrolling: bool
}

fn default_scroll_speed() -> f32 {
    1.0
}

fn default_horizontal_speed() -> f32 {
    1.0
}

fn default_thumbwheel_speed() -> f32 {
    1.0
}

impl Default for ScrollWheelConfig {
    fn default() -> Self {
        Self {
            vertical_speed:   1.0,
            horizontal_speed: 1.0,
            smooth_scrolling: false
        }
    }
}

impl Default for ThumbWheelConfig {
    fn default() -> Self {
        Self {
            speed:            1.0,
            smooth_scrolling: true
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ButtonId {
    LeftClick,
    RightClick,
    MiddleClick,
    Back,
    Forward,
    ThumbGesture,
    WheelModeShift
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Action {
    Keypress { keys: Vec<String> },
    Gestures { gestures: Vec<Gesture> },
    ToggleSmartShift,
    None
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Gesture {
    pub direction: GestureDirection,
    pub mode:      GestureMode,
    pub action:    Box<Action>
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GestureDirection {
    Up,
    Down,
    Left,
    Right,
    None
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GestureMode {
    OnRelease,
    OnPress
}

pub trait MouseDevice {
    fn get_device_name(&mut self) -> Result<String>;

    fn get_battery_info(&mut self) -> Result<BatteryInfo>;

    fn set_dpi(&mut self, dpi: u16) -> Result<()>;

    fn get_dpi(&mut self) -> Result<u16>;

    fn set_smartshift(&mut self, config: SmartShiftConfig) -> Result<()>;

    fn get_smartshift(&mut self) -> Result<SmartShiftConfig>;

    fn set_hires_scroll(&mut self, config: HiResScrollConfig) -> Result<()>;

    fn get_hires_scroll(&mut self) -> Result<HiResScrollConfig>;

    fn set_scroll_wheel(&mut self, config: ScrollWheelConfig) -> Result<()>;

    fn get_scroll_wheel(&mut self) -> Result<ScrollWheelConfig>;

    fn set_thumb_wheel(&mut self, config: ThumbWheelConfig) -> Result<()>;

    fn get_thumb_wheel(&mut self) -> Result<ThumbWheelConfig>;

    fn set_button_action(&mut self, button: ButtonId, action: Action) -> Result<()>;

    fn get_button_action(&mut self, button: ButtonId) -> Result<Action>;

    fn ping(&mut self) -> Result<()>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_battery_info_creation() {
        let battery = BatteryInfo {
            level:  75,
            status: BatteryStatus::Discharging
        };
        assert_eq!(battery.level, 75);
        assert_eq!(battery.status, BatteryStatus::Discharging);
    }

    #[test]
    fn test_battery_status_variants() {
        let statuses = [
            BatteryStatus::Discharging,
            BatteryStatus::Charging,
            BatteryStatus::Full,
            BatteryStatus::Unknown
        ];
        assert_eq!(statuses.len(), 4);
    }

    #[test]
    fn test_smartshift_config_default() {
        let config = SmartShiftConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.threshold, 0);
    }

    #[test]
    fn test_smartshift_config_custom() {
        let config = SmartShiftConfig {
            enabled:   true,
            threshold: 10
        };
        assert!(config.enabled);
        assert_eq!(config.threshold, 10);
    }

    #[test]
    fn test_hires_scroll_config_default() {
        let config = HiResScrollConfig::default();
        assert!(!config.enabled);
        assert!(!config.inverted);
    }

    #[test]
    fn test_hires_scroll_config_custom() {
        let config = HiResScrollConfig {
            enabled:  true,
            inverted: true
        };
        assert!(config.enabled);
        assert!(config.inverted);
    }

    #[test]
    fn test_button_id_variants() {
        let buttons = [
            ButtonId::LeftClick,
            ButtonId::RightClick,
            ButtonId::MiddleClick,
            ButtonId::Back,
            ButtonId::Forward,
            ButtonId::ThumbGesture,
            ButtonId::WheelModeShift
        ];
        assert_eq!(buttons.len(), 7);
    }

    #[test]
    fn test_action_none() {
        let action = Action::None;
        assert_eq!(action, Action::None);
    }

    #[test]
    fn test_action_toggle_smartshift() {
        let action = Action::ToggleSmartShift;
        assert_eq!(action, Action::ToggleSmartShift);
    }

    #[test]
    fn test_action_keypress() {
        let action = Action::Keypress {
            keys: vec!["ctrl".to_string(), "c".to_string()]
        };
        match action {
            Action::Keypress {
                keys
            } => {
                assert_eq!(keys.len(), 2);
                assert_eq!(keys[0], "ctrl");
                assert_eq!(keys[1], "c");
            }
            _ => panic!("Expected Keypress action")
        }
    }

    #[test]
    fn test_gesture_direction_variants() {
        let directions = [
            GestureDirection::Up,
            GestureDirection::Down,
            GestureDirection::Left,
            GestureDirection::Right,
            GestureDirection::None
        ];
        assert_eq!(directions.len(), 5);
    }

    #[test]
    fn test_gesture_mode_variants() {
        let modes = [GestureMode::OnRelease, GestureMode::OnPress];
        assert_eq!(modes.len(), 2);
    }

    #[test]
    fn test_gesture_creation() {
        let gesture = Gesture {
            direction: GestureDirection::Up,
            mode:      GestureMode::OnRelease,
            action:    Box::new(Action::None)
        };
        assert_eq!(gesture.direction, GestureDirection::Up);
        assert_eq!(gesture.mode, GestureMode::OnRelease);
        assert_eq!(*gesture.action, Action::None);
    }

    #[test]
    fn test_default_functions() {
        assert_eq!(default_scroll_speed(), 1.0);
        assert_eq!(default_horizontal_speed(), 1.0);
        assert_eq!(default_thumbwheel_speed(), 1.0);
    }

    #[test]
    fn test_battery_status_serde() {
        let status = BatteryStatus::Charging;
        let json = serde_json::to_string(&status).unwrap();
        let deserialized: BatteryStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(status, deserialized);
    }

    #[test]
    fn test_battery_info_serde() {
        let battery = BatteryInfo {
            level:  50,
            status: BatteryStatus::Full
        };
        let json = serde_json::to_string(&battery).unwrap();
        let deserialized: BatteryInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(battery, deserialized);
    }
}
