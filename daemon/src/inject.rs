// SPDX-FileCopyrightText: 2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! uinput key injection for gesture actions.
//!
//! Gesture bindings press key combinations (for example `KEY_LEFTCTRL` +
//! `KEY_RIGHT`) through a virtual keyboard created on demand. The device is
//! built lazily on the first injection, so a session without permission on
//! `/dev/uinput` keeps the daemon fully functional: injection reports the
//! reason instead of failing at startup, and the next attempt retries.

use std::{thread::sleep, time::Duration};

use evdev::{
    AttributeSet, EventType, InputEvent, KeyCode,
    uinput::{VirtualDevice, VirtualDeviceBuilder}
};
use logi_mx_driver::keys::KEY_NAMES;
use masterror::AppError;
use tracing::warn;

/// Result alias for injection operations failing with [`AppError`].
type Result<T> = std::result::Result<T, AppError>;

/// Name advertised by the virtual keyboard; device discovery and telemetry
/// must skip input nodes carrying this name to avoid feeding events back.
pub const INJECTOR_NAME: &str = "logi-mx virtual keyboard";

/// Time the combination stays held between the press and release waves, so
/// every toolkit observes the keys as a deliberate stroke, not a race.
const KEY_HOLD: Duration = Duration::from_millis(2);

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

/// Injects key combinations through a lazily created uinput keyboard.
///
/// The virtual device is built on the first successful injection and reused
/// afterwards; a failed build is remembered as [`Injector::failure`] and
/// retried on the next call, so granting uinput permission mid-session heals
/// the injector without a restart.
#[derive(Default)]
pub struct Injector {
    device:         Option<VirtualDevice>,
    failure:        Option<String>,
    failure_logged: bool
}

impl Injector {
    /// Creates an injector without touching `/dev/uinput`.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            device:         None,
            failure:        None,
            failure_logged: false
        }
    }

    /// Description of the last device-creation failure, if any.
    #[must_use]
    pub fn failure(&self) -> Option<&str> {
        self.failure.as_deref()
    }

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
    /// let mut injector = Injector::new();
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
                Ok(device) => {
                    self.failure = None;
                    self.device = Some(device);
                }
                Err(error) => {
                    if !self.failure_logged {
                        self.failure_logged = true;
                        warn!("gesture injection unavailable: {error}");
                    }
                    self.failure = Some(error.to_string());
                    return Err(error);
                }
            }
        }
        self.device
            .as_mut()
            .ok_or_else(|| AppError::internal("uinput keyboard is unavailable"))
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

/// Writes one key event; uinput appends the synchronisation marker itself.
fn emit_key(device: &mut VirtualDevice, code: u16, value: i32) -> Result<()> {
    device
        .emit(&[InputEvent::new_now(EventType::KEY.0, code, value)])
        .map_err(|e| AppError::internal("Failed to inject a key event").with_source(e))
}

#[cfg(test)]
#[path = "inject_tests.rs"]
mod inject_tests;
