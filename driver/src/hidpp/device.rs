// SPDX-FileCopyrightText: 2025 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

use std::{collections::HashMap, ffi::CString, time::Duration};

use hidapi::{HidApi, HidDevice};
use masterror::{field, prelude::*};
use tracing::{debug, trace, warn};

use super::{
    RootFunction,
    constants::{
        ERROR_ALREADY_EXISTS, ERROR_BUSY, ERROR_CONNECT_FAIL, ERROR_INVALID_ADDRESS,
        ERROR_INVALID_SUBID, ERROR_INVALID_VALUE, ERROR_REQUEST_UNAVAILABLE, ERROR_RESOURCE_ERROR,
        ERROR_TOO_MANY_DEVICES, ERROR_UNKNOWN_DEVICE, ERROR_UNSUPPORTED_PARAM,
        ERROR_WRONG_PIN_CODE, ROOT_INDEX
    },
    packet::HidppPacket
};
use crate::error::{DeviceErrorKind, Result};

const DEFAULT_TIMEOUT_MS: i32 = 1000;
const RETRY_COUNT: usize = 3;

/// HID++ 2.0 transport over a `hidapi` handle.
///
/// Wraps raw report I/O with retry logic, HID++ error mapping and a feature
/// index cache so higher layers only deal with feature IDs.
///
/// # Examples
///
/// ```no_run
/// use logi_mx_driver::hidpp::{FEATURE_ROOT, HidppDevice};
///
/// let mut device = HidppDevice::open_vid_pid(0x046D, 0xC548, 2)?;
/// let root = device.get_feature_index(FEATURE_ROOT)?;
/// println!("root feature at index {}", root);
/// # Ok::<(), masterror::AppError>(())
/// ```
pub struct HidppDevice {
    device:        HidDevice,
    device_index:  u8,
    feature_cache: HashMap<u16, u8>,
    software_id:   u8
}

impl HidppDevice {
    /// Opens a HID++ device by explicit filesystem path.
    ///
    /// # Arguments
    ///
    /// * `path` - hidraw device path, e.g. `/dev/hidraw2`.
    /// * `device_index` - HID++ device index; `0xFF` for wired devices.
    ///
    /// # Errors
    ///
    /// Returns [`masterror::AppError`] when HID API initialisation fails,
    /// the path contains interior NUL bytes, or the device cannot be opened.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use logi_mx_driver::hidpp::HidppDevice;
    ///
    /// let device = HidppDevice::open_path("/dev/hidraw2", 0xFF)?;
    /// # Ok::<(), masterror::AppError>(())
    /// ```
    pub fn open_path(path: &str, device_index: u8) -> Result<Self> {
        let api = HidApi::new()
            .map_err(|e| AppError::internal("Failed to initialize HID API").with_source(e))?;

        let path_cstr = CString::new(path)
            .map_err(|e| AppError::bad_request("Invalid path").with_source(e))?;

        let device = api
            .open_path(&path_cstr)
            .map_err(|e| AppError::not_found("Failed to open device").with_source(e))?;

        debug!(
            "Opened HID++ device at {} with index {}",
            path, device_index
        );

        Ok(Self {
            device,
            device_index,
            feature_cache: HashMap::new(),
            software_id: 0x05
        })
    }

    /// Locates and opens a HID++ interface by USB vendor/product ID.
    ///
    /// Scans connected HID devices and opens the first match exposing the
    /// HID++ interface (`2`) or an unnumbered interface.
    ///
    /// # Arguments
    ///
    /// * `vendor_id` - USB vendor ID, e.g. `0x046D` for Logitech.
    /// * `product_id` - USB product ID of the receiver or device.
    /// * `device_index` - HID++ device index; `0xFF` for wired devices.
    ///
    /// # Errors
    ///
    /// Returns [`masterror::AppError`] when HID API initialisation fails,
    /// no matching interface is found, or the device cannot be opened.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use logi_mx_driver::hidpp::HidppDevice;
    ///
    /// let device = HidppDevice::open_vid_pid(0x046D, 0xC548, 2)?;
    /// # Ok::<(), masterror::AppError>(())
    /// ```
    pub fn open_vid_pid(vendor_id: u16, product_id: u16, device_index: u8) -> Result<Self> {
        let api = HidApi::new()
            .map_err(|e| AppError::internal("Failed to initialize HID API").with_source(e))?;

        let mut target_path = None;
        for device_info in api.device_list() {
            if device_info.vendor_id() == vendor_id
                && device_info.product_id() == product_id
                && (device_info.interface_number() == 2 || device_info.interface_number() == -1)
            {
                target_path = Some(device_info.path().to_owned());
                debug!(
                    "Found HID++ device at interface {}: {:?}",
                    device_info.interface_number(),
                    device_info.path()
                );
                break;
            }
        }

        let path = target_path
            .ok_or_else(|| AppError::not_found("HID++ interface not found for device"))?;

        let device = api
            .open_path(&path)
            .map_err(|e| AppError::not_found("Failed to open device").with_source(e))?;

        debug!(
            "Opened HID++ device VID:{:04x} PID:{:04x} index:{}",
            vendor_id, product_id, device_index
        );

        Ok(Self {
            device,
            device_index,
            feature_cache: HashMap::new(),
            software_id: 0x05
        })
    }

    /// Sends one HID++ command and returns the device response.
    ///
    /// Chooses a short or long packet based on the parameter count and
    /// retries on transient failures (`ERROR_BUSY`) and transport errors.
    ///
    /// # Arguments
    ///
    /// * `feature_index` - Feature index previously resolved via
    ///   [`get_feature_index`](Self::get_feature_index).
    /// * `function_id` - Function within the feature, low nibble.
    /// * `params` - Up to 16 parameter bytes.
    ///
    /// # Errors
    ///
    /// Returns [`masterror::AppError`] when parameters exceed 16 bytes, the
    /// device reports a HID++ error that persists across retries, or the
    /// transport times out.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use logi_mx_driver::hidpp::{FEATURE_UNIFIED_BATTERY, HidppDevice};
    ///
    /// let mut device = HidppDevice::open_vid_pid(0x046D, 0xC548, 2)?;
    /// let feature = device.get_feature_index(FEATURE_UNIFIED_BATTERY)?;
    /// let response = device.send_command(feature, 0x00, &[])?;
    /// # Ok::<(), masterror::AppError>(())
    /// ```
    pub fn send_command(
        &mut self,
        feature_index: u8,
        function_id: u8,
        params: &[u8]
    ) -> Result<HidppPacket> {
        let packet = if params.len() <= 3 {
            let mut parameters = [0u8; 3];
            parameters[..params.len()].copy_from_slice(params);
            HidppPacket::new_short(
                self.device_index,
                feature_index,
                function_id,
                self.software_id,
                parameters
            )
        } else if params.len() <= 16 {
            let mut parameters = [0u8; 16];
            parameters[..params.len()].copy_from_slice(params);
            HidppPacket::new_long(
                self.device_index,
                feature_index,
                function_id,
                self.software_id,
                parameters
            )
        } else {
            return Err(AppError::bad_request(
                "Parameters too long for HID++ packet"
            ));
        };

        trace!("Sending HID++ packet: {:?}", packet);

        for attempt in 0..RETRY_COUNT {
            match self.send_packet_with_response(&packet) {
                Ok(response) => {
                    if response.is_error()
                        && let Some(error_code) = response.get_error_code()
                    {
                        if error_code == ERROR_BUSY && attempt < RETRY_COUNT - 1 {
                            warn!("Device busy, retrying... (attempt {})", attempt + 1);
                            std::thread::sleep(Duration::from_millis(50));
                            continue;
                        }
                        return Err(Self::map_hidpp_error(error_code));
                    }
                    trace!("Received response: {:?}", response);
                    return Ok(response);
                }
                Err(e) if attempt < RETRY_COUNT - 1 => {
                    warn!(
                        "Command failed, retrying... (attempt {}): {}",
                        attempt + 1,
                        e
                    );
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(e) => return Err(e)
            }
        }

        Err(DeviceErrorKind::CommandFailed.into())
    }

    /// Resolves a feature ID to its runtime feature index.
    ///
    /// Indices are device specific and discovered dynamically via the Root
    /// feature; successful lookups are cached for subsequent calls.
    ///
    /// # Arguments
    ///
    /// * `feature_id` - Static feature ID, e.g. `FEATURE_ADJUSTABLE_DPI`.
    ///
    /// # Errors
    ///
    /// Returns [`masterror::AppError`] when the device does not implement
    /// the feature or the discovery command fails.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use logi_mx_driver::hidpp::{FEATURE_HIRES_WHEEL, HidppDevice};
    ///
    /// let mut device = HidppDevice::open_vid_pid(0x046D, 0xC548, 2)?;
    /// let index = device.get_feature_index(FEATURE_HIRES_WHEEL)?;
    /// # Ok::<(), masterror::AppError>(())
    /// ```
    pub fn get_feature_index(&mut self, feature_id: u16) -> Result<u8> {
        if let Some(&index) = self.feature_cache.get(&feature_id) {
            return Ok(index);
        }

        let params = [(feature_id >> 8) as u8, (feature_id & 0xFF) as u8, 0x00];

        let response = self.send_command(ROOT_INDEX, RootFunction::GetFeature as u8, &params)?;

        let index = match response {
            HidppPacket::Short(p) => p.parameters[0],
            HidppPacket::Long(p) => p.parameters[0]
        };

        if index == 0 {
            return Err(DeviceErrorKind::UnsupportedFeature.into());
        }

        self.feature_cache.insert(feature_id, index);
        debug!("Feature {:04x} mapped to index {}", feature_id, index);

        Ok(index)
    }

    /// Verifies the device answers Root `Ping` requests.
    ///
    /// # Errors
    ///
    /// Returns [`masterror::AppError`] when the device does not respond.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use logi_mx_driver::hidpp::HidppDevice;
    ///
    /// let mut device = HidppDevice::open_vid_pid(0x046D, 0xC548, 2)?;
    /// device.ping()?;
    /// # Ok::<(), masterror::AppError>(())
    /// ```
    pub fn ping(&mut self) -> Result<()> {
        let response = self.send_command(ROOT_INDEX, RootFunction::Ping as u8, &[0, 0, 0])?;
        trace!("Ping response: {:?}", response);
        Ok(())
    }

    fn send_packet_with_response(&self, packet: &HidppPacket) -> Result<HidppPacket> {
        let bytes = packet.to_bytes();
        self.device
            .write(&bytes)
            .map_err(|e| AppError::internal("Failed to write to device").with_source(e))?;

        let mut buf = [0u8; 64];
        let size = self
            .device
            .read_timeout(&mut buf, DEFAULT_TIMEOUT_MS)
            .map_err(|e| AppError::internal("Failed to read from device").with_source(e))?;

        if size == 0 {
            return Err(DeviceErrorKind::Timeout.into());
        }

        HidppPacket::from_bytes(&buf[..size])
    }

    fn map_hidpp_error(error_code: u8) -> AppError {
        match error_code {
            ERROR_INVALID_SUBID => AppError::bad_request("Invalid function ID"),
            ERROR_INVALID_ADDRESS => AppError::bad_request("Invalid address"),
            ERROR_INVALID_VALUE => AppError::bad_request("Invalid value"),
            ERROR_CONNECT_FAIL => AppError::internal("Connection failed"),
            ERROR_TOO_MANY_DEVICES => AppError::conflict("Too many devices"),
            ERROR_ALREADY_EXISTS => AppError::conflict("Already exists"),
            ERROR_BUSY => AppError::internal("Device busy"),
            ERROR_UNKNOWN_DEVICE => AppError::not_found("Unknown device"),
            ERROR_RESOURCE_ERROR => AppError::internal("Resource error"),
            ERROR_REQUEST_UNAVAILABLE => AppError::bad_request("Request unavailable"),
            ERROR_UNSUPPORTED_PARAM => AppError::bad_request("Unsupported parameter"),
            ERROR_WRONG_PIN_CODE => AppError::unauthorized("Wrong PIN code"),
            _ => AppError::internal("Unknown HID++ error")
                .with_field(field::u64("error_code", u64::from(error_code)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_feature_cache() {
        let mut cache = HashMap::new();
        cache.insert(0x1234, 0x05);
        assert_eq!(cache.get(&0x1234), Some(&0x05));
        assert_eq!(cache.get(&0x4321), None);
    }

    #[test]
    fn test_error_mapping() {
        let error_codes = vec![
            ERROR_INVALID_SUBID,
            ERROR_INVALID_ADDRESS,
            ERROR_INVALID_VALUE,
            ERROR_CONNECT_FAIL,
            ERROR_TOO_MANY_DEVICES,
            ERROR_ALREADY_EXISTS,
            ERROR_BUSY,
            ERROR_UNKNOWN_DEVICE,
            ERROR_RESOURCE_ERROR,
            ERROR_REQUEST_UNAVAILABLE,
            ERROR_UNSUPPORTED_PARAM,
            ERROR_WRONG_PIN_CODE,
        ];

        for code in error_codes {
            let err = HidppDevice::map_hidpp_error(code);
            assert!(!err.to_string().is_empty());
        }
    }

    #[test]
    fn test_unknown_error_code() {
        let err = HidppDevice::map_hidpp_error(0xFF);
        assert!(!err.to_string().is_empty());
    }
}
