// SPDX-FileCopyrightText: 2025-2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Battery reporting through the unified and the legacy HID++ features.

use super::{MxMaster3s, parse_battery_status};
use crate::{
    devices::traits::BatteryInfo,
    error::Result,
    hidpp::{
        BatteryStatusFunction, BatteryUnifiedFunction, FEATURE_BATTERY_STATUS,
        FEATURE_UNIFIED_BATTERY
    }
};

impl MxMaster3s {
    fn battery_via(&mut self, feature_id: u16, function: u8) -> Result<BatteryInfo> {
        let feature_index = self.hidpp.feature_index(feature_id)?;
        let response = self.hidpp.send_command(feature_index, function, &[])?;
        let params = response.parameters();
        let level = params.first().copied().unwrap_or(0);
        let status_byte = params.get(2).copied().unwrap_or(0);
        Ok(BatteryInfo {
            level,
            status: parse_battery_status(status_byte)
        })
    }

    pub(super) fn battery_unified(&mut self) -> Result<BatteryInfo> {
        self.battery_via(
            FEATURE_UNIFIED_BATTERY,
            BatteryUnifiedFunction::GetStatus as u8
        )
    }

    pub(super) fn battery_legacy(&mut self) -> Result<BatteryInfo> {
        self.battery_via(
            FEATURE_BATTERY_STATUS,
            BatteryStatusFunction::GetStatus as u8
        )
    }
}
