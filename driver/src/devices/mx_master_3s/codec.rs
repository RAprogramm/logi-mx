// SPDX-FileCopyrightText: 2025-2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Pure wire-format codecs: every byte-level encoding and decoding the
//! device protocol needs, testable without hardware.

use masterror::prelude::*;

use crate::{
    devices::traits::{BatteryStatus, HiResScrollConfig, SmartShiftConfig},
    error::Result
};

/// Hi-res wheel mode byte: high-resolution reporting bit.
const HIRES_MODE_HIRES_BIT: u8 = 0x02;
/// Hi-res wheel mode byte: inverted direction bit.
const HIRES_MODE_INVERT_BIT: u8 = 0x04;

/// `SmartShift` ratchet mode with automatic disengage.
const SMARTSHIFT_MODE_RATCHET_AUTO: u8 = 0x02;
/// Largest scroll-speed threshold the `SmartShift` feature accepts (0-50).
const SMARTSHIFT_THRESHOLD_MAX: u8 = 50;

/// Maps a battery status byte to the [`BatteryStatus`] enum.
///
/// Unified battery (0x1004) encodes `0` discharging, `1-3` charging and
/// `4` fully charged; the legacy 0x1000 feature uses the same low values.
#[must_use]
pub const fn parse_battery_status(status_byte: u8) -> BatteryStatus {
    match status_byte {
        0 => BatteryStatus::Discharging,
        1 | 2 => BatteryStatus::Charging,
        3 | 4 => BatteryStatus::Full,
        _ => BatteryStatus::Unknown
    }
}

/// Parses a `SmartShift` status response into a [`SmartShiftConfig`].
///
/// Response layout: `[mode, autoDisengage, default]`; mode `2` means the
/// ratchet mode with automatic disengage, and a non-zero auto-disengage
/// threshold enables the automatic switching behaviour.
#[must_use]
pub fn parse_smartshift(parameters: &[u8]) -> SmartShiftConfig {
    let mode = parameters.first().copied().unwrap_or(0);
    let auto_disengage = parameters.get(1).copied().unwrap_or(0);

    if mode != SMARTSHIFT_MODE_RATCHET_AUTO || auto_disengage == 0 {
        return SmartShiftConfig {
            enabled:   false,
            threshold: 0
        };
    }

    SmartShiftConfig {
        enabled:   true,
        threshold: auto_disengage
    }
}

/// Encodes a `SmartShift` `SetStatus` payload.
///
/// The wheel always stays in ratchet mode; `enabled` toggles the automatic
/// disengage threshold.
#[must_use]
pub const fn encode_smartshift(config: SmartShiftConfig) -> [u8; 3] {
    let auto_disengage = if config.enabled && config.threshold > 0 {
        config.threshold
    } else {
        0
    };

    [SMARTSHIFT_MODE_RATCHET_AUTO, auto_disengage, 0x00]
}

/// Rejects a `SmartShift` configuration the device firmware cannot accept.
///
/// # Arguments
///
/// * `config` - Enable flag and scroll-speed threshold to validate.
///
/// # Returns
///
/// `Ok(())` when the threshold is within the documented `0..=50` range,
/// a validation error naming the limit otherwise.
///
/// # Errors
///
/// [`AppError::validation`] when `config.threshold` exceeds the device
/// maximum.
pub fn validate_smartshift(config: &SmartShiftConfig) -> Result<()> {
    if config.threshold > SMARTSHIFT_THRESHOLD_MAX {
        return Err(AppError::validation(format!(
            "SmartShift threshold {} exceeds the device maximum {SMARTSHIFT_THRESHOLD_MAX}",
            config.threshold
        )));
    }
    Ok(())
}

/// Parses a hi-res wheel mode byte into a [`HiResScrollConfig`].
#[must_use]
pub const fn parse_hires_mode(mode: u8) -> HiResScrollConfig {
    HiResScrollConfig {
        enabled:  mode & HIRES_MODE_HIRES_BIT != 0,
        inverted: mode & HIRES_MODE_INVERT_BIT != 0
    }
}

/// Encodes a hi-res wheel mode byte from the current mode and a config.
///
/// The target and analytics bits of the current mode are preserved so the
/// change only touches resolution and inversion.
#[must_use]
pub const fn encode_hires_mode(current: u8, config: HiResScrollConfig) -> u8 {
    let mut mode = current & !(HIRES_MODE_HIRES_BIT | HIRES_MODE_INVERT_BIT);

    if config.enabled {
        mode |= HIRES_MODE_HIRES_BIT;
    }

    if config.inverted {
        mode |= HIRES_MODE_INVERT_BIT;
    }

    mode
}

/// Appends one device-name chunk to `name` and returns bytes consumed.
///
/// The chunk is truncated to the remaining name length; a NUL byte inside
/// the slice ends the name early, matching how devices pad the payload.
///
/// # Arguments
///
/// * `name` - Accumulated name to extend.
/// * `chunk` - Raw name bytes returned by the device.
/// * `remaining` - Bytes still expected according to the reported length.
///
/// # Returns
///
/// Number of chunk bytes consumed (before truncation at the remaining
/// budget, and never past a NUL terminator).
///
/// # Examples
///
/// ```
/// use logi_mx_driver::devices::mx_master_3s::append_name_chunk;
///
/// let mut name = String::new();
/// let consumed = append_name_chunk(&mut name, b"MX Master 3S For", 25);
/// assert_eq!(consumed, 16);
/// assert_eq!(name, "MX Master 3S For");
/// ```
pub fn append_name_chunk(name: &mut String, chunk: &[u8], remaining: usize) -> usize {
    let take = chunk.len().min(remaining);

    for (index, &byte) in chunk[..take].iter().enumerate() {
        if byte == 0 {
            name.push_str(&String::from_utf8_lossy(&chunk[..index]));
            return index;
        }
    }

    name.push_str(&String::from_utf8_lossy(&chunk[..take]));
    take
}
