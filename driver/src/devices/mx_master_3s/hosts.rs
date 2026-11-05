// SPDX-FileCopyrightText: 2025-2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Easy-Switch host reporting and switching.

use tracing::{debug, info};

use super::MxMaster3s;
use crate::{
    error::Result,
    hidpp::{ChangeHostFunction, FEATURE_CHANGE_HOST}
};

impl MxMaster3s {
    /// Reads the host count and current host index from the `ChangeHost`
    /// feature (Easy-Switch).
    ///
    /// # Errors
    ///
    /// Returns [`masterror::AppError`] when the feature is unsupported or
    /// the response cannot be parsed.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use logi_mx_driver::devices::{MouseDevice, MxMaster3s};
    ///
    /// let mut device = MxMaster3s::open_bolt_receiver_discovered()?;
    /// let (hosts, current) = device.host_info()?;
    /// println!("host {current} of {hosts}");
    /// # Ok::<(), masterror::AppError>(())
    /// ```
    pub fn host_info(&mut self) -> Result<(u8, u8)> {
        let feature_index = self.hidpp.feature_index(FEATURE_CHANGE_HOST)?;

        let response =
            self.hidpp
                .send_command(feature_index, ChangeHostFunction::GetHostInfo as u8, &[])?;

        let params = response.parameters();
        let host_count = params.first().copied().unwrap_or(0);
        let current_host = params.get(1).copied().unwrap_or(0);

        debug!("Host info: {current_host} of {host_count}");
        Ok((host_count, current_host))
    }

    /// Switches to the host at a zero-based index (Easy-Switch).
    ///
    /// # Errors
    ///
    /// Returns [`masterror::AppError`] when the feature is unsupported or the
    /// command fails.
    pub fn set_host(&mut self, host: u8) -> Result<()> {
        let feature_index = self.hidpp.feature_index(FEATURE_CHANGE_HOST)?;
        self.hidpp.send_command(
            feature_index,
            ChangeHostFunction::SetHost as u8,
            &[host, 0x00, 0x00]
        )?;
        info!("Switched to host {host}");
        Ok(())
    }
}
