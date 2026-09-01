// SPDX-FileCopyrightText: 2025 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! HID++ feature identifiers and per-feature function enumerations.
//!
//! The HID++ protocol resolves feature IDs to device-specific indices at
//! runtime; the enumerations here map to the documented function positions
//! within each feature.

use super::constants::{FEATURE_ROOT, ROOT_INDEX};

/// Pair of a static HID++ feature ID and its device-specific index.
///
/// # Examples
///
/// ```
/// use logi_mx_driver::hidpp::Feature;
///
/// let feature = Feature::new(0x2201, 0x07);
/// assert_eq!(feature.id, 0x2201);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Feature {
    /// Static feature ID from the HID++ specification.
    pub id:    u16,
    /// Runtime index resolved from the device feature table.
    pub index: u8
}

impl Feature {
    /// Builds a feature descriptor from an ID and a resolved index.
    ///
    /// # Examples
    ///
    /// ```
    /// use logi_mx_driver::hidpp::Feature;
    ///
    /// let feature = Feature::new(0x0005, 0x03);
    /// assert_eq!(feature.index, 0x03);
    /// ```
    #[must_use]
    pub const fn new(id: u16, index: u8) -> Self {
        Self {
            id,
            index
        }
    }

    /// Returns the Root feature descriptor used for discovery.
    ///
    /// # Examples
    ///
    /// ```
    /// use logi_mx_driver::hidpp::Feature;
    ///
    /// assert_eq!(Feature::root().id, 0x0000);
    /// ```
    #[must_use]
    pub const fn root() -> Self {
        Self::new(FEATURE_ROOT, ROOT_INDEX)
    }
}

/// Functions exposed by the Root feature (0x0000).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RootFunction {
    /// Resolves a feature ID to its index.
    GetFeature = 0x00,
    /// Protocol liveness probe.
    Ping = 0x01
}

/// Functions exposed by the Device Name feature (0x0005).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceNameFunction {
    /// Reads the total name length in bytes.
    GetNameLength = 0x00,
    /// Reads up to 16 name bytes starting at a single-byte character index.
    GetName = 0x01
}

/// Functions exposed by the legacy Battery Status feature (0x1000).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatteryStatusFunction {
    /// Reads the charge level and charging state.
    GetStatus = 0x00
}

/// Functions exposed by the Unified Battery feature (0x1004).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatteryUnifiedFunction {
    /// Reads battery capability flags and supported level count.
    GetCapabilities = 0x00,
    /// Reads the current charge level and state.
    GetStatus = 0x01
}

/// Functions exposed by the Adjustable DPI feature (0x2201).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DpiFunction {
    /// Reads the number of supported sensors.
    GetSensorCount = 0x00,
    /// Reads the DPI list supported by a sensor.
    GetSensorDpiList = 0x01,
    /// Reads the current DPI of a sensor.
    GetSensorDpi = 0x02,
    /// Applies a DPI setting to a sensor.
    SetSensorDpi = 0x03
}

/// Functions exposed by the `SmartShift` feature (0x2110).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmartShiftFunction {
    /// Reads the ratchet mode, auto-disengage threshold and default.
    GetStatus = 0x00,
    /// Applies the ratchet mode and auto-disengage threshold.
    SetStatus = 0x01
}

/// Functions exposed by the Hi-Res Wheel feature (0x2121).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HiresWheelFunction {
    /// Reads wheel capability flags.
    GetCapabilities = 0x00,
    /// Reads the wheel mode byte (target, resolution, invert bits).
    GetWheelMode = 0x01,
    /// Applies a wheel mode byte.
    SetWheelMode = 0x02,
    /// Reads the physical ratchet switch state.
    GetRatchetSwitchState = 0x03
}

/// Functions exposed by the Reprogrammable Controls feature (0x1B04).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReprogControlsFunction {
    /// Reads the number of reprogrammable controls.
    GetControlCount = 0x00,
    /// Reads metadata for the control at a zero-based index.
    GetControlInfo = 0x01,
    /// Reads the reporting configuration of one control by CID.
    GetControlReporting = 0x02,
    /// Applies a reporting configuration to one control by CID.
    SetControlReporting = 0x03
}

/// Functions exposed by the Thumb Wheel feature (0x2150).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThumbWheelFunction {
    /// Reads native and diverted resolution plus capability flags.
    GetConfig = 0x00,
    /// Applies diversion and direction settings.
    SetConfig = 0x01
}

/// Functions exposed by the `ChangeHost` feature (0x1814).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeHostFunction {
    /// Reads the host count and the currently active host index.
    GetHostInfo = 0x00,
    /// Switches to the host at a zero-based index.
    SetHost = 0x01
}

#[cfg(test)]
#[path = "features_tests.rs"]
mod feature_tests;
