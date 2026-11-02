// SPDX-FileCopyrightText: 2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Swipe recognition for the diverted gesture button.
//!
//! The device streams raw pointer deltas while the gesture button is held;
//! [`SwipeAccumulator`] folds them into directional swipes using the tuning
//! Logi Options+ exhibits: a short hold gate rejects stray clicks, a dominant
//! axis threshold commits a direction mid-swipe, the cross-axis deadzone
//! rejects diagonals, and a hold that never commits is a click.
//!
//! All travel arithmetic saturates: a pathological hold must never panic
//! inside the input pump, only misfire.

use std::time::{Duration, Instant};

use logi_mx_driver::devices::GestureDirection;

/// Minimum dominant-axis travel (raw-XY units) before a direction commits.
pub const SWIPE_THRESHOLD: i32 = 50;
/// Minimum cross-axis allowance at the threshold; grows with the dominant
/// axis (`max(deadzone, 35%)`) so only reasonably straight swipes commit.
pub const SWIPE_DEADZONE: i32 = 40;
/// How long the button must be held before travel may commit, separating a
/// deliberate hold-and-swipe from a quick click whose cursor drifted.
pub const HOLD_FOR_SWIPE: Duration = Duration::from_millis(160);
/// Net travel continuing a committed direction before it fires again, so one
/// continuous motion steps through several targets without double-firing on
/// the overshoot of a single swipe.
pub const REPEAT_DISTANCE: i32 = 12 * SWIPE_THRESHOLD;
/// How long the pointer must rest mid-hold to end a stroke; the next swipe
/// then starts fresh and may repeat the last direction at the normal
/// threshold.
pub const REARM_PAUSE: Duration = Duration::from_millis(250);

/// Classifies accumulated raw-XY travel into a committed swipe direction.
///
/// The dominant axis must reach [`SWIPE_THRESHOLD`] while the cross axis
/// stays within `max(SWIPE_DEADZONE, 35% of dominant)`. Device convention
/// applies: `+x` right, `+y` down, so a negative `dy` is an upward swipe.
///
/// # Examples
///
/// ```
/// use logi_mx_daemon::gesture::detect_swipe;
/// use logi_mx_driver::devices::GestureDirection;
///
/// assert_eq!(detect_swipe(60, 5), Some(GestureDirection::Right));
/// assert_eq!(detect_swipe(10, 10), None);
/// ```
#[must_use]
pub fn detect_swipe(dx: i32, dy: i32) -> Option<GestureDirection> {
    let (abs_x, abs_y) = (dx.saturating_abs(), dy.saturating_abs());
    let dominant = abs_x.max(abs_y);
    if dominant < SWIPE_THRESHOLD {
        return None;
    }
    let cross_limit = SWIPE_DEADZONE.max(dominant.saturating_mul(35) / 100);
    if abs_x > abs_y {
        if abs_y > cross_limit {
            return None;
        }
        Some(if dx > 0 {
            GestureDirection::Right
        } else {
            GestureDirection::Left
        })
    } else {
        if abs_x > cross_limit {
            return None;
        }
        Some(if dy > 0 {
            GestureDirection::Down
        } else {
            GestureDirection::Up
        })
    }
}

/// Travel continuing `dir` in a raw-XY delta (positive keeps going).
const fn along(dir: GestureDirection, dx: i32, dy: i32) -> i32 {
    match dir {
        GestureDirection::Right => dx,
        GestureDirection::Left => dx.saturating_neg(),
        GestureDirection::Down => dy,
        GestureDirection::Up => dy.saturating_neg(),
        GestureDirection::None => 0,
    }
}

/// Travel sideways to `dir` in a raw-XY delta.
const fn across(dir: GestureDirection, dx: i32, dy: i32) -> i32 {
    match dir {
        GestureDirection::Right | GestureDirection::Left => dy,
        GestureDirection::Up | GestureDirection::Down => dx,
        GestureDirection::None => 0,
    }
}

/// Mid-hold state machine: `begin` on press, `accumulate` per raw-XY report,
/// `end` on release. Times are supplied by the caller so tests stay
/// deterministic.
#[derive(Debug, Default)]
pub struct SwipeAccumulator {
    /// When the current hold began; `None` when not holding.
    held_since: Option<Instant>,
    /// Travel accumulated since the last commit (saturating).
    dx: i32,
    dy: i32,
    /// Whether any direction already fired during this hold.
    fired: bool,
    /// Direction committed by the current stroke, if any.
    last: Option<GestureDirection>,
    /// Travel continuing `last` since it committed; fires again at
    /// [`REPEAT_DISTANCE`].
    repeat: i32,
    /// Sideways drift accompanying `repeat`.
    repeat_cross: i32,
    /// When the last motion arrived, to detect the re-arm pause.
    moved_at: Option<Instant>,
    /// Whether the MX Master 3S pre-press burst was already dropped.
    burst_dropped: bool,
}

impl SwipeAccumulator {
    /// Starts a fresh hold, clearing all travel state.
    pub fn begin(&mut self, now: Instant) {
        *self = Self {
            held_since: Some(now),
            ..Self::default()
        };
    }

    /// Whether a hold is in progress between `begin` and `end`.
    #[must_use]
    pub const fn is_holding(&self) -> bool {
        self.held_since.is_some()
    }

    /// Feeds one raw-XY report into the hold.
    ///
    /// Returns `Some(direction)` the instant travel commits (never before the
    /// hold gate passes), once per stroke; a committed direction fires again
    /// only after [`REPEAT_DISTANCE`] further travel in the same direction.
    /// The first report after `begin` is dropped: the MX Master 3S streams
    /// movement that happened before the press, which would otherwise
    /// hijack the gesture.
    pub fn accumulate(&mut self, dx: i32, dy: i32, now: Instant) -> Option<GestureDirection> {
        let started = self.held_since?;
        if !self.burst_dropped {
            self.burst_dropped = true;
            self.moved_at = Some(now);
            return None;
        }

        if self
            .moved_at
            .is_some_and(|t| now.saturating_duration_since(t) >= REARM_PAUSE)
        {
            if i64::from(self.dx) * i64::from(dx) + i64::from(self.dy) * i64::from(dy) < 0 {
                self.dx = 0;
                self.dy = 0;
            }
            self.last = None;
            self.repeat = 0;
            self.repeat_cross = 0;
        }
        self.moved_at = Some(now);

        if let Some(dir) = self.last {
            let continuing = along(dir, dx, dy);
            let drift = across(dir, dx, dy);
            if continuing > 0 {
                self.repeat = self.repeat.saturating_add(continuing);
                self.repeat_cross = self.repeat_cross.saturating_add(drift);
                match dir {
                    GestureDirection::Right | GestureDirection::Left => {
                        self.dy = self.dy.saturating_add(dy);
                    }
                    GestureDirection::Up | GestureDirection::Down => {
                        self.dx = self.dx.saturating_add(dx);
                    }
                    GestureDirection::None => {}
                }
            } else {
                self.repeat = self.repeat.saturating_add(continuing).max(0);
                if self.repeat == 0 {
                    self.repeat_cross = 0;
                }
                self.dx = self.dx.saturating_add(dx);
                self.dy = self.dy.saturating_add(dy);
            }

            if self.repeat >= REPEAT_DISTANCE
                && detect_swipe(self.repeat, self.repeat_cross) == Some(GestureDirection::Right)
            {
                self.repeat = self.repeat.saturating_sub(REPEAT_DISTANCE);
                self.repeat_cross = 0;
                self.dx = 0;
                self.dy = 0;
                self.fired = true;
                return Some(dir);
            }
        } else {
            self.dx = self.dx.saturating_add(dx);
            self.dy = self.dy.saturating_add(dy);
        }

        if now.saturating_duration_since(started) < HOLD_FOR_SWIPE {
            return None;
        }
        let committed = detect_swipe(self.dx, self.dy)?;
        if self.last == Some(committed) {
            return None;
        }
        self.dx = 0;
        self.dy = 0;
        self.last = Some(committed);
        self.repeat = 0;
        self.repeat_cross = 0;
        self.fired = true;
        Some(committed)
    }

    /// Ends the hold and reports the direction to fire on release.
    ///
    /// A hold whose travel never committed mid-swipe is classified once more
    /// from the accumulated totals, so a quick flick still lands; travel
    /// below the threshold is a click ([`GestureDirection::None`]). A hold
    /// that already fired mid-swipe reports `None`.
    pub fn end(&mut self) -> Option<GestureDirection> {
        self.held_since.take()?;
        let fired = self.fired;
        let settled = detect_swipe(self.dx, self.dy);
        *self = Self::default();
        if fired {
            return None;
        }
        Some(settled.unwrap_or(GestureDirection::None))
    }
}

#[cfg(test)]
#[path = "gesture_tests.rs"]
mod gesture_tests;
