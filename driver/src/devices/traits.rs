// SPDX-FileCopyrightText: 2025 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

use serde::{Deserialize, Serialize};

use crate::error::Result;

/// Battery charge snapshot reported by the device.
///
/// # Examples
///
/// ```
/// use logi_mx_driver::devices::{BatteryInfo, BatteryStatus};
///
/// let battery = BatteryInfo {
///     level:  75,
///     status: BatteryStatus::Discharging
/// };
/// assert_eq!(battery.level, 75);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatteryInfo {
    /// Remaining charge as a percentage, 0-100.
    pub level:  u8,
    /// Charge state reported by the battery feature.
    pub status: BatteryStatus
}

/// Charge state reported by the device battery feature.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BatteryStatus {
    /// Device is drawing power from the battery.
    Discharging,
    /// Device is connected to external power.
    Charging,
    /// Battery is fully charged.
    Full,
    /// Device reported an unrecognised state.
    Unknown
}

/// `SmartShift` ratchet/free-spin switching behaviour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SmartShiftConfig {
    /// Whether automatic mode switching is active.
    pub enabled:   bool,
    /// Scroll-speed threshold (0-50) that triggers free-spin mode.
    pub threshold: u8
}

/// High-resolution wheel behaviour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct HiResScrollConfig {
    /// Whether high-resolution scroll reporting is enabled.
    pub enabled:  bool,
    /// Whether scroll direction is inverted.
    pub inverted: bool
}

/// Identifies a programmable physical button on the mouse.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ButtonId {
    /// Primary left button.
    LeftClick,
    /// Primary right button.
    RightClick,
    /// Middle click on the main wheel.
    MiddleClick,
    /// Thumb-side back button.
    Back,
    /// Thumb-side forward button.
    Forward,
    /// Thumb-side gesture button.
    ThumbGesture,
    /// Mode-shift button behind the main wheel.
    WheelModeShift
}

/// Action assigned to a button, either a key sequence, a gesture set or a
/// built-in behaviour.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Action {
    /// Sends a key sequence, e.g. `["KEY_LEFTCTRL", "KEY_C"]`.
    Keypress { keys: Vec<String> },
    /// Runs direction-based gestures on the thumb gesture button.
    Gestures { gestures: Vec<Gesture> },
    /// Toggles `SmartShift` ratchet mode.
    ToggleSmartShift,
    /// Clears the button mapping.
    None
}

/// One direction of a gesture bound to the gesture button.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Gesture {
    /// Physical direction the gesture is recognised from.
    pub direction: GestureDirection,
    /// When the action fires relative to press/release.
    pub mode:      GestureMode,
    /// Action executed when the gesture triggers.
    pub action:    Box<Action>
}

/// Recognised gesture direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GestureDirection {
    /// Upward drag.
    Up,
    /// Downward drag.
    Down,
    /// Leftward drag.
    Left,
    /// Rightward drag.
    Right,
    /// Press without directional movement.
    None
}

/// When a gesture action fires.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GestureMode {
    /// Fires when the button is released.
    OnRelease,
    /// Fires as soon as the button is pressed.
    OnPress
}

/// Device-agnostic interface every supported mouse must implement.
///
/// Implementations wrap HID++ transport details so the daemon, CLI and UI can
/// operate on any device without protocol knowledge.
///
/// # Examples
///
/// ```
/// use logi_mx_driver::{
///     devices::{BatteryInfo, BatteryStatus, MouseDevice, SmartShiftConfig},
///     error::{DeviceErrorKind, Result}
/// };
///
/// struct FakeMouse;
///
/// impl MouseDevice for FakeMouse {
///     fn get_device_name(&mut self) -> Result<String> {
///         Ok("MX Master 3S".to_string())
///     }
///
///     fn get_battery_info(&mut self) -> Result<BatteryInfo> {
///         Ok(BatteryInfo {
///             level:  80,
///             status: BatteryStatus::Discharging
///         })
///     }
///
///     fn set_dpi(&mut self, _dpi: u16) -> Result<()> {
///         Ok(())
///     }
///
///     fn get_dpi(&mut self) -> Result<u16> {
///         Ok(1000)
///     }
///
///     fn set_smartshift(&mut self, _config: SmartShiftConfig) -> Result<()> {
///         Ok(())
///     }
///
///     fn get_smartshift(&mut self) -> Result<SmartShiftConfig> {
///         Err(DeviceErrorKind::UnsupportedFeature.into())
///     }
///
///     fn set_hires_scroll(
///         &mut self,
///         _config: logi_mx_driver::devices::HiResScrollConfig
///     ) -> Result<()> {
///         Ok(())
///     }
///
///     fn get_hires_scroll(&mut self) -> Result<logi_mx_driver::devices::HiResScrollConfig> {
///         Err(DeviceErrorKind::UnsupportedFeature.into())
///     }
///
///     fn set_button_action(
///         &mut self,
///         _button: logi_mx_driver::devices::ButtonId,
///         _action: logi_mx_driver::devices::Action
///     ) -> Result<()> {
///         Ok(())
///     }
///
///     fn get_button_action(
///         &mut self,
///         _button: logi_mx_driver::devices::ButtonId
///     ) -> Result<logi_mx_driver::devices::Action> {
///         Err(DeviceErrorKind::NotFound.into())
///     }
///
///     fn ping(&mut self) -> Result<()> {
///         Ok(())
///     }
/// }
///
/// let mut mouse = FakeMouse;
/// assert_eq!(mouse.get_device_name()?, "MX Master 3S");
/// # Ok::<(), masterror::AppError>(())
/// ```
pub trait MouseDevice {
    /// Reads the marketing device name, e.g. `MX Master 3S`.
    ///
    /// # Errors
    ///
    /// Returns [`masterror::AppError`] when the name feature is unsupported
    /// or the device response is invalid.
    fn get_device_name(&mut self) -> Result<String>;

    /// Reads the current battery level and charge state.
    ///
    /// # Errors
    ///
    /// Returns [`masterror::AppError`] when no battery feature is available
    /// or the response cannot be parsed.
    fn get_battery_info(&mut self) -> Result<BatteryInfo>;

    /// Applies a sensor DPI setting.
    ///
    /// # Arguments
    ///
    /// * `dpi` - Sensitivity in DPI; the device clamps to its supported range.
    ///
    /// # Errors
    ///
    /// Returns [`masterror::AppError`] when the DPI feature is unsupported
    /// or the command fails.
    fn set_dpi(&mut self, dpi: u16) -> Result<()>;

    /// Reads the currently applied sensor DPI.
    ///
    /// # Errors
    ///
    /// Returns [`masterror::AppError`] when the DPI feature is unsupported
    /// or the response cannot be parsed.
    fn get_dpi(&mut self) -> Result<u16>;

    /// Applies `SmartShift` ratchet behaviour.
    ///
    /// # Arguments
    ///
    /// * `config` - Enable flag and scroll-speed threshold.
    ///
    /// # Errors
    ///
    /// Returns [`masterror::AppError`] when the `SmartShift` feature is
    /// unsupported or the command fails.
    fn set_smartshift(&mut self, config: SmartShiftConfig) -> Result<()>;

    /// Reads the current `SmartShift` configuration.
    ///
    /// # Errors
    ///
    /// Returns [`masterror::AppError`] when the `SmartShift` feature is
    /// unsupported or the response cannot be parsed.
    fn get_smartshift(&mut self) -> Result<SmartShiftConfig>;

    /// Applies high-resolution wheel settings.
    ///
    /// # Arguments
    ///
    /// * `config` - Enable flag and inversion preference.
    ///
    /// # Errors
    ///
    /// Returns [`masterror::AppError`] when the hi-res wheel feature is
    /// unsupported or the command fails.
    fn set_hires_scroll(&mut self, config: HiResScrollConfig) -> Result<()>;

    /// Reads the current high-resolution wheel settings.
    ///
    /// # Errors
    ///
    /// Returns [`masterror::AppError`] when the hi-res wheel feature is
    /// unsupported or the response cannot be parsed.
    fn get_hires_scroll(&mut self) -> Result<HiResScrollConfig>;

    /// Assigns an action to a physical button.
    ///
    /// # Arguments
    ///
    /// * `button` - Physical button to reprogramme.
    /// * `action` - Action to execute when the button is used.
    ///
    /// # Errors
    ///
    /// Returns [`masterror::AppError`] when the button cannot be programmed.
    fn set_button_action(&mut self, button: ButtonId, action: Action) -> Result<()>;

    /// Reads the action currently assigned to a button.
    ///
    /// # Errors
    ///
    /// Returns [`masterror::AppError`] when the button has no assigned
    /// action or the query fails.
    fn get_button_action(&mut self, button: ButtonId) -> Result<Action>;

    /// Verifies the device responds to HID++ traffic.
    ///
    /// # Errors
    ///
    /// Returns [`masterror::AppError`] when the device does not answer.
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
