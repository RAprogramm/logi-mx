// SPDX-FileCopyrightText: 2025-2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Reprogrammable controls: enumeration, diversion and firmware remapping.

use tracing::{debug, info};

use super::{MxMaster3s, ReprogControl, control_id_name};
use crate::{
    error::Result,
    hidpp::{
        FEATURE_REPROG_CONTROLS, HidppPacket, ReprogControlsFunction, ReprogNotification,
        decode_reprog_notification
    }
};

impl MxMaster3s {
    /// Enumerates all reprogrammable controls reported by feature 0x1B04.
    ///
    /// # Errors
    ///
    /// Returns [`masterror::AppError`] when the feature is unsupported or a
    /// control info request fails.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use logi_mx_driver::devices::{MouseDevice, MxMaster3s};
    ///
    /// let mut device = MxMaster3s::open_bolt_receiver_discovered()?;
    /// for control in device.list_reprog_controls()? {
    ///     println!("{:?} -> {}", control.control_id, control.control_id);
    /// }
    /// # Ok::<(), masterror::AppError>(())
    /// ```
    pub fn list_reprog_controls(&mut self) -> Result<Vec<ReprogControl>> {
        let feature_index = self.hidpp.feature_index(FEATURE_REPROG_CONTROLS)?;

        let count_response = self.hidpp.send_command(
            feature_index,
            ReprogControlsFunction::GetControlCount as u8,
            &[]
        )?;

        let count = count_response.parameters().first().copied().unwrap_or(0);

        let mut controls = Vec::with_capacity(usize::from(count));

        for index in 0..count {
            let response = self.hidpp.send_command(
                feature_index,
                ReprogControlsFunction::GetControlInfo as u8,
                &[index, 0x00, 0x00]
            )?;

            let params = response.parameters();
            let control = ReprogControl {
                control_id:      params
                    .get(..2)
                    .and_then(|pair| <[u8; 2]>::try_from(pair).ok())
                    .map_or(0, u16::from_be_bytes),
                task_id:         params
                    .get(2..4)
                    .and_then(|pair| <[u8; 2]>::try_from(pair).ok())
                    .map_or(0, u16::from_be_bytes),
                flags:           params.get(4).copied().unwrap_or(0),
                position:        params.get(5).copied().unwrap_or(0),
                group:           params.get(6).copied().unwrap_or(0),
                group_mask:      params.get(7).copied().unwrap_or(0),
                gesture_capable: params.get(8).copied().unwrap_or(0)
            };

            debug!(
                "Control {} ({}) task {:04X} flags {:#04x} group {}",
                control.control_id,
                control_id_name(control.control_id),
                control.task_id,
                control.flags,
                control.group
            );

            controls.push(control);
        }

        Ok(controls)
    }

    /// Reads the reporting configuration of one control by CID.
    ///
    /// Returns `(divert_flags, remap_control_id)`; a zero `divert_flags`
    /// value means the control currently behaves natively.
    ///
    /// # Arguments
    ///
    /// * `control_id` - HID++ control ID, e.g. `0x00C3` for the gesture button.
    ///
    /// # Errors
    ///
    /// Returns [`masterror::AppError`] when the feature is unsupported or
    /// the request fails.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use logi_mx_driver::devices::{MouseDevice, MxMaster3s};
    ///
    /// let mut device = MxMaster3s::open_bolt_receiver_discovered()?;
    /// let (flags, remap) = device.control_divert(0x00C3)?;
    /// println!("flags {flags:#04x}, remap {remap:#06x}");
    /// # Ok::<(), masterror::AppError>(())
    /// ```
    pub fn control_divert(&mut self, control_id: u16) -> Result<(u8, u16)> {
        let feature_index = self.hidpp.feature_index(FEATURE_REPROG_CONTROLS)?;

        let response = self.hidpp.send_command(
            feature_index,
            ReprogControlsFunction::GetControlReporting as u8,
            &[(control_id >> 8) as u8, (control_id & 0xFF) as u8, 0x00]
        )?;

        let params = response.parameters();
        let flags = params.get(2).copied().unwrap_or(0);
        let remap = params
            .get(3..5)
            .and_then(|pair| <[u8; 2]>::try_from(pair).ok())
            .map_or(0, u16::from_be_bytes);

        debug!("Control {control_id:04X} reporting: flags {flags:#04x}, remap {remap:04X}");
        Ok((flags, remap))
    }

    /// Applies a reporting configuration to one control by CID.
    ///
    /// With `divert` set, the device stops performing the native action and
    /// emits diverted button events to the host instead. Clearing `divert`
    /// restores native behaviour. Raw XY diversion additionally streams raw
    /// pointer deltas for gesture support.
    ///
    /// Diverted controls produce no normal HID events until the diversion is
    /// cleared; callers must own the event pump that consumes them.
    ///
    /// # Arguments
    ///
    /// * `control_id` - HID++ control ID.
    /// * `divert` - Whether to divert the control to HID++ notifications.
    /// * `raw_xy` - Whether to also stream raw pointer deltas.
    ///
    /// # Errors
    ///
    /// Returns [`masterror::AppError`] when the feature is unsupported or
    /// the request fails.
    pub fn set_control_divert(
        &mut self,
        control_id: u16,
        divert: bool,
        raw_xy: bool
    ) -> Result<()> {
        let feature_index = self.hidpp.feature_index(FEATURE_REPROG_CONTROLS)?;

        let mut flags = 0u8;
        if divert {
            flags |= 0x03;
        }
        if raw_xy {
            flags |= 0x30;
        }

        let (_, remap_to) = self.control_divert(control_id).unwrap_or((0, 0x0000));

        self.hidpp.send_command(
            feature_index,
            ReprogControlsFunction::SetControlReporting as u8,
            &[
                (control_id >> 8) as u8,
                (control_id & 0xFF) as u8,
                flags,
                (remap_to >> 8) as u8,
                (remap_to & 0xFF) as u8
            ]
        )?;

        info!(
            "Control {control_id:04X} divert: {}, raw_xy: {raw_xy}",
            if divert { "enabled" } else { "disabled" }
        );
        Ok(())
    }

    /// Remaps one control to another native control, keeping its divert flags.
    ///
    /// The device itself performs the remap: pressing `control_id` behaves
    /// exactly as if `target_cid` were pressed, with no host-side daemon
    /// involved. Pass `0x0000` as `target_cid` to restore native behaviour.
    ///
    /// # Arguments
    ///
    /// * `control_id` - Physical control to remap (CID).
    /// * `target_cid` - Control the press should act as, or `0x0000` to clear.
    ///
    /// # Errors
    ///
    /// Returns [`masterror::AppError`] when the feature is unsupported or
    /// the request fails.
    pub fn set_control_remap(&mut self, control_id: u16, target_cid: u16) -> Result<()> {
        let feature_index = self.hidpp.feature_index(FEATURE_REPROG_CONTROLS)?;
        let (flags, _) = self.control_divert(control_id)?;

        self.hidpp.send_command(
            feature_index,
            ReprogControlsFunction::SetControlReporting as u8,
            &[
                (control_id >> 8) as u8,
                (control_id & 0xFF) as u8,
                flags,
                (target_cid >> 8) as u8,
                (target_cid & 0xFF) as u8
            ]
        )?;

        info!("Control {control_id:04X} remapped to {target_cid:04X}");
        Ok(())
    }

    /// Reads one pending Reprogrammable Controls notification, if any.
    ///
    /// Diverted controls stream unsolicited reports while held: pressed
    /// control IDs and raw pointer deltas. This drains a single report,
    /// waiting up to `timeout_ms` (`0` polls without blocking) and returning
    /// `None` on timeout, transport failure, or reports of other features,
    /// so callers can poll it cheaply from an event loop.
    ///
    /// # Arguments
    ///
    /// * `timeout_ms` - Read timeout in milliseconds.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use logi_mx_driver::devices::MxMaster3s;
    ///
    /// let mut device = MxMaster3s::open_bolt_receiver_discovered()?;
    /// while let Some(note) = device.read_reprog_notification(0) {
    ///     println!("{note:?}");
    /// }
    /// # Ok::<(), masterror::AppError>(())
    /// ```
    #[must_use]
    pub fn read_reprog_notification(&mut self, timeout_ms: i32) -> Option<ReprogNotification> {
        let feature_index = self.hidpp.feature_index(FEATURE_REPROG_CONTROLS).ok()?;
        let bytes = self.hidpp.read_raw(timeout_ms)?;
        let packet = HidppPacket::from_bytes(&bytes).ok()?;
        decode_reprog_notification(feature_index, &packet)
    }
}
