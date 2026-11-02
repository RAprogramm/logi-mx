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

use logi_mx_driver::prelude::*;
use tracing::{debug, info, warn};

use crate::inject::Injector;

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
        GestureDirection::None => 0
    }
}

/// Travel sideways to `dir` in a raw-XY delta.
const fn across(dir: GestureDirection, dx: i32, dy: i32) -> i32 {
    match dir {
        GestureDirection::Right | GestureDirection::Left => dy,
        GestureDirection::Up | GestureDirection::Down => dx,
        GestureDirection::None => 0
    }
}

/// Mid-hold state machine: `begin` on press, `accumulate` per raw-XY report,
/// `end` on release. Times are supplied by the caller so tests stay
/// deterministic.
#[derive(Debug, Default)]
pub struct SwipeAccumulator {
    /// When the current hold began; `None` when not holding.
    held_since:    Option<Instant>,
    /// Travel accumulated since the last commit (saturating).
    dx:            i32,
    dy:            i32,
    /// Whether any direction already fired during this hold.
    fired:         bool,
    /// Direction committed by the current stroke, if any.
    last:          Option<GestureDirection>,
    /// Travel continuing `last` since it committed; fires again at
    /// [`REPEAT_DISTANCE`].
    repeat:        i32,
    /// Sideways drift accompanying `repeat`.
    repeat_cross:  i32,
    /// When the last motion arrived, to detect the re-arm pause.
    moved_at:      Option<Instant>,
    /// Whether the MX Master 3S pre-press burst was already dropped.
    burst_dropped: bool
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

/// Wires the [`SwipeAccumulator`] to configured bindings and the key injector.
///
/// Owns the divert lifecycle: the gesture button is diverted when a device
/// arrives with gestures enabled and undiverted on removal or shutdown, so a
/// daemon exit never leaves the button stranded in diverted mode.
#[derive(Default)]
pub struct GestureRuntime {
    bindings:           Vec<Gesture>,
    diverted:           bool,
    accumulator:        SwipeAccumulator,
    injector:           Injector,
    last_commit:        Option<GestureDirection>,
    injection_warned:   bool,
    unsupported_warned: bool
}

impl GestureRuntime {
    /// Builds the runtime from the first configured device entry.
    ///
    /// Without the `gestures_enabled` opt-in the bindings are dropped, so a
    /// disabled runtime can never divert the button.
    #[must_use]
    pub fn from_config(config: &Config) -> Self {
        let Some(device) = config.devices.first() else {
            return Self::default();
        };
        let bindings = match (
            device.gestures_enabled,
            device.buttons.get(&ButtonId::ThumbGesture)
        ) {
            (
                true,
                Some(Action::Gestures {
                    gestures
                })
            ) => gestures.clone(),
            _ => Vec::new()
        };
        Self {
            bindings,
            ..Self::default()
        }
    }

    /// Whether the gesture button must be diverted for this configuration.
    #[must_use]
    pub const fn should_divert(&self) -> bool {
        !self.bindings.is_empty()
    }

    /// Whether a gesture hold is currently in progress; callers poll faster
    /// while this is true.
    #[must_use]
    pub const fn is_holding(&self) -> bool {
        self.accumulator.is_holding()
    }

    /// Replaces the bindings from a configuration change; disabling drops
    /// them so [`Self::should_divert`] goes false and the button is released
    /// back to the firmware on the next [`Self::apply`].
    pub fn set_enabled(&mut self, enabled: bool, bindings: Vec<Gesture>) {
        self.bindings = if enabled { bindings } else { Vec::new() };
    }

    /// Diverts the gesture button after a device attach.
    ///
    /// When gestures are not configured the button is defensively undiverted
    /// once: the divert flag persists in firmware, so this heals a button
    /// stranded by a previous session or a crashed daemon.
    pub fn apply(&mut self, device: &mut MxMaster3s) {
        if self.should_divert() {
            if self.diverted {
                return;
            }
            match device.set_control_divert(control_id::GESTURE_BUTTON, true, true) {
                Ok(()) => {
                    self.diverted = true;
                    match device.control_divert(control_id::GESTURE_BUTTON) {
                        Ok((flags, _)) if flags & 0x01 != 0 => {
                            info!(
                                "gesture button diverted for software recognition (flags {flags:#04x})"
                            );
                        }
                        Ok((flags, _)) => {
                            self.diverted = false;
                            warn!(
                                "device accepted the divert but reports flags {flags:#04x}; gestures stay inactive"
                            );
                        }
                        Err(e) => {
                            debug!("divert accepted, read-back failed: {e}");
                            info!("gesture button diverted for software recognition");
                        }
                    }
                }
                Err(e) => warn!("failed to divert the gesture button: {e}")
            }
        } else if self.diverted {
            self.detach(device);
        } else {
            let _ = device.set_control_divert(control_id::GESTURE_BUTTON, false, false);
        }
    }

    /// Undiverts the gesture button before a device drop or daemon exit.
    pub fn detach(&mut self, device: &mut MxMaster3s) {
        if !self.diverted {
            return;
        }
        self.diverted = false;
        self.accumulator = SwipeAccumulator::default();
        self.last_commit = None;
        if let Err(e) = device.set_control_divert(control_id::GESTURE_BUTTON, false, false) {
            warn!("failed to undivert the gesture button: {e}");
        }
    }

    /// Drains pending reprogrammable-controls notifications and fires the
    /// actions they complete.
    pub fn pump(&mut self, device: &mut MxMaster3s) {
        if !self.diverted {
            return;
        }
        while let Some(note) = device.read_reprog_notification(0) {
            self.on_notification(note);
        }
    }

    fn on_notification(&mut self, note: ReprogNotification) {
        match note {
            ReprogNotification::DivertedButtons {
                pressed
            } => {
                let held = pressed.contains(&control_id::GESTURE_BUTTON);
                if held && !self.accumulator.is_holding() {
                    debug!("gesture button pressed");
                    self.accumulator.begin(Instant::now());
                    self.last_commit = None;
                } else if !held && self.accumulator.is_holding() {
                    debug!("gesture button released");
                    self.finish_hold();
                }
            }
            ReprogNotification::RawXy {
                dx,
                dy
            } => {
                if let Some(dir) =
                    self.accumulator
                        .accumulate(i32::from(dx), i32::from(dy), Instant::now())
                {
                    debug!("swipe committed: {dir:?}");
                    self.last_commit = Some(dir);
                    self.fire(dir, Some(GestureMode::OnPress));
                }
            }
        }
    }

    fn finish_hold(&mut self) {
        let committed = self.last_commit.take();
        match self.accumulator.end() {
            Some(dir) => self.fire(dir, None),
            None if let Some(dir) = committed => self.fire(dir, Some(GestureMode::OnRelease)),
            None => {}
        }
    }

    /// Fires every binding matching the direction; `mode` restricts the fire
    /// moment, while `None` fires both modes (release commit and click).
    fn fire(&mut self, direction: GestureDirection, mode: Option<GestureMode>) {
        for gesture in &self.bindings {
            if gesture.direction != direction {
                continue;
            }
            if mode.is_some_and(|wanted| gesture.mode != wanted) {
                continue;
            }
            match gesture.action.as_ref() {
                Action::Keypress {
                    keys
                } => match self.injector.press_sequence(keys) {
                    Ok(()) => info!("gesture fired {direction:?} -> {}", keys.join("+")),
                    Err(e) if !self.injection_warned => {
                        self.injection_warned = true;
                        warn!("gesture injection failed: {e}");
                    }
                    Err(_) => {}
                },
                Action::Command {
                    command
                } => {
                    info!("gesture fired {direction:?} -> command {command:?}");
                    spawn_detached(command);
                }
                _ => {
                    if !self.unsupported_warned {
                        self.unsupported_warned = true;
                        warn!("gesture action not supported yet: {:?}", gesture.action);
                    }
                }
            }
        }
    }
}

/// Runs a bound shell command detached from the daemon.
///
/// A waiter thread reaps the child so the daemon never accumulates zombies,
/// and a long-running command can never block the gesture pump. The command
/// comes from the user-owned config file, the same trust level as any
/// autostart entry.
fn spawn_detached(command: &str) {
    let command = command.to_string();
    let spawn = std::thread::Builder::new()
        .name("gesture-command".to_string())
        .spawn(move || {
            let child = std::process::Command::new("/bin/sh")
                .arg("-c")
                .arg(&command)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn();
            match child {
                Ok(mut child) => match child.wait() {
                    Ok(status) => debug!("gesture command finished: {status}"),
                    Err(e) => warn!("failed to wait for gesture command: {e}")
                },
                Err(e) => warn!("failed to spawn gesture command: {e}")
            }
        });
    if spawn.is_err() {
        warn!("failed to start the gesture command thread");
    }
}

#[cfg(test)]
#[path = "gesture_tests.rs"]
mod gesture_tests;
