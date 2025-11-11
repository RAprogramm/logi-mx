// SPDX-FileCopyrightText: 2025 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::devices::{
    Action, ButtonId, GestureDirection, GestureMode, HiResScrollConfig, SmartShiftConfig
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub devices: Vec<DeviceConfig>
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceConfig {
    pub name: String,

    #[serde(default = "default_dpi")]
    pub dpi: u16,

    #[serde(default)]
    pub smartshift: SmartShiftConfig,

    #[serde(default)]
    pub hiresscroll: HiResScrollConfig,

    #[serde(default)]
    pub buttons: HashMap<ButtonId, Action>
}

fn default_dpi() -> u16 {
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
