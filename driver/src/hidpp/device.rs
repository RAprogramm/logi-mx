// SPDX-FileCopyrightText: 2025 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

use std::{
    collections::HashMap,
    ffi::CString,
    time::{Duration, Instant}
};

use hidapi::{HidApi, HidDevice};
use masterror::{field, prelude::*};
use tracing::{debug, trace, warn};

use super::{
    RootFunction,
    constants::{ERROR_BUSY, ERROR_HW_ERROR, ROOT_INDEX},
    packet::HidppPacket
};
use crate::error::{DeviceErrorKind, Result};

/// Read timeout applied to each individual HID read.
const DEFAULT_TIMEOUT_MS: i32 = 1000;
/// Total response window per attempt, covering stale and notification
/// reports that must be skipped before the matching response arrives.
const RESPONSE_BUDGET_MS: u64 = 1500;
/// Number of retries for transport failures and transient HID++ errors.
const RETRY_COUNT: usize = 3;
/// Highest usable software ID; identifiers rotate in `1..=SOFTWARE_ID_MAX`.
const SOFTWARE_ID_MAX: u8 = 0x0F;

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
/// let root = device.feature_index(FEATURE_ROOT)?;
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
        if !path.starts_with("/dev/hidraw") {
            return Err(AppError::bad_request(
                "Invalid HID path: must start with /dev/hidraw"
            ));
        }
        if path.contains("..") {
            return Err(AppError::bad_request(
                "Invalid HID path: must not contain .."
            ));
        }

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

    /// Sends one HID++ command and returns the matching device response.
    ///
    /// Every attempt uses a fresh software ID (rotating `1-15`) so stale
    /// responses from earlier commands can never be mistaken for the current
    /// one. While waiting, unrelated reports — notifications (`software_id`
    /// zero) and responses from other commands — are skipped until the
    /// response matching the request (or its error report) arrives.
    ///
    /// Transient failures (`ERROR_BUSY`, `ERROR_HW_ERROR`) and transport
    /// timeouts are retried.
    ///
    /// # Arguments
    ///
    /// * `feature_index` - Feature index previously resolved via
    ///   [`feature_index`](Self::feature_index).
    /// * `function_id` - Function within the feature.
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
    /// let feature = device.feature_index(FEATURE_UNIFIED_BATTERY)?;
    /// let response = device.send_command(feature, 0x01, &[])?;
    /// # Ok::<(), masterror::AppError>(())
    /// ```
    pub fn send_command(
        &mut self,
        feature_index: u8,
        function_id: u8,
        params: &[u8]
    ) -> Result<HidppPacket> {
        for attempt in 0..RETRY_COUNT {
            let software_id = self.next_software_id();
            let packet = Self::build_packet(
                self.device_index,
                feature_index,
                function_id,
                software_id,
                params
            )?;
            let request_bytes = packet.to_bytes();

            trace!("Sending HID++ packet: {packet:?}");

            self.send_raw(&request_bytes)?;

            match self.await_response(&packet, Duration::from_millis(RESPONSE_BUDGET_MS)) {
                Ok(response) => {
                    if response.is_error() {
                        let Some(error_code) = response.error_code() else {
                            return Err(DeviceErrorKind::InvalidResponse.into());
                        };
                        let transient = error_code == ERROR_BUSY || error_code == ERROR_HW_ERROR;
                        if transient && attempt < RETRY_COUNT - 1 {
                            warn!(
                                "Transient HID++ error {error_code:#04x}, retrying (attempt {})",
                                attempt + 1
                            );
                            std::thread::sleep(Duration::from_millis(50));
                            continue;
                        }
                        return Err(Self::map_hidpp_error(error_code));
                    }
                    trace!("Received response: {response:?}");
                    return Ok(response);
                }
                Err(e) if attempt < RETRY_COUNT - 1 => {
                    warn!("Command failed, retrying (attempt {}): {e}", attempt + 1);
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(e) => return Err(e)
            }
        }

        Err(DeviceErrorKind::CommandFailed.into())
    }

    /// Builds the request packet for a command.
    ///
    /// Short packets carry up to three parameters, long packets up to
    /// sixteen; larger parameter lists are rejected.
    fn build_packet(
        device_index: u8,
        feature_index: u8,
        function_id: u8,
        software_id: u8,
        params: &[u8]
    ) -> Result<HidppPacket> {
        if params.len() <= 3 {
            let mut parameters = [0u8; 3];
            parameters[..params.len()].copy_from_slice(params);
            Ok(HidppPacket::new_short(
                device_index,
                feature_index,
                function_id,
                software_id,
                parameters
            ))
        } else if params.len() <= 16 {
            let mut parameters = [0u8; 16];
            parameters[..params.len()].copy_from_slice(params);
            Ok(HidppPacket::new_long(
                device_index,
                feature_index,
                function_id,
                software_id,
                parameters
            ))
        } else {
            Err(AppError::bad_request(
                "Parameters too long for HID++ packet"
            ))
        }
    }

    /// Returns the next rotating software ID in `1..=15`.
    ///
    /// Distinct IDs per request prevent stale responses from earlier
    /// commands being confused with the current exchange.
    const fn next_software_id(&mut self) -> u8 {
        self.software_id = if self.software_id == 0 || self.software_id >= SOFTWARE_ID_MAX {
            0x01
        } else {
            self.software_id + 1
        };
        self.software_id
    }

    /// Reads reports until one answers `request` or the budget expires.
    ///
    /// Notifications (`software_id` zero), reports from other commands and
    /// unparseable bytes are skipped.
    fn await_response(&self, request: &HidppPacket, budget: Duration) -> Result<HidppPacket> {
        let start = Instant::now();

        while start.elapsed() < budget {
            let remaining_ms = i32::try_from(budget.saturating_sub(start.elapsed()).as_millis())
                .unwrap_or(DEFAULT_TIMEOUT_MS)
                .clamp(1, DEFAULT_TIMEOUT_MS);

            if let Some(bytes) = self.read_raw(remaining_ms)
                && let Ok(response) = HidppPacket::from_bytes(&bytes)
            {
                if response.matches_request(request) {
                    return Ok(response);
                }
                trace!("Skipping unrelated report: {response:?}");
            }
        }

        Err(DeviceErrorKind::Timeout.into())
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
    /// let index = device.feature_index(FEATURE_HIRES_WHEEL)?;
    /// # Ok::<(), masterror::AppError>(())
    /// ```
    pub fn feature_index(&mut self, feature_id: u16) -> Result<u8> {
        if let Some(&index) = self.feature_cache.get(&feature_id) {
            return Ok(index);
        }

        let params = [(feature_id >> 8) as u8, (feature_id & 0xFF) as u8, 0x00];

        let response = self.send_command(ROOT_INDEX, RootFunction::GetFeature as u8, &params)?;

        let index = response.parameters().first().copied().unwrap_or(0);

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

    /// Writes a raw HID++ report to the device without validation.
    ///
    /// Intended for diagnostics and protocol extensions; use
    /// [`send_command`](Self::send_command) for normal traffic.
    ///
    /// # Arguments
    ///
    /// * `bytes` - Complete on-wire report including the report ID byte.
    ///
    /// # Errors
    ///
    /// Returns [`masterror::AppError`] when the write fails.
    pub fn send_raw(&self, bytes: &[u8]) -> Result<()> {
        self.device
            .write(bytes)
            .map_err(|e| AppError::internal("Failed to write to device").with_source(e))?;
        Ok(())
    }

    /// Reads one raw HID++ report with a timeout.
    ///
    /// Returns the raw bytes as received, without parsing.
    ///
    /// # Arguments
    ///
    /// * `timeout_ms` - Milliseconds to wait for a report.
    ///
    /// # Returns
    ///
    /// `Some(bytes)` when a report arrived in time, `None` on timeout.
    #[must_use]
    pub fn read_raw(&self, timeout_ms: i32) -> Option<Vec<u8>> {
        let mut buf = [0u8; 64];
        match self.device.read_timeout(&mut buf, timeout_ms) {
            Ok(size) if size > 0 => Some(buf[..size].to_vec()),
            _ => None
        }
    }

    fn map_hidpp_error(error_code: u8) -> AppError {
        match error_code {
            super::constants::ERROR_UNKNOWN => AppError::internal("Unknown HID++ error"),
            super::constants::ERROR_INVALID_ARGUMENT => AppError::bad_request("Invalid argument"),
            super::constants::ERROR_OUT_OF_RANGE => AppError::bad_request("Value out of range"),
            ERROR_HW_ERROR => AppError::internal("Hardware error"),
            super::constants::ERROR_LOGITECH_INTERNAL => {
                AppError::internal("Logitech internal error")
            }
            super::constants::ERROR_INVALID_FEATURE_INDEX => {
                AppError::bad_request("Invalid feature index")
            }
            super::constants::ERROR_INVALID_FUNCTION_ID => {
                AppError::bad_request("Invalid function ID")
            }
            ERROR_BUSY => AppError::internal("Device busy"),
            super::constants::ERROR_UNSUPPORTED => {
                AppError::bad_request("Feature or function not supported")
            }
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
    fn test_software_id_rotation() {
        let mut software_id = 0u8;

        let next = |software_id: &mut u8| {
            *software_id = if *software_id == 0 || *software_id >= SOFTWARE_ID_MAX {
                0x01
            } else {
                *software_id + 1
            };
            *software_id
        };

        let ids: Vec<u8> = (0..20).map(|_| next(&mut software_id)).collect();

        assert_eq!(ids[0], 0x01);
        assert_eq!(ids[14], 0x0F);
        assert_eq!(ids[15], 0x01);
    }

    #[test]
    fn test_build_packet_short() {
        let packet =
            HidppDevice::build_packet(0x02, 0x09, 0x03, 0x05, &[0xAA, 0xBB, 0xCC]).unwrap();

        match packet {
            HidppPacket::Short(p) => {
                assert_eq!(p.device_index, 0x02);
                assert_eq!(p.feature_index, 0x09);
                assert_eq!(p.function_id, 0x03);
                assert_eq!(p.software_id, 0x05);
                assert_eq!(p.parameters, [0xAA, 0xBB, 0xCC]);
            }
            _ => panic!("Expected short packet")
        }
    }

    #[test]
    fn test_build_packet_long() {
        let params: Vec<u8> = (0..16).collect();
        let packet = HidppDevice::build_packet(0x02, 0x09, 0x03, 0x05, &params).unwrap();

        match packet {
            HidppPacket::Long(p) => assert_eq!(p.parameters, params.as_slice()),
            _ => panic!("Expected long packet")
        }
    }

    #[test]
    fn test_build_packet_too_long() {
        let params: Vec<u8> = (0..17).collect();
        let result = HidppDevice::build_packet(0x02, 0x09, 0x03, 0x05, &params);
        assert!(result.is_err());
    }

    #[test]
    fn test_error_mapping() {
        let error_codes = [
            super::super::constants::ERROR_UNKNOWN,
            super::super::constants::ERROR_INVALID_ARGUMENT,
            super::super::constants::ERROR_OUT_OF_RANGE,
            ERROR_HW_ERROR,
            super::super::constants::ERROR_LOGITECH_INTERNAL,
            super::super::constants::ERROR_INVALID_FEATURE_INDEX,
            super::super::constants::ERROR_INVALID_FUNCTION_ID,
            ERROR_BUSY,
            super::super::constants::ERROR_UNSUPPORTED
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
