// SPDX-FileCopyrightText: 2025 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

use super::constants::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Feature {
    pub id:    u16,
    pub index: u8
}

impl Feature {
    pub const fn new(id: u16, index: u8) -> Self {
        Self {
            id,
            index
        }
    }

    pub const fn root() -> Self {
        Self::new(FEATURE_ROOT, ROOT_INDEX)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RootFunction {
    GetFeature = 0x00,
    Ping = 0x01
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatteryFunction {
    GetStatus = 0x00,
    GetCapability = 0x01
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DpiFunction {
    GetSensorCount = 0x00,
    GetSensorDpiList = 0x01,
    GetSensorDpi = 0x02,
    SetSensorDpi = 0x03
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmartShiftFunction {
    GetRatchetControlMode = 0x00,
    SetRatchetControlMode = 0x01
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HiresWheelFunction {
    GetCapabilities = 0x00,
    GetMode = 0x01,
    SetMode = 0x02,
    GetRatchetSwitchState = 0x03
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReprogControlsFunction {
    GetControlCount = 0x00,
    GetControlInfo = 0x01,
    GetControlReporting = 0x02,
    SetControlReporting = 0x03
}
