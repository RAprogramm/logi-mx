// SPDX-FileCopyrightText: 2025-2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Device handle pool: opening, caching and configuring the single tracked
//! receiver, plus the stored-configuration writes that ride on it.

use std::{collections::HashMap, sync::Mutex};

use logi_mx_driver::prelude::*;
use tracing::{debug, error, info, warn};

use crate::ipc;

/// Map key for a handle opened by discovery rather than a udev event.
const DISCOVERED_KEY: &str = "discovered";

/// Cache of open device handles and the stored configuration applied to
/// them. The pool tracks exactly one device: a second handle on the same
/// receiver would split the HID++ response stream.
pub(super) struct DevicePool {
    devices:        Mutex<HashMap<String, MxMaster3s>>,
    config:         Config,
    preferred_slot: Option<u8>
}

impl DevicePool {
    /// Creates an empty pool bound to the given stored configuration.
    pub(super) fn new(config: Config) -> Self {
        Self {
            devices: Mutex::new(HashMap::new()),
            config,
            preferred_slot: None
        }
    }

    /// Opens the discovered device, applies its configuration and stores the
    /// handle keyed by hidraw path.
    ///
    /// The pool tracks exactly one device: a second handle on the same
    /// receiver would split the HID++ response stream. A handle cached by
    /// discovery is adopted under the real path instead of being reopened.
    pub(super) fn handle_device_added(&mut self, device_path: String) {
        info!("Device added: {device_path}");

        {
            let mut guard = self
                .devices
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if guard.contains_key(&device_path) {
                debug!("Device at {device_path} is already tracked");
                return;
            }
            if guard.len() == 1
                && let Some(device) = guard.remove(DISCOVERED_KEY)
            {
                debug!("Adopted the discovered handle for {device_path}");
                guard.insert(device_path, device);
                return;
            }
            if !guard.is_empty() {
                debug!("A device is already tracked; ignoring extra node {device_path}");
                return;
            }
        }

        match self.open_device() {
            Ok(mut device) => {
                if let Ok(name) = device.device_name() {
                    info!("Detected: {name}");

                    if let Some(device_config) = self
                        .config
                        .devices
                        .iter()
                        .find(|d| name.starts_with(&d.name))
                    {
                        info!("Applying configuration for {name}");
                        Self::apply_config(&mut device, device_config);
                    }
                } else {
                    debug!("Device opened but name read failed; storing handle anyway");
                }

                self.devices
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .insert(device_path, device);
            }
            Err(e) => {
                warn!("Failed to open device: {e}");
            }
        }
    }

    /// Opens a device, preferring the last successful receiver slot.
    fn open_device(&mut self) -> std::result::Result<MxMaster3s, masterror::AppError> {
        if let Some(slot) = self.preferred_slot
            && let Ok(device) = MxMaster3s::open_bolt_receiver(slot)
        {
            return Ok(device);
        }

        for slot in 1u8..=6 {
            if let Ok(device) = MxMaster3s::open_bolt_receiver(slot) {
                self.preferred_slot = Some(slot);
                return Ok(device);
            }
        }

        Err(DeviceErrorKind::NotFound.into())
    }

    /// Runs a closure against a cached device, discovering one if needed.
    pub(super) fn with_device<R, F>(&mut self, f: F) -> std::result::Result<R, masterror::AppError>
    where
        F: FnOnce(&mut MxMaster3s) -> std::result::Result<R, masterror::AppError>
    {
        {
            let mut guard = self
                .devices
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(device) = guard.values_mut().next() {
                return f(device);
            }
        }

        let mut device = self.open_device()?;
        if let Ok(name) = device.device_name() {
            info!("Detected: {name}");
            if let Some(device_config) = self
                .config
                .devices
                .iter()
                .find(|d| name.starts_with(&d.name))
            {
                info!("Applying configuration for {name}");
                Self::apply_config(&mut device, device_config);
            }
        }

        let mut guard = self
            .devices
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let device = guard.entry(DISCOVERED_KEY.to_string()).or_insert(device);
        let result = f(device);
        drop(guard);
        result
    }

    /// Runs a closure against every cached device handle, if any.
    ///
    /// Never discovers a device: used by the periodic gesture pump, which
    /// must stay cheap when no device is connected.
    pub(super) fn for_each_cached_mut(&self, mut f: impl FnMut(&mut MxMaster3s)) {
        let mut guard = self
            .devices
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for device in guard.values_mut() {
            f(device);
        }
    }

    /// Reads the gesture opt-in, bindings and per-app profiles from the
    /// stored configuration.
    pub(super) fn gesture_config(
        &self
    ) -> (
        bool,
        Vec<Gesture>,
        std::collections::HashMap<String, Vec<Gesture>>
    ) {
        let Some(device) = self.config.devices.first() else {
            return (false, Vec::new(), std::collections::HashMap::new());
        };
        let bindings = match device.buttons.get(&ButtonId::ThumbGesture) {
            Some(Action::Gestures {
                gestures
            }) => gestures.clone(),
            _ => Vec::new()
        };
        (
            device.gestures_enabled,
            bindings,
            device.gesture_profiles.clone()
        )
    }

    /// Writes the gesture opt-in, bindings and profiles into the
    /// configuration and persists it to disk.
    ///
    /// # Errors
    ///
    /// Returns an error when no device entry exists or the file cannot be
    /// written.
    pub(super) fn set_gestures(
        &mut self,
        enabled: bool,
        gestures: Vec<Gesture>,
        profiles: std::collections::HashMap<String, Vec<Gesture>>
    ) -> std::result::Result<(), masterror::AppError> {
        let Some(device) = self.config.devices.first_mut() else {
            return Err(masterror::AppError::bad_request("No configured device"));
        };
        device.gestures_enabled = enabled;
        device.gesture_profiles = profiles;
        device.buttons.insert(
            ButtonId::ThumbGesture,
            Action::Gestures {
                gestures
            }
        );
        save_config(&self.config)
    }

    /// Reads the global Back and Forward mappings and the per-app button
    /// profiles from the stored configuration.
    pub(super) fn button_config(
        &self
    ) -> (
        HashMap<ButtonId, Action>,
        HashMap<String, HashMap<ButtonId, Action>>
    ) {
        let Some(device) = self.config.devices.first() else {
            return (HashMap::new(), HashMap::new());
        };
        let buttons = [ButtonId::Back, ButtonId::Forward]
            .into_iter()
            .filter_map(|button| {
                device
                    .buttons
                    .get(&button)
                    .map(|action| (button, action.clone()))
            })
            .collect();
        (buttons, device.button_profiles.clone())
    }

    /// Mirrors a button remap request into the stored configuration and
    /// persists it, so software remaps survive a daemon restart.
    ///
    /// The mutation itself lives in [`super::rpc::apply_setting`], the same
    /// pure mapping used for device settings.
    ///
    /// # Errors
    ///
    /// Returns an error when no device entry exists, the request changes no
    /// button configuration, or the file cannot be written.
    pub(super) fn set_button_request(
        &mut self,
        request: &ipc::Request
    ) -> std::result::Result<(), masterror::AppError> {
        if self.config.devices.is_empty() {
            return Err(masterror::AppError::bad_request("No configured device"));
        }
        if !super::rpc::apply_setting(&mut self.config, request) {
            return Err(masterror::AppError::bad_request(
                "Request changes no button configuration"
            ));
        }
        save_config(&self.config)
    }

    /// Reads the radial pie menu tuning from the stored configuration,
    /// falling back to the schema defaults when no device entry exists.
    pub(super) fn pie_config(&self) -> PieConfig {
        self.config
            .devices
            .first()
            .map_or_else(PieConfig::default, |device| device.pie.clone())
    }

    /// Mirrors a pie menu tuning request into the stored configuration and
    /// persists it, so the tuning survives a daemon restart.
    ///
    /// # Errors
    ///
    /// Returns an error when no device entry exists, the request changes no
    /// pie configuration, or the file cannot be written.
    pub(super) fn set_pie_request(
        &mut self,
        request: &ipc::Request
    ) -> std::result::Result<(), masterror::AppError> {
        if self.config.devices.is_empty() {
            return Err(masterror::AppError::bad_request("No configured device"));
        }
        if !super::rpc::apply_setting(&mut self.config, request) {
            return Err(masterror::AppError::bad_request(
                "Request changes no pie configuration"
            ));
        }
        save_config(&self.config)
    }

    /// Mirrors a successful device setting into the stored configuration and
    /// persists it, so DPI, `SmartShift` and hi-res choices survive a daemon
    /// restart instead of being rolled back by the next `apply_config`.
    pub(super) fn persist_setting(&mut self, request: &ipc::Request) {
        if super::rpc::apply_setting(&mut self.config, request)
            && let Err(e) = save_config(&self.config)
        {
            warn!("Failed to persist configuration: {e}");
        }
    }

    /// The hidraw path of the tracked device, when a real path is cached.
    /// The synthetic discovery key is not a path and is never reported.
    pub(super) fn tracked_path(&self) -> Option<String> {
        let guard = self
            .devices
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        guard
            .keys()
            .find(|key| key.as_str() != DISCOVERED_KEY)
            .cloned()
    }

    /// Drops the stored device handle for a removed hidraw path.
    ///
    /// The cached handles are keyed by hidraw path, but RPC discovery stores
    /// under a synthetic key, so any removal invalidates the whole cache to
    /// avoid talking to a detached device.
    pub(super) fn handle_device_removed(&mut self, device_path: &str) {
        info!("Device removed: {device_path}");
        self.devices
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
        self.preferred_slot = None;
    }

    /// Writes DPI, `SmartShift` and hi-res settings onto the device.
    ///
    /// Individual failures are logged and skipped so one unsupported feature
    /// does not block the remaining settings.
    fn apply_config(device: &mut MxMaster3s, config: &DeviceConfig) {
        debug!("Setting DPI to {}", config.dpi);
        if let Err(e) = device.set_dpi(config.dpi) {
            error!("Failed to set DPI: {e}");
        }

        debug!(
            "Setting SmartShift: enabled={}, threshold={}",
            config.smartshift.enabled, config.smartshift.threshold
        );
        if let Err(e) = device.set_smartshift(config.smartshift) {
            error!("Failed to set SmartShift: {e}");
        }

        debug!(
            "Setting hi-res scroll: enabled={}, inverted={}",
            config.hiresscroll.enabled, config.hiresscroll.inverted
        );
        if let Err(e) = device.set_hires_scroll(config.hiresscroll) {
            error!("Failed to set hi-res scroll: {e}");
        }

        for (button, action) in &config.buttons {
            debug!("Setting button {button:?} to action {action:?}");
            if let Err(e) = device.set_button_action(*button, action.clone()) {
                error!("Failed to set button action: {e}");
            }
        }

        info!("Configuration applied successfully");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_manager_new_empty() {
        let manager = DevicePool::new(Config::default());
        assert!(
            manager
                .devices
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .is_empty()
        );
    }
}
