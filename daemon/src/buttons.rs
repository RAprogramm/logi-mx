// SPDX-FileCopyrightText: 2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Per-application remapping of the two thumb side buttons.
//!
//! Back and Forward are diverted to the daemon only when some mapping —
//! global or inside a per-application profile — actually takes a button
//! over; with an empty configuration both keep their native firmware
//! behaviour and nothing is diverted. A diverted press resolves through the
//! focused application's profile first and the global map second: a key
//! chord fires through the injector, a shell command spawns detached, and
//! anything else is injected back as the real mouse button, so the
//! passthrough is indistinguishable from the firmware press. While a button
//! is diverted the kernel never sees it, so the runtime raises the virtual
//! live bits itself and the HUD keeps lighting the buttons up.

use std::{collections::HashMap, sync::atomic::Ordering};

use logi_mx_driver::prelude::*;
use tracing::{debug, info, warn};

use crate::{
    focus::SharedFocus,
    gesture::spawn_detached,
    inject::Injector,
    ipc::{BTN_BIT_BACK, BTN_BIT_FORWARD, LiveFrame, LiveHub, SharedLiveState}
};

/// Divert and press bookkeeping for one thumb side button.
#[derive(Default, Clone, Copy)]
struct ThumbButton {
    diverted: bool,
    held:     bool,
    injected: bool
}

/// What a button edge resolved to; the runtime executes it through the
/// injector, while tests observe the decision without touching uinput.
#[derive(Debug, PartialEq, Eq)]
enum ButtonEffect {
    /// Inject the real mouse button press (native passthrough).
    InjectPress,
    /// Release a previously injected native press.
    InjectRelease,
    /// Press a configured key chord.
    Chord(Vec<String>),
    /// Run a configured shell command.
    Command(String),
    /// Nothing to do.
    Idle
}

/// Reports whether a mapping takes the button over from the firmware.
///
/// Only key chords and shell commands do; `Action::None` and every
/// gesture-specific action leave the button native.
const fn is_remapped(action: Option<&Action>) -> bool {
    matches!(
        action,
        Some(Action::Keypress { .. } | Action::Command { .. })
    )
}

/// HID++ control ID of a thumb side button.
///
/// # Panics
///
/// Never in practice: only `Back` and `Forward` reach this runtime.
fn control_for(button: ButtonId) -> u16 {
    match button {
        ButtonId::Back => control_id::BACK_BUTTON,
        ButtonId::Forward => control_id::FORWARD_BUTTON,
        other => unreachable!("{other:?} is not a thumb side button")
    }
}

/// Live telemetry bit of a thumb side button.
///
/// # Panics
///
/// Never in practice: only `Back` and `Forward` reach this runtime.
fn live_bit_for(button: ButtonId) -> u32 {
    match button {
        ButtonId::Back => BTN_BIT_BACK,
        ButtonId::Forward => BTN_BIT_FORWARD,
        other => unreachable!("{other:?} is not a thumb side button")
    }
}

/// Wires the diverted thumb buttons to the configured remaps and the
/// injector.
///
/// Owns the divert lifecycle like [`crate::gesture::GestureRuntime`] owns
/// the gesture button: controls are diverted when a device arrives with a
/// mapping that needs them and undiverted on removal or shutdown, so a
/// daemon exit never leaves a button stranded. The two buttons are tracked
/// independently: a configuration that only remaps Back never diverts
/// Forward.
#[derive(Default)]
pub struct ButtonRuntime {
    global:             HashMap<ButtonId, Action>,
    profiles:           HashMap<String, HashMap<ButtonId, Action>>,
    focus:              SharedFocus,
    back:               ThumbButton,
    forward:            ThumbButton,
    live:               SharedLiveState,
    hub:                Option<LiveHub>,
    injector:           Injector,
    injection_warned:   bool,
    unsupported_warned: bool
}

impl ButtonRuntime {
    /// Builds the runtime from the first configured device entry.
    ///
    /// Only the Back and Forward entries of the global button map are
    /// taken; gesture and wheel-mode bindings belong to
    /// [`crate::gesture::GestureRuntime`].
    #[must_use]
    pub fn from_config(config: &Config) -> Self {
        let Some(device) = config.devices.first() else {
            return Self::default();
        };
        let global = [ButtonId::Back, ButtonId::Forward]
            .into_iter()
            .filter_map(|button| {
                device
                    .buttons
                    .get(&button)
                    .map(|action| (button, action.clone()))
            })
            .collect();
        Self {
            global,
            profiles: device.button_profiles.clone(),
            ..Self::default()
        }
    }

    /// Attaches the live telemetry handles used to surface diverted buttons.
    pub fn attach_live(&mut self, live: SharedLiveState, hub: LiveHub) {
        self.live = live;
        self.hub = Some(hub);
    }

    /// Attaches the shared focus slot consulted when resolving mappings.
    pub fn attach_focus(&mut self, focus: SharedFocus) {
        self.focus = focus;
    }

    /// Replaces the global mappings and per-app profiles from a
    /// configuration change; the next [`Self::apply`] reconciles the divert
    /// state with the new [`Self::should_divert`] decision.
    pub fn set_config(
        &mut self,
        global: HashMap<ButtonId, Action>,
        profiles: HashMap<String, HashMap<ButtonId, Action>>
    ) {
        self.global = global;
        self.profiles = profiles;
    }

    /// Whether any thumb button must be diverted for this configuration.
    ///
    /// With an empty configuration nothing is diverted, so Back and Forward
    /// keep working natively until the user binds something.
    #[must_use]
    pub fn should_divert(&self) -> bool {
        self.needs_divert(ButtonId::Back) || self.needs_divert(ButtonId::Forward)
    }

    /// Whether a button is diverted; callers poll this to decide whether the
    /// notification stream is worth reading.
    #[must_use]
    pub const fn wants_pump(&self) -> bool {
        self.back.diverted || self.forward.diverted
    }

    /// Diverts the thumb controls that carry a software mapping after a
    /// device attach or a configuration change.
    ///
    /// Controls that no longer need the divert are released; with an empty
    /// configuration no control is touched at all, so Back and Forward keep
    /// their native firmware behaviour.
    pub fn apply(&mut self, device: &mut MxMaster3s) {
        let back = self.needs_divert(ButtonId::Back);
        let forward = self.needs_divert(ButtonId::Forward);
        if !back {
            self.release_injected(ButtonId::Back);
        }
        if !forward {
            self.release_injected(ButtonId::Forward);
        }
        if divert_one(device, control_for(ButtonId::Back), back, &mut self.back) {
            self.set_live_bit(BTN_BIT_BACK, false);
        }
        if divert_one(
            device,
            control_for(ButtonId::Forward),
            forward,
            &mut self.forward
        ) {
            self.set_live_bit(BTN_BIT_FORWARD, false);
        }
    }

    /// Undiverts every diverted thumb control before a device drop or
    /// daemon exit and clears the live bits it owns.
    pub fn detach(&mut self, device: &mut MxMaster3s) {
        self.release_injected(ButtonId::Back);
        self.release_injected(ButtonId::Forward);
        if undivert_one(device, control_for(ButtonId::Back), &mut self.back) {
            self.set_live_bit(BTN_BIT_BACK, false);
        }
        if undivert_one(device, control_for(ButtonId::Forward), &mut self.forward) {
            self.set_live_bit(BTN_BIT_FORWARD, false);
        }
    }

    /// Handles one diverted-buttons report: Back and Forward edges.
    pub fn on_diverted_buttons(&mut self, pressed: &[u16]) {
        let back = self.on_button_edge(ButtonId::Back, pressed.contains(&control_id::BACK_BUTTON));
        let forward = self.on_button_edge(
            ButtonId::Forward,
            pressed.contains(&control_id::FORWARD_BUTTON)
        );
        self.execute(ButtonId::Back, back);
        self.execute(ButtonId::Forward, forward);
    }

    /// The mapping in force for a button: the focused application's profile
    /// wins per button, anything the profile does not mention falls through
    /// to the global map.
    fn effective_action(&self, button: ButtonId) -> Option<&Action> {
        let focused = self.focused_app();
        focused
            .as_deref()
            .and_then(|app| self.profiles.get(app))
            .and_then(|profile| profile.get(&button))
            .or_else(|| self.global.get(&button))
    }

    /// The class of the currently focused window, if the desktop reports one.
    fn focused_app(&self) -> Option<String> {
        self.focus
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// Tracks one button edge: raises or lowers the live bit and decides
    /// what the press fires. The returned effect is executed separately so
    /// the bookkeeping can be tested without an injector.
    fn on_button_edge(&mut self, button: ButtonId, pressed: bool) -> ButtonEffect {
        let state = *self.slot(button);
        if pressed == state.held {
            return ButtonEffect::Idle;
        }
        self.slot_mut(button).held = pressed;
        self.set_live_bit(live_bit_for(button), pressed);
        if pressed {
            match self.effective_action(button).cloned() {
                Some(Action::Keypress {
                    keys
                }) => ButtonEffect::Chord(keys),
                Some(Action::Command {
                    command
                }) => ButtonEffect::Command(command),
                Some(other) => {
                    if !self.unsupported_warned {
                        self.unsupported_warned = true;
                        warn!(
                            "button action not supported yet: {other:?}; falling back to native"
                        );
                    }
                    self.slot_mut(button).injected = true;
                    ButtonEffect::InjectPress
                }
                None => {
                    self.slot_mut(button).injected = true;
                    ButtonEffect::InjectPress
                }
            }
        } else if state.injected {
            self.slot_mut(button).injected = false;
            ButtonEffect::InjectRelease
        } else {
            ButtonEffect::Idle
        }
    }

    fn execute(&mut self, button: ButtonId, effect: ButtonEffect) {
        match effect {
            ButtonEffect::Idle => {}
            ButtonEffect::InjectPress => {
                if let Err(e) = self.injector.press_button(button) {
                    self.warn_injection(e);
                }
            }
            ButtonEffect::InjectRelease => {
                if let Err(e) = self.injector.release_button(button) {
                    self.warn_injection(e);
                }
            }
            ButtonEffect::Chord(keys) => match self.injector.press_sequence(&keys) {
                Ok(()) => info!("button {button:?} fired chord {}", keys.join("+")),
                Err(e) => self.warn_injection(e)
            },
            ButtonEffect::Command(command) => {
                info!("button {button:?} fired command {command:?}");
                spawn_detached("button-command", &command);
            }
        }
    }

    /// Releases a native press the virtual mouse still holds. The firmware
    /// release no longer reaches the daemon once the control is undiverted,
    /// so the injected press would hang until the daemon exits.
    fn release_injected(&mut self, button: ButtonId) {
        if self.slot(button).injected {
            self.slot_mut(button).injected = false;
            if let Err(e) = self.injector.release_button(button) {
                self.warn_injection(e);
            }
        }
    }

    fn warn_injection(&mut self, error: impl std::fmt::Display) {
        if !self.injection_warned {
            self.injection_warned = true;
            warn!("button injection failed: {error}");
        }
    }

    fn needs_divert(&self, button: ButtonId) -> bool {
        is_remapped(self.global.get(&button))
            || self
                .profiles
                .values()
                .any(|profile| is_remapped(profile.get(&button)))
    }

    fn slot(&self, button: ButtonId) -> &ThumbButton {
        match button {
            ButtonId::Back => &self.back,
            ButtonId::Forward => &self.forward,
            other => unreachable!("{other:?} is not a thumb side button")
        }
    }

    fn slot_mut(&mut self, button: ButtonId) -> &mut ThumbButton {
        match button {
            ButtonId::Back => &mut self.back,
            ButtonId::Forward => &mut self.forward,
            other => unreachable!("{other:?} is not a thumb side button")
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
}

/// Diverts or releases one thumb control to match the configuration;
/// returns whether the control was undiverted by this call.
fn divert_one(device: &mut MxMaster3s, cid: u16, wanted: bool, slot: &mut ThumbButton) -> bool {
    if wanted {
        if !slot.diverted {
            match device.set_control_divert(cid, true, false) {
                Ok(()) => {
                    slot.diverted = true;
                    info!("control {cid:#06x} diverted for software remapping");
                }
                Err(e) => warn!("failed to divert control {cid:#06x}: {e}")
            }
        }
        false
    } else if slot.diverted {
        undivert_one(device, cid, slot)
    } else {
        debug!("control {cid:#06x} stays native");
        let _ = device.set_control_divert(cid, false, false);
        false
    }
}

/// Releases one diverted thumb control; returns whether it was diverted.
fn undivert_one(device: &mut MxMaster3s, cid: u16, slot: &mut ThumbButton) -> bool {
    if !slot.diverted {
        return false;
    }
    slot.diverted = false;
    slot.held = false;
    slot.injected = false;
    if let Err(e) = device.set_control_divert(cid, false, false) {
        warn!("failed to undivert control {cid:#06x}: {e}");
    }
    true
}

#[cfg(test)]
#[path = "buttons_tests.rs"]
mod buttons_tests;
