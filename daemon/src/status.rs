// SPDX-FileCopyrightText: 2025-2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Shared device status snapshot.
//!
//! The device worker publishes state into a [`SharedStatus`] handle; the
//! system tray and other surfaces read it without owning device access.

use std::sync::{Arc, Mutex};

use logi_mx_driver::prelude::*;
use tracing::debug;

/// Battery percentage at or below which the tray and notifications warn.
#[cfg(any(feature = "tray", feature = "notify"))]
pub const BATTERY_LOW_PERCENT: u8 = 20;
/// Battery percentage at or below which the tray and notifications alert.
#[cfg(any(feature = "tray", feature = "notify"))]
pub const BATTERY_CRITICAL_PERCENT: u8 = 10;
/// Battery status string that makes charge alerts meaningful.
#[cfg(any(feature = "tray", feature = "notify"))]
pub const BATTERY_DISCHARGING: &str = "Discharging";

/// Snapshot of device state shared between the worker and UI surfaces.
///
/// Readers live behind the `tray` feature; without it the snapshot is only
/// exercised by tests.
#[cfg_attr(
    not(feature = "tray"),
    allow(dead_code, reason = "fields are read by the tray feature only")
)]
#[derive(Clone, Debug)]
pub struct DeviceStatus {
    /// Whether the mouse is currently reachable.
    pub connected:            bool,
    /// Last observed battery charge percentage.
    pub battery_level:        u8,
    /// Last observed battery charge state.
    pub battery_status:       String,
    /// Last observed DPI setting.
    pub dpi:                  u16,
    /// Whether `SmartShift` automatic switching is enabled.
    pub smartshift:           bool,
    /// `SmartShift` scroll-speed threshold.
    pub smartshift_threshold: u8,
    /// Last fatal error message, if any.
    pub error:                Option<String>
}

impl Default for DeviceStatus {
    fn default() -> Self {
        Self {
            connected:            false,
            battery_level:        0,
            battery_status:       "Unknown".to_string(),
            dpi:                  1000,
            smartshift:           false,
            smartshift_threshold: 20,
            error:                None
        }
    }
}

/// Shared handle around [`DeviceStatus`].
pub type SharedStatus = Arc<Mutex<DeviceStatus>>;

/// Device readings folded into a status refresh.
///
/// The worker owns exactly one HID++ handle per device; every surface reads
/// through it. A second handle on the same hidraw node would split the HID++
/// response stream between the two readers, so opening one per consumer is
/// forbidden by design.
#[derive(Debug, Default)]
pub struct DeviceReadings {
    /// Battery report, when the device answered.
    pub battery:    Option<BatteryInfo>,
    /// DPI setting, when the device answered.
    pub dpi:        Option<u16>,
    /// `SmartShift` configuration, when the device answered.
    pub smartshift: Option<SmartShiftConfig>
}

/// Refreshes the shared status from one device reading.
///
/// Publishes the snapshot built from the worker's single device handle.
/// `None` marks the device unreachable.
pub fn refresh(status: &SharedStatus, readings: Option<DeviceReadings>) {
    if let Some(readings) = readings {
        let (battery_level, dpi_value) = {
            let mut guard = status
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            guard.connected = true;
            guard.error = None;

            if let Some(battery) = readings.battery {
                guard.battery_level = battery.level;
                guard.battery_status = format!("{:?}", battery.status);
            } else {
                guard.battery_status = "Unknown".to_string();
            }
            if let Some(current_dpi) = readings.dpi {
                guard.dpi = current_dpi;
            }
            if let Some(ss_config) = readings.smartshift {
                guard.smartshift = ss_config.enabled;
                guard.smartshift_threshold = ss_config.threshold;
            }

            (guard.battery_level, guard.dpi)
        };

        debug!("Tray status refreshed: battery={battery_level}%, dpi={dpi_value}");
    } else {
        let mut guard = status
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        guard.connected = false;
        guard.error = Some("device not reachable".to_string());
        drop(guard);
        debug!("Device not connected");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_device_status_default() {
        let status = DeviceStatus::default();
        assert!(!status.connected);
        assert_eq!(status.battery_level, 0);
        assert_eq!(status.battery_status, "Unknown");
        assert_eq!(status.dpi, 1000);
        assert!(!status.smartshift);
        assert_eq!(status.smartshift_threshold, 20);
        assert!(status.error.is_none());
    }

    #[test]
    fn test_device_status_fields() {
        let status = DeviceStatus {
            connected:            true,
            battery_level:        95,
            battery_status:       "Discharging".to_string(),
            dpi:                  3200,
            smartshift:           true,
            smartshift_threshold: 30,
            error:                None
        };

        assert!(status.connected);
        assert_eq!(status.battery_level, 95);
        assert_eq!(status.battery_status, "Discharging");
        assert_eq!(status.dpi, 3200);
        assert!(status.smartshift);
        assert_eq!(status.smartshift_threshold, 30);
        assert!(status.error.is_none());
    }

    #[test]
    fn test_device_status_with_error() {
        let status = DeviceStatus {
            connected:            false,
            battery_level:        0,
            battery_status:       "Unknown".to_string(),
            dpi:                  1000,
            smartshift:           false,
            smartshift_threshold: 20,
            error:                Some("Test error".to_string())
        };
        assert_eq!(status.error, Some("Test error".to_string()));
    }

    #[test]
    fn test_shared_status_is_cloneable() {
        let status: SharedStatus = Arc::new(Mutex::new(DeviceStatus::default()));
        let handle = Arc::clone(&status);

        {
            let mut guard = status.lock().unwrap();
            guard.connected = true;
        }

        assert!(handle.lock().unwrap().connected);
    }
}
