// SPDX-FileCopyrightText: 2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! uinput key and mouse-button injection for bound actions.
//!
//! Gesture bindings press key combinations (for example `KEY_LEFTCTRL` +
//! `KEY_RIGHT`) through a virtual keyboard created on demand, and native
//! passthrough of the diverted thumb buttons presses the matching mouse
//! buttons through a virtual mouse. The devices are built lazily on the
//! first injection, so a session without permission on `/dev/uinput` keeps
//! the daemon fully functional: injection reports the reason instead of
//! failing at startup, and the next attempt retries.

use std::{thread::sleep, time::Duration};

use evdev::{
    AttributeSet, EventType, InputEvent, KeyCode,
    uinput::{VirtualDevice, VirtualDeviceBuilder}
};
use logi_mx_driver::{devices::ButtonId, keys::KEY_NAMES};
use masterror::AppError;
use tracing::warn;

/// Result alias for injection operations failing with [`AppError`].
type Result<T> = std::result::Result<T, AppError>;

/// Name advertised by the virtual keyboard; device discovery and telemetry
/// must skip input nodes carrying this name to avoid feeding events back.
pub const INJECTOR_NAME: &str = "logi-mx virtual keyboard";

/// Name advertised by the virtual mouse carrying injected button presses.
pub const MOUSE_INJECTOR_NAME: &str = "logi-mx virtual mouse";

/// Time the combination stays held between the press and release waves, so
/// every toolkit observes the keys as a deliberate stroke, not a race.
const KEY_HOLD: Duration = Duration::from_millis(2);

/// Kernel code of the rear thumb button.
const BTN_SIDE: u16 = 275;
/// Kernel code of the front thumb button.
const BTN_EXTRA: u16 = 276;

/// Maps a thumb side button onto the kernel button code the firmware would
/// report for a native press.
///
/// The kernel orders `BTN_SIDE` as the rear thumb button and `BTN_EXTRA`
/// the front one, while the physical labels run the other way: the front
/// button is Back and the rear one is Forward, so the codes cross over.
///
/// # Arguments
///
/// * `button` - Physical button to resolve.
///
/// # Returns
///
/// The evdev code for Back and Forward, `None` for every other button.
///
/// # Examples
///
/// ```
/// use logi_mx_daemon::inject::native_button_code;
/// use logi_mx_driver::devices::ButtonId;
///
/// assert_eq!(native_button_code(ButtonId::Back), Some(276));
/// assert_eq!(native_button_code(ButtonId::Forward), Some(275));
/// assert_eq!(native_button_code(ButtonId::LeftClick), None);
/// ```
#[must_use]
pub const fn native_button_code(button: ButtonId) -> Option<u16> {
    match button {
        ButtonId::Back => Some(BTN_EXTRA),
        ButtonId::Forward => Some(BTN_SIDE),
        _ => None
    }
}

/// Resolves a list of `KEY_*` names into evdev codes, preserving order.
///
/// # Errors
///
/// Returns [`AppError`] when the list is empty or contains a name the kernel
/// does not define.
///
/// # Examples
///
/// ```
/// use logi_mx_daemon::inject::resolve_sequence;
///
/// let codes = resolve_sequence(&["KEY_LEFTCTRL".into(), "KEY_C".into()]).unwrap();
/// assert_eq!(codes, vec![29, 46]);
/// ```
pub fn resolve_sequence(keys: &[String]) -> Result<Vec<u16>> {
    if keys.is_empty() {
        return Err(AppError::bad_request(
            "Key sequence must name at least one key"
        ));
    }
    let mut codes = Vec::with_capacity(keys.len());
    for name in keys {
        let code = logi_mx_driver::keys::key_code(name)
            .ok_or_else(|| AppError::bad_request(format!("Unknown key name: {name}")))?;
        codes.push(code);
    }
    Ok(codes)
}

/// Injects key combinations and mouse button presses through lazily created
/// uinput devices.
///
/// The virtual devices are built on the first successful injection and
/// reused afterwards; a failed build is logged once and retried on the next
/// call, so granting uinput permission mid-session heals the injector
/// without a restart.
#[derive(Default)]
pub struct Injector {
    device:               Option<VirtualDevice>,
    mouse:                Option<VirtualDevice>,
    failure_logged:       bool,
    mouse_failure_logged: bool
}

impl Injector {
    /// Presses the given `KEY_*` combination in order, holds it briefly and
    /// releases it in reverse order, as a keyboard would.
    ///
    /// # Errors
    ///
    /// Returns [`AppError`] when a name is unknown, the virtual keyboard
    /// cannot be created, or an event cannot be written.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use logi_mx_daemon::inject::Injector;
    ///
    /// let mut injector = Injector::default();
    /// injector.press_sequence(&["KEY_LEFTCTRL".into(), "KEY_RIGHT".into()])?;
    /// # Ok::<(), masterror::AppError>(())
    /// ```
    pub fn press_sequence(&mut self, keys: &[String]) -> Result<()> {
        let codes = resolve_sequence(keys)?;
        let device = self.ensure_device()?;
        for code in &codes {
            emit_key(device, *code, 1)?;
        }
        sleep(KEY_HOLD);
        for code in codes.iter().rev() {
            emit_key(device, *code, 0)?;
        }
        Ok(())
    }

    fn ensure_device(&mut self) -> Result<&mut VirtualDevice> {
        if self.device.is_none() {
            match build_keyboard() {
                Ok(device) => self.device = Some(device),
                Err(error) => {
                    if !self.failure_logged {
                        self.failure_logged = true;
                        warn!("gesture injection unavailable: {error}");
                    }
                    return Err(error);
                }
            }
        }
        self.device
            .as_mut()
            .ok_or_else(|| AppError::internal("uinput keyboard is unavailable"))
    }

    /// Presses the native kernel button matching a thumb side button, so a
    /// diverted control keeps behaving exactly like its firmware press.
    ///
    /// # Arguments
    ///
    /// * `button` - `ButtonId::Back` or `ButtonId::Forward`.
    ///
    /// # Errors
    ///
    /// Returns [`AppError`] when the button has no native code, the virtual
    /// mouse cannot be created, or an event cannot be written.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use logi_mx_daemon::inject::Injector;
    /// use logi_mx_driver::devices::ButtonId;
    ///
    /// let mut injector = Injector::default();
    /// injector.press_button(ButtonId::Back)?;
    /// injector.release_button(ButtonId::Back)?;
    /// # Ok::<(), masterror::AppError>(())
    /// ```
    pub fn press_button(&mut self, button: ButtonId) -> Result<()> {
        self.button_event(button, 1)
    }

    /// Releases a button previously pressed through [`Self::press_button`].
    ///
    /// # Arguments
    ///
    /// * `button` - `ButtonId::Back` or `ButtonId::Forward`.
    ///
    /// # Errors
    ///
    /// Returns [`AppError`] when the button has no native code, the virtual
    /// mouse cannot be created, or an event cannot be written.
    pub fn release_button(&mut self, button: ButtonId) -> Result<()> {
        self.button_event(button, 0)
    }

    fn button_event(&mut self, button: ButtonId, value: i32) -> Result<()> {
        let code = native_button_code(button).ok_or_else(|| {
            AppError::bad_request(format!("No native button code for {button:?}"))
        })?;
        let mouse = self.ensure_mouse()?;
        emit_key(mouse, code, value)
    }

    fn ensure_mouse(&mut self) -> Result<&mut VirtualDevice> {
        if self.mouse.is_none() {
            match build_mouse() {
                Ok(device) => self.mouse = Some(device),
                Err(error) => {
                    if !self.mouse_failure_logged {
                        self.mouse_failure_logged = true;
                        warn!("button injection unavailable: {error}");
                    }
                    return Err(error);
                }
            }
        }
        self.mouse
            .as_mut()
            .ok_or_else(|| AppError::internal("uinput mouse is unavailable"))
    }
}

/// Creates the virtual keyboard advertising every named evdev key code.
fn build_keyboard() -> Result<VirtualDevice> {
    let mut keys = AttributeSet::new();
    for (code, _) in KEY_NAMES {
        keys.insert(KeyCode(code));
    }
    evdev::uinput::VirtualDevice::builder()
        .and_then(|builder| builder.name(INJECTOR_NAME).with_keys(&keys))
        .and_then(VirtualDeviceBuilder::build)
        .map_err(|e| AppError::internal("Failed to create the uinput keyboard").with_source(e))
}

/// Creates the virtual mouse advertising the two thumb side buttons.
///
/// Kernel button codes live in the `KEY` event namespace (`BTN_*` are
/// `KeyCode`s), and the device deliberately carries no pointer axes: the
/// live-telemetry discovery looks for `REL_X` and therefore never mistakes
/// this node for the physical mouse.
fn build_mouse() -> Result<VirtualDevice> {
    let mut buttons = AttributeSet::new();
    buttons.insert(KeyCode(BTN_SIDE));
    buttons.insert(KeyCode(BTN_EXTRA));
    evdev::uinput::VirtualDevice::builder()
        .and_then(|builder| builder.name(MOUSE_INJECTOR_NAME).with_keys(&buttons))
        .and_then(VirtualDeviceBuilder::build)
        .map_err(|e| AppError::internal("Failed to create the uinput mouse").with_source(e))
}

/// Writes one key event; uinput appends the synchronisation marker itself.
fn emit_key(device: &mut VirtualDevice, code: u16, value: i32) -> Result<()> {
    device
        .emit(&[InputEvent::new_now(EventType::KEY.0, code, value)])
        .map_err(|e| AppError::internal("Failed to inject a key event").with_source(e))
}

#[cfg(test)]
#[path = "inject_tests.rs"]
mod inject_tests;
