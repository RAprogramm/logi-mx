// SPDX-FileCopyrightText: 2025 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::devices::{
    Action, ButtonId, Gesture, GestureDirection, GestureMode, HiResScrollConfig, SmartShiftConfig
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
    pub buttons: HashMap<ButtonId, Action>,

    /// Diverts the gesture button to the daemon for software recognition.
    /// Disabled by default so the button keeps its native firmware behaviour
    /// until the user opts in.
    #[serde(default)]
    pub gestures_enabled: bool,

    /// Per-application gesture overrides keyed by the Hyprland window class.
    /// A direction bound in a profile replaces the global binding for that
    /// direction while the app is focused; unbound directions fall through
    /// to the global set.
    #[serde(default)]
    pub gesture_profiles: HashMap<String, Vec<Gesture>>,

    /// Per-application remaps of the thumb side buttons keyed by the Hyprland
    /// window class. A button bound in a profile replaces the global mapping
    /// for that button while the app is focused; unbound buttons fall through
    /// to the global [`Self::buttons`] map.
    #[serde(default)]
    pub button_profiles: HashMap<String, HashMap<ButtonId, Action>>,

    /// Radial pie menu tuning for the diverted gesture button.
    #[serde(default)]
    pub pie: PieConfig
}

/// Radial pie menu tuning for the diverted gesture button.
///
/// While the gesture button is held, the cursor position around the press
/// point selects one of eight sectors around the anchor; releasing over a
/// bound sector fires its action. The daemon-side state machine lives in
/// `logi-mx-daemon::pie`.
///
/// # Examples
///
/// ```
/// use logi_mx_driver::config::PieConfig;
///
/// let pie = PieConfig::default();
/// assert!(!pie.enabled);
/// assert_eq!(pie.activation_radius, 40);
/// assert_eq!(pie.dead_zone, 12);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PieConfig {
    /// Whether the daemon opens the pie menu on a gesture hold.
    pub enabled:           bool,
    /// Cursor travel from the press point that opens the menu, in pixels.
    pub activation_radius: i32,
    /// Radius inside which no sector is highlighted, in pixels.
    pub dead_zone:         i32
}

impl Default for PieConfig {
    fn default() -> Self {
        Self {
            enabled:           false,
            activation_radius: 40,
            dead_zone:         12
        }
    }
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
            buttons,
            gestures_enabled: false,
            gesture_profiles: HashMap::new(),
            button_profiles: HashMap::new(),
            pie: PieConfig::default()
        }
    }
}
