// SPDX-FileCopyrightText: 2025-2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! The wire protocol: every [`Request`] a client may send and every
//! [`Response`] the daemon answers with, one JSON object per line.

use logi_mx_driver::prelude::{
    Action, ButtonId, Gesture, GestureDirection, GestureMode, HiResScrollConfig, PieConfig,
    PieProfile, PieSlot, ReprogControl, SmartShiftConfig
};
use serde::{Deserialize, Serialize};

use super::live::LiveFrame;
use crate::{hyprbind::KeybindEntry, pie::PieEvent};

/// Client → daemon request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Request {
    /// Liveness probe.
    Ping,
    /// Device name, DPI, `SmartShift` and hi-res state.
    Info,
    /// Battery level and status.
    Battery,
    /// Easy-Switch host info.
    Hosts,
    /// Reprogrammable controls.
    Buttons,
    /// Current DPI value.
    Dpi,
    /// Current `SmartShift` configuration.
    SmartShift,
    /// Current hi-res scroll configuration.
    HiresScroll,
    /// Set DPI.
    SetDpi {
        /// Target DPI.
        value: u16
    },
    /// Set `SmartShift`.
    SetSmartShift {
        /// Enable flag.
        enabled:   bool,
        /// Threshold.
        threshold: u8
    },
    /// Set hi-res scroll.
    SetHires {
        /// Enable flag.
        enabled:  bool,
        /// Inverted flag.
        inverted: bool
    },
    /// Divert a programmable control to HID++ notifications.
    SetButtonDivert {
        /// HID++ control ID.
        control_id: u16,
        /// Whether to divert the control.
        divert:     bool,
        /// Whether to also stream raw XY deltas.
        raw_xy:     bool
    },
    /// Remap a programmable control to another native control on the device.
    SetButtonRemap {
        /// HID++ control ID to remap.
        control_id: u16,
        /// Target control ID, or `0x0000` to restore native behaviour.
        target_cid: u16
    },
    /// Reads the firmware remap currently stored for one control.
    GetButtonRemap {
        /// HID++ control ID to query.
        control_id: u16
    },
    /// Switch to an Easy-Switch host.
    SetHost {
        /// Zero-based host index.
        host: u8
    },
    /// Read the gesture configuration: opt-in flag and bindings.
    GetGestures,
    /// Enable or disable software gesture recognition; persists to config.
    SetGesturesEnabled {
        /// Whether the daemon should own the gesture button.
        enabled: bool
    },
    /// Bind an action to a gesture direction; persists to config.
    SetGesture {
        /// Direction to bind.
        direction: GestureDirection,
        /// When the action fires.
        mode:      GestureMode,
        /// Action to run: a validated `KEY_*` sequence or a shell command.
        action:    Box<Action>
    },
    /// Remove the binding for a gesture direction; persists to config.
    ClearGesture {
        /// Direction to unbind.
        direction: GestureDirection
    },
    /// Bind an action inside a per-application profile; persists to config.
    SetAppGesture {
        /// Hyprland window class the profile applies to.
        app_id:    String,
        /// Direction to bind.
        direction: GestureDirection,
        /// When the action fires.
        mode:      GestureMode,
        /// Action to run: a validated `KEY_*` sequence or a shell command.
        action:    Box<Action>
    },
    /// Remove one direction from a per-application profile; persists.
    ClearAppGesture {
        /// Hyprland window class.
        app_id:    String,
        /// Direction to unbind.
        direction: GestureDirection
    },
    /// Delete a per-application profile entirely; persists.
    RemoveAppProfile {
        /// Hyprland window class.
        app_id: String
    },
    /// Tune the radial pie menu for the gesture button; persists to config.
    SetPie {
        /// Whether the pie menu opens on a gesture hold.
        enabled:           bool,
        /// Cursor travel from the press point that opens the menu, in pixels.
        activation_radius: i32,
        /// Radius inside which no sector highlights, in pixels.
        dead_zone:         i32
    },
    /// Replace the global pie ring; the menu shows exactly as many equal
    /// wedges as there are slots. Persists to config.
    SetPieSectors {
        /// Ring slots in wedge order: index 0 at 12 o'clock, clockwise.
        sectors: Vec<PieSlot>
    },
    /// Replace the pie ring of one application profile; persists to config.
    SetAppPieSectors {
        /// Hyprland window class the profile applies to.
        app_id:  String,
        /// Ring slots in wedge order.
        sectors: Vec<PieSlot>
    },
    /// Delete a per-application pie profile entirely; persists.
    RemoveAppPieProfile {
        /// Hyprland window class.
        app_id: String
    },
    /// Read the pie menu state: global tuning with its ring and every
    /// per-application profile.
    GetPie,
    /// Read the software button remaps: global map and per-app profiles.
    GetButtons,
    /// Bind an action to a thumb side button globally; persists to config.
    SetButton {
        /// `ButtonId::Back` or `ButtonId::Forward`.
        button: ButtonId,
        /// Action to run: a validated `KEY_*` sequence or a shell command.
        action: Box<Action>
    },
    /// Restore a thumb side button to its native behaviour globally; persists.
    ClearButton {
        /// `ButtonId::Back` or `ButtonId::Forward`.
        button: ButtonId
    },
    /// Bind an action to a thumb side button inside a per-application
    /// profile; persists to config.
    SetAppButton {
        /// Hyprland window class the profile applies to.
        app_id: String,
        /// `ButtonId::Back` or `ButtonId::Forward`.
        button: ButtonId,
        /// Action to run: a validated `KEY_*` sequence or a shell command.
        action: Box<Action>
    },
    /// Remove one thumb button from a per-application profile; persists.
    ClearAppButton {
        /// Hyprland window class.
        app_id: String,
        /// `ButtonId::Back` or `ButtonId::Forward`.
        button: ButtonId
    },
    /// Delete a per-application button profile entirely; persists.
    RemoveAppButtonProfile {
        /// Hyprland window class.
        app_id: String
    },
    /// Report the currently focused Hyprland window class, if any.
    GetFocusedApp,
    /// List the active Hyprland keyboard bindings that can be replayed as a
    /// key chord; answered without touching the device.
    GetKeybinds,
    /// Subscribe to live input telemetry; the connection becomes a stream of
    /// [`Response::Live`] lines until it is closed.
    WatchLive,
    /// Subscribe to radial pie menu events; the connection becomes a stream
    /// of [`Response::Pie`] lines until it is closed.
    WatchPie
}

/// Daemon → client response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Response {
    /// Liveness probe reply.
    Pong,
    /// `Info` reply.
    Info {
        /// Marketing name.
        name:       String,
        /// Current DPI.
        dpi:        u16,
        /// `SmartShift` config.
        smartshift: SmartShiftConfig,
        /// Hi-res config.
        hires:      HiResScrollConfig
    },
    /// `Battery` reply.
    Battery {
        /// Remaining charge.
        level:  u8,
        /// Status string.
        status: String
    },
    /// `Hosts` reply.
    Hosts {
        /// Supported host count.
        hosts:   u8,
        /// Current host index.
        current: u8
    },
    /// `Buttons` reply.
    Buttons {
        /// All controls.
        controls: Vec<ReprogControl>
    },
    /// `Dpi` reply.
    Dpi {
        /// Current value.
        value: u16
    },
    /// `SmartShift` reply.
    SmartShift {
        /// Current config.
        config: SmartShiftConfig
    },
    /// `HiresScroll` reply.
    HiresScroll {
        /// Current config.
        config: HiResScrollConfig
    },
    /// `GetGestures` reply.
    Gestures {
        /// Whether software gesture recognition is enabled.
        enabled:  bool,
        /// Current global bindings.
        gestures: Vec<Gesture>,
        /// Per-application overrides keyed by window class.
        profiles: std::collections::HashMap<String, Vec<Gesture>>,
        /// Radial pie menu tuning.
        pie:      PieConfig
    },
    /// `GetPie` reply.
    PieSettings {
        /// Global pie tuning including its ring slots.
        config:   PieConfig,
        /// Per-application rings keyed by window class.
        profiles: std::collections::HashMap<String, PieProfile>
    },
    /// `GetButtons` reply.
    ButtonsConfig {
        /// Global thumb button remaps, Back and Forward only.
        buttons:  std::collections::HashMap<ButtonId, Action>,
        /// Per-application overrides keyed by window class.
        profiles: std::collections::HashMap<String, std::collections::HashMap<ButtonId, Action>>
    },
    /// `GetButtonRemap` reply.
    ButtonRemap {
        /// Queried control ID.
        control_id: u16,
        /// Control the firmware currently maps it to; `0` means native.
        target_cid: u16
    },
    /// The focused window class reported by the desktop, if any.
    FocusedApp {
        /// Hyprland window class of the focused window.
        app_id: Option<String>
    },
    /// `GetKeybinds` reply.
    Keybinds {
        /// Replayable keyboard bindings reported by Hyprland.
        entries: Vec<KeybindEntry>
    },
    /// Generic success for setters.
    Ok,
    /// Acknowledgement that a live telemetry stream was accepted; further
    /// lines on this connection are [`Response::Live`] frames.
    LiveReady,
    /// Live input telemetry frame on a `WatchLive` connection.
    Live(LiveFrame),
    /// Acknowledgement that a pie event stream was accepted; further lines
    /// on this connection are [`Response::Pie`] events.
    PieReady,
    /// Radial pie menu event on a `WatchPie` connection.
    Pie(PieEvent),
    /// Human-readable error.
    Error {
        /// Error message.
        message: String
    }
}
