// SPDX-FileCopyrightText: 2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

use super::*;
use crate::hidpp::packet::HidppPacket;

/// Builds a long notification report from a feature index and parameters.
fn long_notification(feature_index: u8, function_id: u8, params: &[u8]) -> HidppPacket {
    let mut parameters = [0u8; 16];
    parameters[..params.len()].copy_from_slice(params);
    HidppPacket::new_long(0xFF, feature_index, function_id, 0x00, parameters)
}

#[test]
fn test_pressed_gesture_button() {
    // Real MX Master 3S frame: gesture button 0x00C3 pressed while diverted.
    let packet = long_notification(0x09, 0x00, &[0x00, 0xC3]);
    assert_eq!(
        decode_reprog_notification(0x09, &packet),
        Some(ReprogNotification::DivertedButtons {
            pressed: vec![0x00C3]
        })
    );
}

#[test]
fn test_release_reports_no_pressed_controls() {
    let packet = long_notification(0x09, 0x00, &[]);
    assert_eq!(
        decode_reprog_notification(0x09, &packet),
        Some(ReprogNotification::DivertedButtons { pressed: vec![] })
    );
}

#[test]
fn test_raw_xy_positive_and_negative() {
    // Real MX Master 3S frames: initial burst dx=451 dy=22, then dx=-1 dy=0.
    let burst = long_notification(0x09, 0x01, &[0x01, 0xC3, 0x00, 0x16]);
    assert_eq!(
        decode_reprog_notification(0x09, &burst),
        Some(ReprogNotification::RawXy { dx: 451, dy: 22 })
    );

    let drift = long_notification(0x09, 0x01, &[0xFF, 0xFF, 0x00, 0x00]);
    assert_eq!(
        decode_reprog_notification(0x09, &drift),
        Some(ReprogNotification::RawXy { dx: -1, dy: 0 })
    );
}

#[test]
fn test_multiple_pressed_controls_reported_in_order() {
    let packet = long_notification(0x09, 0x00, &[0x00, 0xC3, 0x00, 0xC4]);
    assert_eq!(
        decode_reprog_notification(0x09, &packet),
        Some(ReprogNotification::DivertedButtons {
            pressed: vec![0x00C3, 0x00C4]
        })
    );
}

#[test]
fn test_rejects_short_reports() {
    let short = HidppPacket::new_short(0xFF, 0x09, 0x00, 0x00, [0; 3]);
    assert_eq!(decode_reprog_notification(0x09, &short), None);
}

#[test]
fn test_rejects_responses_and_other_features() {
    let response = HidppPacket::new_long(0xFF, 0x09, 0x00, 0x05, [0; 16]);
    assert_eq!(decode_reprog_notification(0x09, &response), None);

    let other = long_notification(0x0A, 0x00, &[0x00, 0xC3]);
    assert_eq!(decode_reprog_notification(0x09, &other), None);
}

#[test]
fn test_rejects_unknown_function() {
    let unknown = long_notification(0x09, 0x02, &[0; 4]);
    assert_eq!(decode_reprog_notification(0x09, &unknown), None);
}
