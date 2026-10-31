// SPDX-FileCopyrightText: 2025 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

use logi_mx_daemon::ipc::{Request, Response, try_request};
use logi_mx_driver::prelude::*;

const DEFAULT_DEVICE_NAME: &str = "MX Master 3S";
const DEFAULT_DPI: u16 = 1000;

/// One-shot device state collected at startup.
///
/// The UI opens the device exactly once per launch and seeds every settings
/// group from this snapshot instead of re-opening the device four times.
#[derive(Debug, Clone)]
pub struct DeviceSnapshot {
    /// Marketing name reported by the device.
    pub name:        String,
    /// Battery state, when the battery feature answered.
    pub battery:     Option<BatteryInfo>,
    /// Currently applied DPI.
    pub dpi:         u16,
    /// Current `SmartShift` configuration.
    pub smartshift:  SmartShiftConfig,
    /// Current hi-res wheel configuration.
    pub hiresscroll: HiResScrollConfig
}

/// Opens the device once and reads every supported property.
///
/// # Returns
///
/// `Some` snapshot when the device opened, `None` when it is unreachable.
///
/// # Examples
///
/// ```no_run
/// # use logi_mx_driver::prelude::*;
/// // Mirrors the UI startup path: a single open, a single set of reads.
/// let mut device = MxMaster3s::open_bolt_receiver_discovered()?;
/// let name = device.device_name()?;
/// # Ok::<(), masterror::AppError>(())
/// ```
#[must_use]
pub fn collect_device_snapshot() -> Option<DeviceSnapshot> {
    if let Some(Response::Info {
        name,
        dpi,
        smartshift,
        hires
    }) = try_request(&Request::Info)
    {
        let battery = try_request(&Request::Battery).and_then(|r| match r {
            Response::Battery {
                level,
                status
            } => Some(BatteryInfo {
                level,
                status: parse_battery_status(&status)
            }),
            _ => None
        });

        return Some(DeviceSnapshot {
            name,
            battery,
            dpi,
            smartshift,
            hiresscroll: hires
        });
    }

    collect_device_snapshot_direct()
}

#[must_use]
pub fn parse_battery_status(status: &str) -> BatteryStatus {
    match status {
        "Charging" => BatteryStatus::Charging,
        "Full" => BatteryStatus::Full,
        "Discharging" => BatteryStatus::Discharging,
        _ => BatteryStatus::Unknown
    }
}

fn collect_device_snapshot_direct() -> Option<DeviceSnapshot> {
    let mut device = MxMaster3s::open_bolt_receiver_discovered().ok()?;

    let name = device
        .device_name()
        .unwrap_or_else(|_| DEFAULT_DEVICE_NAME.to_string());
    let battery = device.battery_info().ok();
    let dpi = device.dpi().unwrap_or(DEFAULT_DPI);
    let smartshift = device.smartshift().unwrap_or_default();
    let hiresscroll = device.hires_scroll().unwrap_or_default();

    Some(DeviceSnapshot {
        name,
        battery,
        dpi,
        smartshift,
        hiresscroll
    })
}
