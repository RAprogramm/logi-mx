// SPDX-FileCopyrightText: 2025 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Shared device status snapshot.
//!
//! The device worker publishes state into a [`SharedStatus`] handle; the
//! system tray and other surfaces read it without owning device access.

use std::sync::{Arc, Mutex};

/// Snapshot of device state shared between the worker and UI surfaces.
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
