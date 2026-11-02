// SPDX-FileCopyrightText: 2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Decoding of unsolicited HID++ notifications from the Reprogrammable
//! Controls feature (`0x1B04`).
//!
//! Once a control is diverted, the device stops emitting its normal HID
//! events and streams notifications instead: function `0x00` reports the set
//! of currently pressed diverted control IDs, function `0x01` streams raw
//! pointer deltas while a raw-XY-capable control is held. Both arrive as long
//! reports with `software_id` zero.
//!
//! Wire layouts (verified against Solaar device logs for the MX Master 3S):
//!
//! * pressed: parameters `C3 00 | 00 00 | ...` — up to four big-endian u16
//!   control IDs, zero-padded; an all-zero report means every diverted control
//!   was released;
//! * raw XY: parameters `01 C3 | 00 16 | ...` — big-endian signed `dx` at
//!   offset 0 and `dy` at offset 2, in device convention (`+x` right, `+y`
//!   down).

use super::packet::HidppPacket;

/// A notification emitted by the Reprogrammable Controls feature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReprogNotification {
    /// The diverted controls currently held down; empty means all released.
    DivertedButtons {
        /// Control IDs reported as pressed, in wire order.
        pressed: Vec<u16>,
    },
    /// A raw pointer delta streamed while a raw-XY control is held.
    RawXy {
        /// Horizontal movement, positive to the right.
        dx: i16,
        /// Vertical movement, positive downwards.
        dy: i16,
    },
}

/// Decodes a packet into a [`ReprogNotification`].
///
/// Returns `None` for anything that is not an unsolicited (`software_id` zero)
/// long report from the given feature index, and for functions without a
/// known layout.
///
/// # Arguments
///
/// * `feature_index` - Runtime index of feature `0x1B04` on this device.
/// * `packet` - Parsed report to inspect.
///
/// # Examples
///
/// ```
/// use logi_mx_driver::hidpp::{HidppPacket, ReprogNotification, decode_reprog_notification};
///
/// let pressed = HidppPacket::from_bytes(&[
///     0x11, 0xFF, 0x09, 0x00, 0x00, 0xC3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0
/// ])
/// .unwrap();
/// assert_eq!(
///     decode_reprog_notification(0x09, &pressed),
///     Some(ReprogNotification::DivertedButtons {
///         pressed: vec![0x00C3]
///     })
/// );
/// ```
#[must_use]
pub fn decode_reprog_notification(
    feature_index: u8,
    packet: &HidppPacket,
) -> Option<ReprogNotification> {
    let HidppPacket::Long(report) = packet else {
        return None;
    };
    if report.software_id != 0 || report.feature_index != feature_index {
        return None;
    }

    match report.function_id {
        0x00 => Some(ReprogNotification::DivertedButtons {
            pressed: parse_pressed(&report.parameters),
        }),
        0x01 => Some(ReprogNotification::RawXy {
            dx: i16::from_be_bytes([report.parameters[0], report.parameters[1]]),
            dy: i16::from_be_bytes([report.parameters[2], report.parameters[3]]),
        }),
        _ => None,
    }
}

/// Extracts the non-zero control IDs from a diverted-buttons payload.
fn parse_pressed(parameters: &[u8; 16]) -> Vec<u16> {
    parameters
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
        .filter(|cid| *cid != 0)
        .collect()
}

#[cfg(test)]
#[path = "notifications_tests.rs"]
mod notifications_tests;
