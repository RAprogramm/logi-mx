// SPDX-FileCopyrightText: 2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Gesture runtime for the diverted gesture button.
//!
//! Owns the divert lifecycle: the gesture button is diverted when a device
//! arrives with gestures enabled and undiverted on removal or shutdown, so a
//! daemon exit never leaves the button stranded. Raw-XY travel is folded into
//! directions by [`SwipeAccumulator`], re-exported from the `swipe`
//! submodule, and committed directions fire the configured bindings through
//! the key injector.
//!
//! The runtime also hosts the radial pie menu ([`crate::pie::PieRuntime`]):
//! the press and release edges arm and settle it, and the worker loop drives
//! [`GestureRuntime::on_pie_tick`] once per poll while the button is held.
//! When the pie is disabled (the default) it never queries the cursor and
//! the release path behaves exactly as without the feature.

mod swipe;

use std::{collections::HashMap, sync::atomic::Ordering, time::Instant};

use logi_mx_driver::prelude::*;
#[allow(
    unused_imports,
    reason = "the binary crate compiles this module privately; the re-exports are the library API"
)]
pub use swipe::{
    HOLD_FOR_SWIPE, REARM_PAUSE, REPEAT_DISTANCE, SWIPE_DEADZONE, SWIPE_THRESHOLD,
    SwipeAccumulator, detect_swipe
};
use tracing::{debug, info, warn};

use crate::{
    focus::SharedFocus,
    inject::Injector,
    ipc::{BTN_BIT_DPI, BTN_BIT_GESTURE, LiveFrame, LiveHub, SharedLiveState},
    pie::{PieConfig, PieHub, PieRuntime, merge_effective}
};

/// Divert bookkeeping for the wheel-mode (DPI shift) button.
#[derive(Default)]
struct DpiButton {
    diverted: bool,
    held:     bool
}

/// Wires the [`SwipeAccumulator`] to configured bindings and the key injector.
///
/// Owns the divert lifecycle: the gesture button is diverted when a device
/// arrives with gestures enabled and undiverted on removal or shutdown, so a
/// daemon exit never leaves the button stranded in diverted mode. The
/// wheel-mode button is always diverted: the kernel never sees it, so the
/// daemon toggles `SmartShift` itself and reports the press on live
/// telemetry, which is the only way the HUD can light the button up.
#[derive(Default)]
pub struct GestureRuntime {
    bindings:           Vec<Gesture>,
    profiles:           HashMap<String, Vec<Gesture>>,
    focus:              SharedFocus,
    diverted:           bool,
    dpi:                DpiButton,
    smartshift:         SmartShiftConfig,
    live:               SharedLiveState,
    hub:                Option<LiveHub>,
    pie:                PieRuntime,
    accumulator:        SwipeAccumulator,
    injector:           Injector,
    last_commit:        Option<GestureDirection>,
    injection_warned:   bool,
    unsupported_warned: bool
}

impl GestureRuntime {
    /// Builds the runtime from the first configured device entry.
    ///
    /// Without the `gestures_enabled` opt-in the bindings and profiles are
    /// dropped, so a disabled runtime can never divert the button.
    #[must_use]
    pub fn from_config(config: &Config) -> Self {
        let Some(device) = config.devices.first() else {
            return Self::default();
        };
        let (bindings, profiles) = match (
            device.gestures_enabled,
            device.buttons.get(&ButtonId::ThumbGesture)
        ) {
            (
                true,
                Some(Action::Gestures {
                    gestures
                })
            ) => (gestures.clone(), device.gesture_profiles.clone()),
            _ => (Vec::new(), HashMap::new())
        };
        let mut pie = PieRuntime::default();
        pie.set_config(device.pie.clone());
        pie.set_bindings(bindings.clone(), profiles.clone());
        Self {
            bindings,
            profiles,
            smartshift: device.smartshift,
            pie,
            ..Self::default()
        }
    }

    /// Attaches the live telemetry handles used to surface diverted buttons.
    pub fn attach_live(&mut self, live: SharedLiveState, hub: LiveHub) {
        self.live = live;
        self.hub = Some(hub);
    }

    /// Attaches the shared focus slot consulted when resolving bindings.
    pub fn attach_focus(&mut self, focus: SharedFocus) {
        self.pie.attach_focus(focus.clone());
        self.focus = focus;
    }

    /// Attaches the pie event fan-out the radial menu publishes on.
    pub fn attach_pie(&mut self, hub: PieHub) {
        self.pie.attach_hub(hub);
    }

    /// Replaces the radial pie menu tuning from a configuration change.
    pub const fn set_pie_config(&mut self, config: PieConfig) {
        self.pie.set_config(config);
    }

    /// Advances the pie state machine one poll while the gesture button is
    /// held; the worker loop calls this beside the notification pump.
    pub fn on_pie_tick(&mut self) {
        self.pie.on_tick();
    }

    /// The class of the currently focused window, if the desktop reports one.
    #[must_use]
    pub fn focused_app(&self) -> Option<String> {
        self.focus
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// Whether the gesture button must be diverted for this configuration.
    #[must_use]
    pub fn should_divert(&self) -> bool {
        !self.bindings.is_empty() || self.profiles.values().any(|bindings| !bindings.is_empty())
    }

    /// Whether a gesture hold is currently in progress; callers poll faster
    /// while this is true.
    #[must_use]
    pub const fn is_holding(&self) -> bool {
        self.accumulator.is_holding()
    }

    /// Replaces the bindings and per-app profiles from a configuration
    /// change; disabling drops everything so [`Self::should_divert`] goes
    /// false and the button is released back to the firmware on the next
    /// [`Self::apply`].
    pub fn set_config(
        &mut self,
        enabled: bool,
        bindings: Vec<Gesture>,
        profiles: HashMap<String, Vec<Gesture>>
    ) {
        if enabled {
            self.bindings = bindings;
            self.profiles = profiles;
        } else {
            self.bindings = Vec::new();
            self.profiles = HashMap::new();
        }
        self.pie
            .set_bindings(self.bindings.clone(), self.profiles.clone());
    }

    /// The bindings in force right now: the global set with the focused
    /// app's profile layered on top. A profile entry replaces the global
    /// binding for its direction; directions the profile does not mention
    /// fall through to the global set.
    fn effective_bindings(&self) -> Vec<Gesture> {
        merge_effective(
            &self.bindings,
            &self.profiles,
            self.focused_app().as_deref()
        )
    }

    /// Diverts the gesture and wheel-mode buttons after a device attach.
    ///
    /// When gestures are not configured the gesture button is defensively
    /// undiverted once: the divert flag persists in firmware, so this heals a
    /// button stranded by a previous session or a crashed daemon.
    pub fn apply(&mut self, device: &mut MxMaster3s) {
        self.apply_gesture(device);
        self.apply_dpi(device);
    }

    fn apply_gesture(&mut self, device: &mut MxMaster3s) {
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
            self.undivert_gesture(device);
        } else {
            let _ = device.set_control_divert(control_id::GESTURE_BUTTON, false, false);
        }
    }

    /// Diverts the wheel-mode (DPI shift) button so its presses reach the
    /// daemon. The kernel never reports this control, so without the divert
    /// neither the HUD highlight nor the `SmartShift` toggle can work.
    fn apply_dpi(&mut self, device: &mut MxMaster3s) {
        if self.dpi.diverted {
            return;
        }
        match device.set_control_divert(control_id::SMARTSHIFT_TOGGLE, true, false) {
            Ok(()) => {
                self.dpi.diverted = true;
                info!("wheel-mode button diverted for live telemetry");
            }
            Err(e) => debug!("cannot divert the wheel-mode button: {e}")
        }
    }

    /// Undiverts the gesture and wheel-mode buttons before a device drop or
    /// daemon exit.
    pub fn detach(&mut self, device: &mut MxMaster3s) {
        self.undivert_gesture(device);
        if self.dpi.diverted {
            self.dpi.diverted = false;
            self.dpi.held = false;
            self.set_live_bit(BTN_BIT_DPI, false);
            if let Err(e) = device.set_control_divert(control_id::SMARTSHIFT_TOGGLE, false, false)
            {
                warn!("failed to undivert the wheel-mode button: {e}");
            }
        }
    }

    fn undivert_gesture(&mut self, device: &mut MxMaster3s) {
        if !self.diverted {
            return;
        }
        self.diverted = false;
        self.accumulator = SwipeAccumulator::default();
        self.last_commit = None;
        self.pie.on_cancel();
        self.set_live_bit(BTN_BIT_GESTURE, false);
        if let Err(e) = device.set_control_divert(control_id::GESTURE_BUTTON, false, false) {
            warn!("failed to undivert the gesture button: {e}");
        }
    }

    /// Whether the runtime needs the reprogrammable-controls notification
    /// stream; callers skip the device read while this is false.
    #[must_use]
    pub const fn wants_pump(&self) -> bool {
        self.diverted || self.dpi.diverted
    }

    /// Handles one diverted-buttons report: the gesture hold opens and
    /// settles on its edge, the wheel-mode button toggles on its press.
    pub fn on_diverted_buttons(&mut self, pressed: &[u16], device: &mut MxMaster3s) {
        self.on_gesture_edge(pressed.contains(&control_id::GESTURE_BUTTON));
        self.on_wheel_mode_edge(pressed.contains(&control_id::SMARTSHIFT_TOGGLE), device);
    }

    /// Folds one raw-XY report into the swipe accumulator and fires the
    /// direction it commits, if any.
    pub fn on_raw_xy(&mut self, dx: i16, dy: i16) {
        if let Some(dir) =
            self.accumulator
                .accumulate(i32::from(dx), i32::from(dy), Instant::now())
        {
            debug!("swipe committed: {dir:?}");
            self.last_commit = Some(dir);
            self.fire(dir, Some(GestureMode::OnPress));
        }
    }

    /// Tracks the gesture button edge: press opens the swipe accumulator,
    /// arms the pie and raises the live bit; release settles the hold and
    /// lowers it.
    fn on_gesture_edge(&mut self, held: bool) {
        if held && !self.accumulator.is_holding() {
            debug!("gesture button pressed");
            self.accumulator.begin(Instant::now());
            self.last_commit = None;
            self.set_live_bit(BTN_BIT_GESTURE, true);
            self.pie.on_press();
        } else if !held && self.accumulator.is_holding() {
            debug!("gesture button released");
            self.set_live_bit(BTN_BIT_GESTURE, false);
            self.finish_hold();
        }
    }

    /// Tracks the wheel-mode button edge: press toggles `SmartShift` (the
    /// firmware's native job once the control is diverted) and raises the
    /// live bit so the HUD lights the button up.
    fn on_wheel_mode_edge(&mut self, pressed: bool, device: &mut MxMaster3s) {
        if pressed == self.dpi.held {
            return;
        }
        self.dpi.held = pressed;
        self.set_live_bit(BTN_BIT_DPI, pressed);
        if !pressed {
            return;
        }
        self.smartshift.enabled = !self.smartshift.enabled;
        match device.set_smartshift(self.smartshift) {
            Ok(()) => info!(
                "wheel-mode button: SmartShift {}",
                if self.smartshift.enabled {
                    "enabled"
                } else {
                    "disabled"
                }
            ),
            Err(e) => {
                self.smartshift.enabled = !self.smartshift.enabled;
                warn!("failed to toggle SmartShift: {e}");
            }
        }
    }

    /// Raises or lowers a daemon-side button bit and publishes an immediate
    /// frame so the HUD reacts without waiting for kernel input.
    fn set_live_bit(&self, bit: u32, pressed: bool) {
        if pressed {
            self.live.virtual_bits.fetch_or(bit, Ordering::Relaxed);
        } else {
            self.live.virtual_bits.fetch_and(!bit, Ordering::Relaxed);
        }
        self.publish_live();
    }

    fn publish_live(&self) {
        let Some(hub) = &self.hub else {
            return;
        };
        let frame = LiveFrame {
            buttons: self.live.last_buttons.load(Ordering::Relaxed)
                | self.live.virtual_bits.load(Ordering::Relaxed),
            ..LiveFrame::default()
        };
        let _ = hub.send(frame);
    }

    /// Settles a released hold.
    ///
    /// When the pie was open at release it owns the hold: the committed
    /// sector (if bound) fires and the swipe `OnRelease` fallback is
    /// suppressed, so one hold never fires twice. Mid-hold `OnPress` swipe
    /// commits already fired through [`Self::on_raw_xy`] and are untouched.
    /// When the pie never opened, the swipe release path is byte-identical
    /// to the pie-less runtime.
    fn finish_hold(&mut self) {
        let pie_owned = self.pie.is_open();
        let pie_commit = self.pie.on_release();
        let swipe_commit = self.last_commit.take();
        let settled = self.accumulator.end();
        if pie_owned {
            if let Some(dir) = pie_commit {
                self.fire(dir, None);
            }
            return;
        }
        match settled {
            Some(dir) => self.fire(dir, None),
            None => {
                if let Some(dir) = swipe_commit {
                    self.fire(dir, Some(GestureMode::OnRelease));
                }
            }
        }
    }

    /// Fires every binding matching the direction; `mode` restricts the fire
    /// moment, while `None` fires both modes (release commit and click).
    fn fire(&mut self, direction: GestureDirection, mode: Option<GestureMode>) {
        let gestures = self.effective_bindings();
        for gesture in &gestures {
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
                    spawn_detached("gesture-command", command);
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

/// Runs a bound shell command detached from the daemon on a named thread.
///
/// A waiter thread reaps the child so the daemon never accumulates zombies,
/// and a long-running command can never block the event pump. The command
/// comes from the user-owned config file, the same trust level as any
/// autostart entry.
///
/// # Arguments
///
/// * `name` - Thread name used to label the completion log line.
/// * `command` - Shell command handed to `/bin/sh -c`.
///
/// # Examples
///
/// ```
/// logi_mx_daemon::gesture::spawn_detached("demo", "true");
/// ```
pub fn spawn_detached(name: &str, command: &str) {
    let command = command.to_string();
    let thread_name = name.to_string();
    let log_name = name.to_string();
    let spawn = std::thread::Builder::new()
        .name(thread_name)
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
                    Ok(status) => debug!("{log_name} command finished: {status}"),
                    Err(e) => warn!("failed to wait for {log_name} command: {e}")
                },
                Err(e) => warn!("failed to spawn {log_name} command: {e}")
            }
        });
    if spawn.is_err() {
        warn!("failed to start the {name} command thread");
    }
}

#[cfg(test)]
#[path = "gesture_tests.rs"]
mod gesture_tests;
