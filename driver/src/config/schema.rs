// SPDX-FileCopyrightText: 2025 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::devices::{
    Action, ButtonId, GestureDirection, GestureMode, HiResScrollConfig, SmartShiftConfig
};

/// Root configuration schema persisted at `~/.config/logi-mx.toml`.
///
/// Holds one [`DeviceConfig`] entry per supported mouse. Unknown keys are
/// rejected by `toml`, so keep this structure in sync with documentation.
///
/// # Examples
///
/// ```
/// use logi_mx_driver::config::Config;
///
/// let config = Config::default();
/// assert!(!config.devices.is_empty());
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Per-device settings; the daemon matches them by device name.
    #[serde(default)]
    pub devices: Vec<DeviceConfig>
}

/// Configuration for a single mouse.
///
/// # Examples
///
/// ```
/// use logi_mx_driver::config::DeviceConfig;
///
/// let device = DeviceConfig::default();
/// assert_eq!(device.name, "MX Master 3S");
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceConfig {
    /// Device name reported by HID++ feature 0x0005; used for matching.
    pub name: String,

    /// Sensor sensitivity in DPI, validated against device limits.
    #[serde(default = "default_dpi")]
    pub dpi: u16,

    /// `SmartShift` ratchet/free-spin switching behaviour.
    #[serde(default)]
    pub smartshift: SmartShiftConfig,

    /// High-resolution wheel mode and inversion flag.
    #[serde(default)]
    pub hiresscroll: HiResScrollConfig,

    /// Per-button remapping applied by the daemon on device attach.
    #[serde(default)]
    pub buttons: HashMap<ButtonId, Action>
}

const fn default_dpi() -> u16 {
    1000
}

impl Default for Config {
    fn default() -> Self {
        Self {
            devices: vec![DeviceConfig::default()]
        }
    }
}

impl Default for DeviceConfig {
    fn default() -> Self {
        let mut buttons = HashMap::new();

        buttons.insert(
            ButtonId::ThumbGesture,
            Action::Gestures {
                gestures: vec![
                    crate::devices::Gesture {
                        direction: GestureDirection::Up,
                        mode:      GestureMode::OnRelease,
                        action:    Box::new(Action::Keypress {
                            keys: vec!["KEY_UP".to_string()]
                        })
                    },
                    crate::devices::Gesture {
                        direction: GestureDirection::Down,
                        mode:      GestureMode::OnRelease,
                        action:    Box::new(Action::Keypress {
                            keys: vec!["KEY_DOWN".to_string()]
                        })
                    },
                    crate::devices::Gesture {
                        direction: GestureDirection::Left,
                        mode:      GestureMode::OnRelease,
                        action:    Box::new(Action::Keypress {
                            keys: vec!["KEY_LEFTCTRL".to_string(), "KEY_LEFT".to_string()]
                        })
                    },
                    crate::devices::Gesture {
                        direction: GestureDirection::Right,
                        mode:      GestureMode::OnRelease,
                        action:    Box::new(Action::Keypress {
                            keys: vec!["KEY_LEFTCTRL".to_string(), "KEY_RIGHT".to_string()]
                        })
                    },
                    crate::devices::Gesture {
                        direction: GestureDirection::None,
                        mode:      GestureMode::OnRelease,
                        action:    Box::new(Action::Keypress {
                            keys: vec!["KEY_LEFTMETA".to_string()]
                        })
                    },
                ]
            }
        );

        buttons.insert(ButtonId::WheelModeShift, Action::ToggleSmartShift);

        Self {
            name: "MX Master 3S".to_string(),
            dpi: 1000,
            smartshift: SmartShiftConfig {
                enabled:   true,
                threshold: 20
            },
            hiresscroll: HiResScrollConfig {
                enabled:  true,
                inverted: false
            },
            buttons
        }
    }
}
