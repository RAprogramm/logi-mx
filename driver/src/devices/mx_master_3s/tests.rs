// SPDX-FileCopyrightText: 2025-2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Tests for the MX Master 3S wire codecs, identifiers and button mapping.

use super::{
    open::{
        PID_BOLT_RECEIVER, PID_MX_MASTER_3S_BT, PID_MX_MASTER_3S_USB, RECEIVER_SLOTS, VID_LOGITECH
    },
    *
};
use crate::devices::traits::BatteryStatus;

#[test]
fn test_constants() {
    assert_eq!(VID_LOGITECH, 0x046D);
    assert_eq!(PID_BOLT_RECEIVER, 0xC548);
    assert_eq!(PID_MX_MASTER_3S_USB, 0x4082);
    assert_eq!(PID_MX_MASTER_3S_BT, 0xB034);
    assert_eq!(RECEIVER_SLOTS.len(), 6);
}

#[test]
fn test_validate_smartshift_accepts_documented_range() {
    assert!(
        validate_smartshift(&SmartShiftConfig {
            enabled:   false,
            threshold: 0
        })
        .is_ok()
    );
    assert!(
        validate_smartshift(&SmartShiftConfig {
            enabled:   true,
            threshold: 50
        })
        .is_ok()
    );
}

#[test]
fn test_validate_smartshift_rejects_out_of_range_threshold() {
    let error = validate_smartshift(&SmartShiftConfig {
        enabled:   true,
        threshold: 51
    })
    .expect_err("51 is above the device maximum");
    assert!(
        error.to_string().contains("50"),
        "the error names the limit"
    );
}

#[test]
fn test_parse_battery_status() {
    assert_eq!(parse_battery_status(0), BatteryStatus::Discharging);
    assert_eq!(parse_battery_status(1), BatteryStatus::Charging);
    assert_eq!(parse_battery_status(2), BatteryStatus::Charging);
    assert_eq!(parse_battery_status(3), BatteryStatus::Full);
    assert_eq!(parse_battery_status(4), BatteryStatus::Full);
    assert_eq!(parse_battery_status(5), BatteryStatus::Unknown);
    assert_eq!(parse_battery_status(0xFF), BatteryStatus::Unknown);
}

#[test]
fn test_parse_smartshift() {
    let config = parse_smartshift(&[0x02, 0x0A, 0x0A]);
    assert!(config.enabled);
    assert_eq!(config.threshold, 10);

    let disabled = parse_smartshift(&[0x02, 0x00, 0x0A]);
    assert!(!disabled.enabled);

    let freespin = parse_smartshift(&[0x01, 0x0A, 0x0A]);
    assert!(!freespin.enabled);

    let empty = parse_smartshift(&[]);
    assert!(!empty.enabled);
}

#[test]
fn test_encode_smartshift() {
    let params = encode_smartshift(SmartShiftConfig {
        enabled:   true,
        threshold: 10
    });
    assert_eq!(params, [0x02, 0x0A, 0x00]);

    let disabled = encode_smartshift(SmartShiftConfig {
        enabled:   false,
        threshold: 10
    });
    assert_eq!(disabled, [0x02, 0x00, 0x00]);

    let zero_threshold = encode_smartshift(SmartShiftConfig {
        enabled:   true,
        threshold: 0
    });
    assert_eq!(zero_threshold, [0x02, 0x00, 0x00]);
}

#[test]
fn test_hires_mode_roundtrip() {
    let config = parse_hires_mode(0x06);
    assert!(config.enabled);
    assert!(config.inverted);

    let native = parse_hires_mode(0x00);
    assert!(!native.enabled);
    assert!(!native.inverted);

    let encoded = encode_hires_mode(0x00, config);
    assert_eq!(encoded, 0x06);

    let target_preserved = encode_hires_mode(0x01, config);
    assert_eq!(target_preserved, 0x07);
}

#[test]
fn test_hires_invert_only() {
    let inverted = encode_hires_mode(
        0x02,
        HiResScrollConfig {
            enabled:  true,
            inverted: true
        }
    );
    assert_eq!(inverted, 0x06);

    let restored = encode_hires_mode(
        0x06,
        HiResScrollConfig {
            enabled:  true,
            inverted: false
        }
    );
    assert_eq!(restored, 0x02);
}

#[test]
fn test_append_name_chunk() {
    let mut name = String::new();
    let consumed = append_name_chunk(&mut name, b"MX Master 3S For", 25);
    assert_eq!(consumed, 16);
    assert_eq!(name, "MX Master 3S For");

    let consumed = append_name_chunk(&mut name, b" Business\0\0\0\0\0\0\0\0", 9);
    assert_eq!(consumed, 9);
    assert_eq!(name, "MX Master 3S For Business");
}

#[test]
fn test_append_name_chunk_padding() {
    let mut name = String::new();
    let consumed = append_name_chunk(&mut name, b"MX Master 3S\0\0\0\0", 12);
    assert_eq!(consumed, 12);
    assert_eq!(name, "MX Master 3S");
}

#[test]
fn test_append_name_chunk_empty() {
    let mut name = String::new();
    let consumed = append_name_chunk(&mut name, &[], 10);
    assert_eq!(consumed, 0);
    assert_eq!(name, "");
}

#[test]
fn test_control_id_names() {
    assert_eq!(control_id_name(0x0050), "Left Click");
    assert_eq!(control_id_name(0x0051), "Right Click");
    assert_eq!(control_id_name(0x0052), "Middle Click");
    assert_eq!(control_id_name(0x0053), "Back");
    assert_eq!(control_id_name(0x0056), "Forward");
    assert_eq!(control_id_name(0x00C3), "Gesture Button");
    assert_eq!(control_id_name(0x00C4), "SmartShift Toggle");
    assert_eq!(control_id_name(0x00D7), "Unknown Control");
}

#[test]
fn test_button_mapping() {
    let mut mappings = HashMap::new();
    mappings.insert(ButtonId::ThumbGesture, Action::ToggleSmartShift);
    assert!(mappings.contains_key(&ButtonId::ThumbGesture));
}
