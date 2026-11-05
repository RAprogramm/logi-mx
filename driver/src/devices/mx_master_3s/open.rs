// SPDX-FileCopyrightText: 2025-2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Device discovery and transport opening: Bolt receiver, USB and Bluetooth.

use std::collections::HashMap;

use masterror::prelude::*;
use tracing::info;

use super::MxMaster3s;
use crate::{
    error::{DeviceErrorKind, Result},
    hidpp::HidppDevice
};

pub(super) const VID_LOGITECH: u16 = 0x046D;
pub(super) const PID_BOLT_RECEIVER: u16 = 0xC548;
pub(super) const PID_MX_MASTER_3S_USB: u16 = 0x4082;
pub(super) const PID_MX_MASTER_3S_BT: u16 = 0xB034;
pub(super) const RECEIVER_SLOTS: [u8; 6] = [1, 2, 3, 4, 5, 6];

impl MxMaster3s {
    /// Creates an instance from an already opened HID++ transport.
    fn new(hidpp: HidppDevice) -> Self {
        Self {
            hidpp,
            button_mappings: HashMap::new()
        }
    }

    /// Opens the mouse paired with a Logi Bolt receiver.
    ///
    /// # Arguments
    ///
    /// * `device_index` - Receiver slot of the paired device, `1`-`6`.
    ///
    /// # Errors
    ///
    /// Returns [`masterror::AppError`] when the receiver is absent, the slot
    /// is empty, or the device does not answer a ping.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use logi_mx_driver::devices::MxMaster3s;
    ///
    /// let device = MxMaster3s::open_bolt_receiver(2)?;
    /// # Ok::<(), masterror::AppError>(())
    /// ```
    pub fn open_bolt_receiver(device_index: u8) -> Result<Self> {
        info!("Opening MX Master 3S via Bolt receiver, device index: {device_index}");

        let mut hidpp = HidppDevice::open_vid_pid(VID_LOGITECH, PID_BOLT_RECEIVER, device_index)?;

        hidpp.ping()?;

        Ok(Self::new(hidpp))
    }

    /// Opens the mouse connected over USB cable.
    ///
    /// # Errors
    ///
    /// Returns [`masterror::AppError`] when no wired device is found or the
    /// device does not answer a ping.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use logi_mx_driver::devices::MxMaster3s;
    ///
    /// let device = MxMaster3s::open_usb()?;
    /// # Ok::<(), masterror::AppError>(())
    /// ```
    pub fn open_usb() -> Result<Self> {
        info!("Opening MX Master 3S via USB");

        let mut hidpp = HidppDevice::open_vid_pid(VID_LOGITECH, PID_MX_MASTER_3S_USB, 0xFF)?;

        hidpp.ping()?;

        Ok(Self::new(hidpp))
    }

    /// Opens the mouse paired over Bluetooth.
    ///
    /// # Errors
    ///
    /// Returns [`masterror::AppError`] when the Bluetooth HID interface is
    /// absent or the device does not answer a ping.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use logi_mx_driver::devices::MxMaster3s;
    ///
    /// let device = MxMaster3s::open_bluetooth()?;
    /// # Ok::<(), masterror::AppError>(())
    /// ```
    pub fn open_bluetooth() -> Result<Self> {
        info!("Opening MX Master 3S via Bluetooth");

        let mut hidpp = HidppDevice::open_vid_pid(VID_LOGITECH, PID_MX_MASTER_3S_BT, 0xFF)?;

        hidpp.ping()?;

        Ok(Self::new(hidpp))
    }

    /// Opens the first MX Master 3S paired to a Logi Bolt receiver.
    ///
    /// Probes every receiver slot in order and returns the first paired
    /// device that answers a ping, so callers do not need to know which slot
    /// the mouse occupies.
    ///
    /// # Errors
    ///
    /// Returns [`masterror::AppError`] when no receiver is present or no
    /// paired device responds in any slot.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use logi_mx_driver::devices::{MouseDevice, MxMaster3s};
    ///
    /// let mut device = MxMaster3s::open_bolt_receiver_discovered()?;
    /// println!("{}", device.device_name()?);
    /// # Ok::<(), masterror::AppError>(())
    /// ```
    pub fn open_bolt_receiver_discovered() -> Result<Self> {
        info!("Discovering MX Master 3S on Bolt receiver slots");

        // Resolve the receiver interface once instead of re-enumerating HID
        // devices (and rebuilding the HidApi) for every candidate slot.
        let api = hidapi::HidApi::new()
            .map_err(|e| AppError::internal("Failed to initialize HID API").with_source(e))?;

        let mut target_path = None;
        for device_info in api.device_list() {
            if device_info.vendor_id() == VID_LOGITECH
                && device_info.product_id() == PID_BOLT_RECEIVER
                && (device_info.interface_number() == 2 || device_info.interface_number() == -1)
            {
                target_path = Some(device_info.path().to_string_lossy().into_owned());
                break;
            }
        }

        let path = target_path
            .ok_or_else(|| AppError::not_found("HID++ interface not found for device"))?;

        for index in RECEIVER_SLOTS {
            if let Ok(mut hidpp) = HidppDevice::open_path(&path, index)
                && hidpp.ping().is_ok()
            {
                return Ok(Self::new(hidpp));
            }
        }

        Err(DeviceErrorKind::NotFound.into())
    }
}
