// SPDX-FileCopyrightText: 2025-2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Pure wire-format codecs: every byte-level encoding and decoding the
//! device protocol needs, testable without hardware.

use masterror::prelude::*;

use crate::{
    devices::traits::{BatteryStatus, HiResScrollConfig, SmartShiftConfig, WheelMode},
    error::Result
};

/// Hi-res wheel mode byte: high-resolution reporting bit.
const HIRES_MODE_HIRES_BIT: u8 = 0x02;
/// Hi-res wheel mode byte: inverted direction bit.
const HIRES_MODE_INVERT_BIT: u8 = 0x04;

/// `SmartShift` wheel mode byte: frictionless free-spin.
const WHEEL_MODE_FREESPIN: u8 = 0x01;
/// `SmartShift` wheel mode byte: clicky ratchet.
const WHEEL_MODE_RATCHET: u8 = 0x02;
/// `SmartShift` auto-disengage byte that keeps the ratchet engaged forever.
const SMARTSHIFT_DISENGAGE_NEVER: u8 = 0xFF;
/// Largest scroll-speed threshold the `SmartShift` feature accepts (1-50).
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
/// Response layout: `[wheelMode, autoDisengage, torque]`. The
/// auto-disengage byte carries the threshold in `1..=0xFE`; `0x00` is the
/// "do not change" sentinel and `0xFF` keeps the ratchet engaged
/// permanently, so only `1..=0xFE` means the automatic switching is active.
#[must_use]
pub fn parse_smartshift(parameters: &[u8]) -> SmartShiftConfig {
    let auto_disengage = parameters.get(1).copied().unwrap_or(0);

    if auto_disengage == 0 || auto_disengage == SMARTSHIFT_DISENGAGE_NEVER {
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

/// Maps a `SmartShift` wheel mode byte to a [`WheelMode`].
///
/// Anything other than the free-spin byte is treated as ratchet, the
/// firmware's safe clicky default.
#[must_use]
pub const fn parse_wheel_mode(mode_byte: u8) -> WheelMode {
    if mode_byte == WHEEL_MODE_FREESPIN {
        WheelMode::FreeSpin
    } else {
        WheelMode::Ratchet
    }
}

/// Encodes a [`WheelMode`] as the `SmartShift` wheel mode byte.
#[must_use]
pub const fn encode_wheel_mode(mode: WheelMode) -> u8 {
    match mode {
        WheelMode::FreeSpin => WHEEL_MODE_FREESPIN,
        WheelMode::Ratchet => WHEEL_MODE_RATCHET
    }
}

/// Encodes a `SmartShift` `SetStatus` payload from a config.
///
/// The wheel mode and tunable torque bytes of the current status are
/// preserved so applying a threshold never disturbs the physical ratchet
/// state; disabling writes the permanent-ratchet byte instead of the
/// "do not change" sentinel `0`, which the device would silently ignore.
#[must_use]
pub fn encode_smartshift(current: &[u8], config: SmartShiftConfig) -> [u8; 3] {
    let mode = match current.first().copied() {
        Some(WHEEL_MODE_FREESPIN) => WHEEL_MODE_FREESPIN,
        _ => WHEEL_MODE_RATCHET
    };
    let torque = current.get(2).copied().unwrap_or(0);
    let auto_disengage = if config.enabled {
        config.threshold.max(1)
    } else {
        SMARTSHIFT_DISENGAGE_NEVER
    };

    [mode, auto_disengage, torque]
}

/// Encodes a wheel-mode flip, preserving the auto-disengage and torque
/// bytes of the current status.
///
/// A zero auto-disengage read back is the "do not change" sentinel and
/// cannot be re-sent as a target, so it is normalised to the
/// permanent-ratchet byte.
#[must_use]
pub fn encode_wheel_mode_flip(current: &[u8], mode: WheelMode) -> [u8; 3] {
    let auto_disengage = match current.get(1).copied().unwrap_or(0) {
        0 => SMARTSHIFT_DISENGAGE_NEVER,
        value => value
    };
    let torque = current.get(2).copied().unwrap_or(0);

    [encode_wheel_mode(mode), auto_disengage, torque]
}

/// Rejects a `SmartShift` configuration the device firmware cannot accept.
///
/// # Arguments
///
/// * `config` - Enable flag and scroll-speed threshold to validate.
///
/// # Returns
///
/// `Ok(())` when the threshold is within the documented `1..=50` range
/// while enabled, a validation error naming the limit otherwise.
///
/// # Errors
///
/// [`AppError::validation`] when an enabled config carries a threshold
/// outside the device limits; a disabled config accepts any threshold
/// because the byte is not sent.
pub fn validate_smartshift(config: &SmartShiftConfig) -> Result<()> {
    if config.enabled && (config.threshold == 0 || config.threshold > SMARTSHIFT_THRESHOLD_MAX) {
        return Err(AppError::validation(format!(
            "SmartShift threshold {} is outside the device range 1..={SMARTSHIFT_THRESHOLD_MAX}",
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
