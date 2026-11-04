// SPDX-FileCopyrightText: 2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Pure wedge geometry of the pie ring.
//!
//! Equal wedges, slot 0 centred at 12 o'clock, indices running clockwise.
//! Every function is total and allocation-free so the state machine and
//! the renderer share one source of truth for where each wedge lives.

use logi_mx_driver::prelude::GestureDirection;

/// Compass angle of a gesture direction in cairo screen coordinates
/// (0 along +x, clockwise because y grows downward), `None` for the
/// non-directional click binding.
///
/// # Examples
///
/// ```
/// use logi_mx_daemon::pie::geometry::direction_angle;
/// use logi_mx_driver::devices::GestureDirection;
///
/// assert_eq!(direction_angle(GestureDirection::Right), Some(0.0));
/// assert!(direction_angle(GestureDirection::Up).is_some());
/// assert_eq!(direction_angle(GestureDirection::None), None);
/// ```
#[must_use]
pub fn direction_angle(direction: GestureDirection) -> Option<f64> {
    let steps: f64 = match direction {
        GestureDirection::Right => 0.0,
        GestureDirection::DownRight => 1.0,
        GestureDirection::Down => 2.0,
        GestureDirection::DownLeft => 3.0,
        GestureDirection::Left => 4.0,
        GestureDirection::UpLeft => 5.0,
        GestureDirection::Up => 6.0,
        GestureDirection::UpRight => 7.0,
        GestureDirection::None => return None
    };
    Some(steps * std::f64::consts::FRAC_PI_4)
}

/// Centre angle of wedge `index` out of `count` equal wedges.
///
/// Wedge 0 points at 12 o'clock (cairo `-π/2`) and the indices run
/// clockwise, matching the classic pie-menu convention so muscle memory
/// transfers between layouts.
///
/// # Examples
///
/// ```
/// use std::f64::consts::FRAC_PI_2;
///
/// use logi_mx_daemon::pie::geometry::wedge_angle;
///
/// assert!(
///     (wedge_angle(0, 4) + FRAC_PI_2).abs() < 1e-9,
///     "slot 0 points up"
/// );
/// ```
#[must_use]
pub fn wedge_angle(index: usize, count: usize) -> f64 {
    let wedges = f64::from(u32::try_from(count.max(1)).unwrap_or(u32::MAX));
    let step = f64::from(u32::try_from(index).unwrap_or(0));
    -std::f64::consts::FRAC_PI_2 + step * std::f64::consts::TAU / wedges
}

/// The wedge `(start, end)` of slot `index` out of `count` equal wedges,
/// in cairo radians with `end − start == 360° / count`.
///
/// # Examples
///
/// ```
/// use logi_mx_daemon::pie::geometry::wedge_for;
///
/// let (start, end) = wedge_for(0, 4);
/// assert!(end > start, "the wedge runs clockwise from its start edge");
/// ```
#[must_use]
#[allow(
    dead_code,
    reason = "wedge geometry is consumed by the overlay and HUD crates"
)]
pub fn wedge_for(index: usize, count: usize) -> (f64, f64) {
    let wedges = f64::from(u32::try_from(count.max(1)).unwrap_or(u32::MAX));
    let half = std::f64::consts::PI / wedges;
    let centre = wedge_angle(index, count);
    (centre - half, centre + half)
}

/// The wedge a screen vector falls into: the angle is measured clockwise
/// from 12 o'clock and rounded to the nearest wedge centre, so every point
/// of the circle selects exactly one slot.
///
/// # Arguments
///
/// * `dx`, `dy` - Cursor offset from the anchor in screen pixels.
/// * `count` - Number of equal wedges, at least 1.
///
/// # Returns
///
/// The wedge index, or `None` for the zero vector (the cursor sits on the
/// anchor and selects nothing).
///
/// # Examples
///
/// ```
/// use logi_mx_daemon::pie::geometry::slot_index;
///
/// assert_eq!(slot_index(0, -50, 4), Some(0), "straight up is slot 0");
/// assert_eq!(
///     slot_index(50, 0, 4),
///     Some(1),
///     "right is slot 1 of a four-ring"
/// );
/// assert_eq!(
///     slot_index(0, 0, 4),
///     None,
///     "the anchor itself selects nothing"
/// );
/// ```
#[must_use]
pub fn slot_index(dx: i32, dy: i32, count: usize) -> Option<usize> {
    if dx == 0 && dy == 0 {
        return None;
    }
    slot_for_angle(f64::from(dy).atan2(f64::from(dx)), count)
}

/// The wedge a compass angle falls into, measured like [`slot_index`].
#[must_use]
pub fn slot_for_angle(angle: f64, count: usize) -> Option<usize> {
    if count == 0 {
        return None;
    }
    let from_top = angle + std::f64::consts::FRAC_PI_2;
    let step = std::f64::consts::TAU / f64::from(u32::try_from(count).unwrap_or(u32::MAX));
    let rounded = (from_top.rem_euclid(std::f64::consts::TAU) / step).round();
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "the quotient lies in [0, count) before rounding, so the rounded index is a small non-negative integer"
    )]
    let index = rounded as usize;
    Some(index % count)
}

/// Wraps an angle difference into `0..=π`, the shorter way around the circle.
#[must_use]
pub fn wrapped_distance(angle: f64, centre: f64) -> f64 {
    let tau = std::f64::consts::TAU;
    let diff = (angle - centre).rem_euclid(tau);
    diff.min(tau - diff)
}

/// Whether a cursor vector left the activation circle: strictly further than
/// `radius` from the anchor, in saturating integer arithmetic.
#[must_use]
pub fn beyond_radius(dx: i32, dy: i32, radius: i32) -> bool {
    let (abs_x, abs_y) = (
        i64::from(dx.saturating_abs()),
        i64::from(dy.saturating_abs())
    );
    let radius = i64::from(radius.max(0));
    abs_x * abs_x + abs_y * abs_y > radius * radius
}

#[cfg(test)]
#[path = "geometry_tests.rs"]
mod tests;
