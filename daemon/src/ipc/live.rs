// SPDX-FileCopyrightText: 2025-2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Live input telemetry: the coalesced frame pushed to `WatchLive` clients
//! and the daemon-side button state merged into it.

use serde::{Deserialize, Serialize};

/// Fan-out channel carrying live input telemetry to subscribed clients.
pub type LiveHub = tokio::sync::broadcast::Sender<LiveFrame>;

/// Button state the daemon injects into live telemetry.
///
/// Diverted HID++ buttons (gesture, wheel mode) never reach the kernel, so
/// the worker thread raises their bits here and the evdev tap merges them
/// into every frame it publishes. `last_buttons` records the raw kernel
/// button mask of the newest frame so an immediate daemon-published frame
/// never drops physically held buttons.
#[derive(Debug, Default)]
pub struct LiveState {
    /// Bits for daemon-side (diverted) pressed buttons.
    pub virtual_bits: std::sync::atomic::AtomicU32,
    /// Kernel button mask of the most recently published frame.
    pub last_buttons: std::sync::atomic::AtomicU32
}

/// Shared handle to the daemon-injected button state.
pub type SharedLiveState = std::sync::Arc<LiveState>;

/// One coalesced live input snapshot pushed to `WatchLive` clients.
///
/// Motion and wheel values are deltas accumulated since the previous frame;
/// button bits are the absolute pressed state.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LiveFrame {
    /// Bitmask of pressed buttons, see `BTN_BIT_*` constants.
    pub buttons:    u32,
    /// Horizontal movement since the previous frame, in device units.
    pub dx:         i32,
    /// Vertical movement since the previous frame, in device units.
    pub dy:         i32,
    /// Milliseconds covered by this frame.
    pub dt_ms:      u32,
    /// Vertical wheel notches since the previous frame.
    pub wheel:      i32,
    /// High-resolution vertical wheel units since the previous frame
    /// (120 units per notch).
    pub wheel_hi:   i32,
    /// Horizontal (tilt) wheel notches since the previous frame.
    pub hwheel:     i32,
    /// High-resolution horizontal wheel units since the previous frame.
    pub hwheel_hi:  i32,
    /// Pressed key codes without a named button mapping, ascending.
    pub extra_keys: Vec<u16>
}

/// Bit for the primary left button in [`LiveFrame::buttons`].
pub const BTN_BIT_LEFT: u32 = 1 << 0;
/// Bit for the primary right button in [`LiveFrame::buttons`].
pub const BTN_BIT_RIGHT: u32 = 1 << 1;
/// Bit for the wheel click in [`LiveFrame::buttons`].
pub const BTN_BIT_MIDDLE: u32 = 1 << 2;
/// Bit for the front thumb button in [`LiveFrame::buttons`].
pub const BTN_BIT_BACK: u32 = 1 << 3;
/// Bit for the rear thumb button in [`LiveFrame::buttons`].
pub const BTN_BIT_FORWARD: u32 = 1 << 4;
/// Bit for the gesture button in [`LiveFrame::buttons`].
pub const BTN_BIT_GESTURE: u32 = 1 << 5;
/// Bit for the wheel-mode (DPI shift) button in [`LiveFrame::buttons`].
///
/// The button is a pure HID++ control: the daemon diverts it and raises this
/// bit itself, the kernel never reports it.
pub const BTN_BIT_DPI: u32 = 1 << 6;
