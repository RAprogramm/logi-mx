// SPDX-FileCopyrightText: 2025-2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! evdev tap turning kernel input events into live telemetry frames.
//!
//! The daemon owns the HID++ control path, while pointer motion, the wheels
//! and the standard buttons are delivered by the kernel through evdev. A
//! dedicated thread attaches to the mouse event node, coalesces every
//! `SYN_REPORT` batch into a [`LiveFrame`] and publishes it on the IPC
//! fan-out hub. The tap never grabs the device: it observes the same stream
//! the compositor consumes.

use std::{
    collections::HashSet,
    thread,
    time::{Duration, Instant}
};

use evdev::{Device, EventType, InputEvent, RelativeAxisCode};
use tracing::{debug, info, warn};

use crate::ipc::{
    BTN_BIT_BACK, BTN_BIT_FORWARD, BTN_BIT_GESTURE, BTN_BIT_LEFT, BTN_BIT_MIDDLE, BTN_BIT_RIGHT,
    LiveFrame, LiveHub
};

/// Delay before the next scan after the device vanished or could not open.
const RESCAN_DELAY: Duration = Duration::from_secs(2);

const SYN_REPORT: u16 = 0;
const REL_X: u16 = 0;
const REL_Y: u16 = 1;
const REL_HWHEEL: u16 = 6;
const REL_WHEEL: u16 = 8;
const REL_WHEEL_HI_RES: u16 = 11;
const REL_HWHEEL_HI_RES: u16 = 12;

const BTN_LEFT: u16 = 272;
const BTN_RIGHT: u16 = 273;
const BTN_MIDDLE: u16 = 274;
const BTN_SIDE: u16 = 275;
const BTN_EXTRA: u16 = 276;
const BTN_FORWARD: u16 = 277;

/// Maps an evdev key code onto a [`LiveFrame`] button bit.
///
/// `BTN_SIDE` is the rear thumb button and `BTN_EXTRA` the front one, which
/// is how the kernel orders them on Logitech mice. The physical labels run
/// the other way: the front button is Back and the rear one is Forward, so
/// the codes cross over here. Codes without a named mapping (gesture and DPI
/// buttons never reach the kernel at all) are handled through
/// [`LiveFrame::extra_keys`] or the daemon's [`crate::ipc::LiveState`].
#[must_use]
pub const fn key_bit(code: u16) -> Option<u32> {
    match code {
        BTN_LEFT => Some(BTN_BIT_LEFT),
        BTN_RIGHT => Some(BTN_BIT_RIGHT),
        BTN_MIDDLE => Some(BTN_BIT_MIDDLE),
        BTN_SIDE => Some(BTN_BIT_FORWARD),
        BTN_EXTRA => Some(BTN_BIT_BACK),
        BTN_FORWARD => Some(BTN_BIT_GESTURE),
        _ => None
    }
}

/// Coalesces raw device events into frames flushed on every `SYN_REPORT`.
#[derive(Debug)]
pub struct TelemetryAccumulator {
    frame:      LiveFrame,
    extra:      HashSet<u16>,
    dirty:      bool,
    last_flush: Instant
}

impl Default for TelemetryAccumulator {
    fn default() -> Self {
        Self::new()
    }
}

impl TelemetryAccumulator {
    /// Creates an empty accumulator with the flush clock started now.
    #[must_use]
    pub fn new() -> Self {
        Self {
            frame:      LiveFrame::default(),
            extra:      HashSet::new(),
            dirty:      false,
            last_flush: Instant::now()
        }
    }

    /// Applies a key event; `pressed` distinguishes press from release.
    pub fn key(&mut self, code: u16, pressed: bool) {
        if let Some(bit) = key_bit(code) {
            if pressed {
                self.frame.buttons |= bit;
            } else {
                self.frame.buttons &= !bit;
            }
        } else if code >= BTN_LEFT {
            if pressed {
                self.extra.insert(code);
            } else {
                self.extra.remove(&code);
            }
        } else {
            return;
        }
        self.dirty = true;
    }

    /// Applies a relative-axis event to the pending deltas.
    pub const fn rel(&mut self, code: u16, value: i32) {
        match code {
            REL_X => self.frame.dx += value,
            REL_Y => self.frame.dy += value,
            REL_WHEEL => self.frame.wheel += value,
            REL_WHEEL_HI_RES => self.frame.wheel_hi += value,
            REL_HWHEEL => self.frame.hwheel += value,
            REL_HWHEEL_HI_RES => self.frame.hwheel_hi += value,
            _ => return
        }
        self.dirty = true;
    }

    /// Completes one report batch, returning the coalesced frame when the
    /// device state changed since the previous flush.
    pub fn report(&mut self) -> Option<LiveFrame> {
        if !self.dirty {
            return None;
        }

        let mut frame = self.frame.clone();
        frame.extra_keys = self.extra.iter().copied().collect();
        frame.extra_keys.sort_unstable();
        frame.dt_ms = u32::try_from(self.last_flush.elapsed().as_millis()).unwrap_or(u32::MAX);

        self.last_flush = Instant::now();
        self.dirty = false;
        self.frame.dx = 0;
        self.frame.dy = 0;
        self.frame.wheel = 0;
        self.frame.wheel_hi = 0;
        self.frame.hwheel = 0;
        self.frame.hwheel_hi = 0;
        Some(frame)
    }

    /// Feeds one raw event, returning a frame when a report batch completed.
    pub fn apply(&mut self, event: &InputEvent) -> Option<LiveFrame> {
        let kind = event.event_type();
        if kind == EventType::SYNCHRONIZATION {
            return if event.code() == SYN_REPORT {
                self.report()
            } else {
                None
            };
        }
        if kind == EventType::KEY {
            self.key(event.code(), event.value() != 0);
            return None;
        }
        if kind == EventType::RELATIVE {
            self.rel(event.code(), event.value());
        }
        None
    }
}

/// Finds the pointer event node belonging to the MX Master 3S.
///
/// Prefers a node whose name mentions the mouse (direct Bluetooth pairing);
/// falls back to any Logitech node with a relative X axis, which is the
/// "Logitech USB Receiver Mouse" node the Bolt receiver exposes. That node
/// carries motion, both wheels and every thumb button, so exactly one node
/// is tapped and tilt events are never double-counted through the Consumer
/// Control node.
fn find_mouse() -> Option<(std::path::PathBuf, String)> {
    let mut fallback = None;
    for (path, device) in evdev::enumerate() {
        let name = device.name().unwrap_or_default().to_lowercase();
        if name.contains(crate::inject::INJECTOR_NAME) {
            continue;
        }
        let is_pointer = device
            .supported_relative_axes()
            .is_some_and(|axes| axes.contains(RelativeAxisCode::REL_X));
        if !is_pointer {
            continue;
        }
        if name.contains("mx master") {
            return Some((path, name));
        }
        if name.contains("logitech") && fallback.is_none() {
            fallback = Some((path, name));
        }
    }
    fallback
}

/// Streams event batches until the device stops answering.
///
/// `evdev::Device::fetch_events` performs a single blocking read and its
/// iterator ends as soon as that batch is drained, so the stream is only
/// alive while this loop keeps re-arming reads. A read error — typically
/// `ENODEV` after an unplug — ends the attachment and lets the caller
/// rescan for a fresh node. Diverted buttons never appear in the kernel
/// stream, so the daemon-side bits from `state` are merged into every frame.
fn stream(
    device: &mut Device,
    accumulator: &mut TelemetryAccumulator,
    hub: &LiveHub,
    state: &crate::ipc::SharedLiveState
) -> std::io::Result<()> {
    use std::sync::atomic::Ordering;

    loop {
        for event in device.fetch_events()? {
            if let Some(mut frame) = accumulator.apply(&event) {
                state.last_buttons.store(frame.buttons, Ordering::Relaxed);
                frame.buttons |= state.virtual_bits.load(Ordering::Relaxed);
                let _ = hub.send(frame);
            }
        }
    }
}

/// Attaches to the mouse and streams frames until the device detaches.
fn pump(hub: &LiveHub, state: &crate::ipc::SharedLiveState) {
    let Some((path, name)) = find_mouse() else {
        debug!("live telemetry: MX Master event node not found");
        thread::sleep(RESCAN_DELAY);
        return;
    };

    let Ok(mut device) = Device::open(&path) else {
        warn!("live telemetry: cannot open {path:?}: permission denied or busy");
        thread::sleep(RESCAN_DELAY);
        return;
    };
    info!("live telemetry attached to {name} ({path:?})");

    let mut accumulator = TelemetryAccumulator::new();
    if let Err(e) = stream(&mut device, &mut accumulator, hub, state) {
        info!("live telemetry detached from {name}: {e}");
    }
}

/// Runs the evdev tap until the process exits.
pub fn run(hub: &LiveHub, state: &crate::ipc::SharedLiveState) {
    loop {
        pump(hub, state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key_event(code: u16, pressed: bool) -> InputEvent {
        InputEvent::new(EventType::KEY.0, code, i32::from(pressed))
    }

    fn rel_event(code: u16, value: i32) -> InputEvent {
        InputEvent::new(EventType::RELATIVE.0, code, value)
    }

    fn syn_report() -> InputEvent {
        InputEvent::new(EventType::SYNCHRONIZATION.0, SYN_REPORT, 0)
    }

    #[test]
    fn key_bit_maps_standard_mouse_buttons() {
        assert_eq!(key_bit(BTN_LEFT), Some(BTN_BIT_LEFT));
        assert_eq!(key_bit(BTN_RIGHT), Some(BTN_BIT_RIGHT));
        assert_eq!(key_bit(BTN_MIDDLE), Some(BTN_BIT_MIDDLE));
        assert_eq!(key_bit(BTN_SIDE), Some(BTN_BIT_FORWARD));
        assert_eq!(key_bit(BTN_EXTRA), Some(BTN_BIT_BACK));
        assert_eq!(key_bit(BTN_FORWARD), Some(BTN_BIT_GESTURE));
    }

    #[test]
    fn key_bit_rejects_non_button_codes() {
        assert_eq!(key_bit(1), None);
        assert_eq!(key_bit(0x1ff), None);
    }

    #[test]
    fn motion_accumulates_until_report() {
        let mut acc = TelemetryAccumulator::new();
        acc.apply(&rel_event(REL_X, 5));
        acc.apply(&rel_event(REL_Y, -3));
        let frame = acc.apply(&syn_report()).expect("report emits a frame");

        assert_eq!(frame.dx, 5);
        assert_eq!(frame.dy, -3);
        assert_eq!(frame.buttons, 0);
        assert!(
            acc.apply(&syn_report()).is_none(),
            "idle reports emit nothing"
        );
    }

    #[test]
    fn report_resets_deltas_but_keeps_pressed_buttons() {
        let mut acc = TelemetryAccumulator::new();
        acc.apply(&key_event(BTN_LEFT, true));
        acc.apply(&rel_event(REL_X, 10));
        acc.apply(&syn_report()).expect("first frame");

        acc.apply(&rel_event(REL_X, 2));
        let second = acc.apply(&syn_report()).expect("second frame");
        assert_eq!(second.dx, 2, "deltas must not leak between frames");
        assert_eq!(second.buttons & BTN_BIT_LEFT, BTN_BIT_LEFT);

        acc.apply(&key_event(BTN_LEFT, false));
        let third = acc.apply(&syn_report()).expect("release frame");
        assert_eq!(third.buttons & BTN_BIT_LEFT, 0);
    }

    #[test]
    fn wheel_and_tilt_are_tracked_separately() {
        let mut acc = TelemetryAccumulator::new();
        acc.apply(&rel_event(REL_WHEEL, 1));
        acc.apply(&rel_event(REL_WHEEL_HI_RES, 120));
        acc.apply(&rel_event(REL_HWHEEL, -1));
        acc.apply(&rel_event(REL_HWHEEL_HI_RES, -120));
        let frame = acc.apply(&syn_report()).expect("wheel frame");

        assert_eq!(frame.wheel, 1);
        assert_eq!(frame.wheel_hi, 120);
        assert_eq!(frame.hwheel, -1);
        assert_eq!(frame.hwheel_hi, -120);
    }

    #[test]
    fn unmapped_buttons_land_in_extra_keys() {
        let mut acc = TelemetryAccumulator::new();
        acc.apply(&key_event(280, true));
        let frame = acc.apply(&syn_report()).expect("unknown button frame");
        assert_eq!(frame.extra_keys, vec![280]);
        assert_eq!(frame.buttons, 0);

        acc.apply(&key_event(280, false));
        let released = acc.apply(&syn_report()).expect("release frame");
        assert_eq!(released.extra_keys, Vec::<u16>::new());
    }

    #[test]
    fn unrelated_event_types_are_ignored() {
        let mut acc = TelemetryAccumulator::new();
        acc.apply(&InputEvent::new(EventType::MISC.0, 4, 1));
        assert!(acc.apply(&syn_report()).is_none());
    }
}
