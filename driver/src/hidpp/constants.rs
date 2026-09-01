// SPDX-FileCopyrightText: 2025 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Static HID++ constants: report IDs, packet sizes, feature IDs and the
//! HID++ 2.0 error code table.

/// Report ID of a short HID++ report (7 bytes).
pub const REPORT_ID_SHORT: u8 = 0x10;
/// Report ID of a long HID++ report (20 bytes).
pub const REPORT_ID_LONG: u8 = 0x11;
/// Total size of a short report on the wire.
pub const SHORT_PACKET_SIZE: usize = 7;
/// Total size of a long report on the wire.
pub const LONG_PACKET_SIZE: usize = 20;

/// Feature ID of the Root feature, used for discovery and liveness checks.
pub const FEATURE_ROOT: u16 = 0x0000;
/// Fixed device index of the Root feature; it is always index zero.
pub const ROOT_INDEX: u8 = 0x00;
/// Feature ID of the device name feature (0x0005).
pub const FEATURE_DEVICE_NAME: u16 = 0x0005;
/// Feature ID of the legacy battery status feature (0x1000).
pub const FEATURE_BATTERY_STATUS: u16 = 0x1000;
/// Feature ID of the unified battery feature (0x1004).
pub const FEATURE_UNIFIED_BATTERY: u16 = 0x1004;
/// Feature ID of the `ChangeHost` (Easy-Switch) feature (0x1814).
pub const FEATURE_CHANGE_HOST: u16 = 0x1814;
/// Feature ID of the Reprogrammable Controls feature (0x1B04).
pub const FEATURE_REPROG_CONTROLS: u16 = 0x1B04;
/// Feature ID of the `SmartShift` feature (0x2110).
pub const FEATURE_SMART_SHIFT: u16 = 0x2110;
/// Feature ID of the Hi-Res Wheel feature (0x2121).
pub const FEATURE_HIRES_WHEEL: u16 = 0x2121;
/// Feature ID of the Thumb Wheel feature (0x2150).
pub const FEATURE_THUMB_WHEEL: u16 = 0x2150;
/// Feature ID of the Adjustable DPI feature (0x2201).
pub const FEATURE_ADJUSTABLE_DPI: u16 = 0x2201;

/// HID++ 2.0: the request completed successfully.
pub const ERROR_NO_ERROR: u8 = 0x00;
/// HID++ 2.0: unknown error.
pub const ERROR_UNKNOWN: u8 = 0x01;
/// HID++ 2.0: an argument was invalid.
pub const ERROR_INVALID_ARGUMENT: u8 = 0x02;
/// HID++ 2.0: a value was outside the allowed range.
pub const ERROR_OUT_OF_RANGE: u8 = 0x03;
/// HID++ 2.0: hardware failure; commands sent too fast also surface here.
pub const ERROR_HW_ERROR: u8 = 0x04;
/// HID++ 2.0: Logitech internal error.
pub const ERROR_LOGITECH_INTERNAL: u8 = 0x05;
/// HID++ 2.0: the referenced feature index does not exist.
pub const ERROR_INVALID_FEATURE_INDEX: u8 = 0x06;
/// HID++ 2.0: the referenced function ID does not exist.
pub const ERROR_INVALID_FUNCTION_ID: u8 = 0x07;
/// HID++ 2.0: the device is busy; the command should be retried.
pub const ERROR_BUSY: u8 = 0x08;
/// HID++ 2.0: the feature or function is not supported.
pub const ERROR_UNSUPPORTED: u8 = 0x09;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_report_ids() {
        assert_eq!(REPORT_ID_SHORT, 0x10);
        assert_eq!(REPORT_ID_LONG, 0x11);
        assert_eq!(SHORT_PACKET_SIZE, 7);
        assert_eq!(LONG_PACKET_SIZE, 20);
    }

    #[test]
    fn test_feature_ids() {
        assert_eq!(FEATURE_ROOT, 0x0000);
        assert_eq!(FEATURE_DEVICE_NAME, 0x0005);
        assert_eq!(FEATURE_BATTERY_STATUS, 0x1000);
        assert_eq!(FEATURE_UNIFIED_BATTERY, 0x1004);
        assert_eq!(FEATURE_CHANGE_HOST, 0x1814);
        assert_eq!(FEATURE_REPROG_CONTROLS, 0x1B04);
        assert_eq!(FEATURE_SMART_SHIFT, 0x2110);
        assert_eq!(FEATURE_HIRES_WHEEL, 0x2121);
        assert_eq!(FEATURE_THUMB_WHEEL, 0x2150);
        assert_eq!(FEATURE_ADJUSTABLE_DPI, 0x2201);
    }

    #[test]
    fn test_error_codes() {
        assert_eq!(ERROR_NO_ERROR, 0x00);
        assert_eq!(ERROR_UNKNOWN, 0x01);
        assert_eq!(ERROR_INVALID_ARGUMENT, 0x02);
        assert_eq!(ERROR_OUT_OF_RANGE, 0x03);
        assert_eq!(ERROR_HW_ERROR, 0x04);
        assert_eq!(ERROR_LOGITECH_INTERNAL, 0x05);
        assert_eq!(ERROR_INVALID_FEATURE_INDEX, 0x06);
        assert_eq!(ERROR_INVALID_FUNCTION_ID, 0x07);
        assert_eq!(ERROR_BUSY, 0x08);
        assert_eq!(ERROR_UNSUPPORTED, 0x09);
    }
}
