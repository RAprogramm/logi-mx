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
