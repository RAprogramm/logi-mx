// SPDX-FileCopyrightText: 2025 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! HID++ report packet model.
//!
//! Covers the two report sizes used by HID++ 2.0: short (7 bytes) and long
//! (20 bytes), plus the dedicated error report layout. Provides
//! construction, wire encoding and parsing, request/response matching and
//! error packet detection.

use super::constants::{LONG_PACKET_SIZE, REPORT_ID_LONG, REPORT_ID_SHORT, SHORT_PACKET_SIZE};
use crate::error::{DeviceErrorKind, Result};

/// Wire bytes that mark an error report in the feature index position.
const ERROR_MARKER_SHORT: u8 = 0x8F;
const ERROR_MARKER_LONG: u8 = 0xFF;

/// A HID++ request, response, notification or error report.
///
/// # Examples
///
/// ```
/// use logi_mx_driver::hidpp::HidppPacket;
///
/// let packet = HidppPacket::new_short(0xFF, 0x00, 0x01, 0x05, [0, 0, 0]);
/// assert_eq!(packet.to_bytes().len(), 7);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HidppPacket {
    /// 7-byte report with 3 parameter bytes.
    Short(ShortPacket),
    /// 20-byte report with 16 parameter bytes.
    Long(LongPacket),
    /// Error report raised by the device for a failed request.
    Error(ErrorPacket)
}

/// Payload of a short HID++ report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShortPacket {
    /// Target device index; `0xFF` for wired devices.
    pub device_index:  u8,
    /// Resolved feature index.
    pub feature_index: u8,
    /// Function within the feature.
    pub function_id:   u8,
    /// Caller identity used to match responses; `0` marks notifications.
    pub software_id:   u8,
    /// Three parameter bytes.
    pub parameters:    [u8; 3]
}

/// Payload of a long HID++ report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LongPacket {
    /// Target device index; `0xFF` for wired devices.
    pub device_index:  u8,
    /// Resolved feature index.
    pub feature_index: u8,
    /// Function within the feature.
    pub function_id:   u8,
    /// Caller identity used to match responses; `0` marks notifications.
    pub software_id:   u8,
    /// Sixteen parameter bytes.
    pub parameters:    [u8; 16]
}

/// Error report layout.
///
/// Wire format: `[reportId, deviceIndex, 0x8F|0xFF, featureIndex,
/// functionId<<4|softwareId, errorCode, ...]`. The feature index refers to
/// the failed request, and the error code describes the failure reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrorPacket {
    /// Device the request was addressed to.
    pub device_index:  u8,
    /// Feature index of the request that failed.
    pub feature_index: u8,
    /// Function of the request that failed.
    pub function_id:   u8,
    /// Caller identity of the request that failed.
    pub software_id:   u8,
    /// HID++ 2.0 error code.
    pub error_code:    u8
}

impl HidppPacket {
    /// Builds a short packet.
    ///
    /// # Arguments
    ///
    /// * `device_index` - Target device index.
    /// * `feature_index` - Resolved feature index.
    /// * `function_id` - Function within the feature.
    /// * `software_id` - Caller identity.
    /// * `parameters` - Three parameter bytes.
    ///
    /// # Examples
    ///
    /// ```
    /// use logi_mx_driver::hidpp::HidppPacket;
    ///
    /// let packet = HidppPacket::new_short(0x02, 0x00, 0x01, 0x05, [0; 3]);
    /// assert!(matches!(packet, HidppPacket::Short(_)));
    /// ```
    #[must_use]
    pub const fn new_short(
        device_index: u8,
        feature_index: u8,
        function_id: u8,
        software_id: u8,
        parameters: [u8; 3]
    ) -> Self {
        Self::Short(ShortPacket {
            device_index,
            feature_index,
            function_id,
            software_id,
            parameters
        })
    }

    /// Builds a long packet.
    ///
    /// # Arguments
    ///
    /// * `device_index` - Target device index.
    /// * `feature_index` - Resolved feature index.
    /// * `function_id` - Function within the feature.
    /// * `software_id` - Caller identity.
    /// * `parameters` - Sixteen parameter bytes.
    ///
    /// # Examples
    ///
    /// ```
    /// use logi_mx_driver::hidpp::HidppPacket;
    ///
    /// let packet = HidppPacket::new_long(0x02, 0x00, 0x01, 0x05, [0; 16]);
    /// assert!(matches!(packet, HidppPacket::Long(_)));
    /// ```
    #[must_use]
    pub const fn new_long(
        device_index: u8,
        feature_index: u8,
        function_id: u8,
        software_id: u8,
        parameters: [u8; 16]
    ) -> Self {
        Self::Long(LongPacket {
            device_index,
            feature_index,
            function_id,
            software_id,
            parameters
        })
    }

    /// Encodes the packet to the on-wire report layout.
    ///
    /// # Examples
    ///
    /// ```
    /// use logi_mx_driver::hidpp::HidppPacket;
    ///
    /// let packet = HidppPacket::new_short(0xFF, 0x00, 0x01, 0x05, [0x11, 0x22, 0x33]);
    /// let bytes = packet.to_bytes();
    /// assert_eq!(bytes[0], 0x10);
    /// assert_eq!(bytes[3], 0x15);
    /// ```
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        match self {
            Self::Short(packet) => {
                let mut bytes = Vec::with_capacity(SHORT_PACKET_SIZE);
                bytes.push(REPORT_ID_SHORT);
                bytes.push(packet.device_index);
                bytes.push(packet.feature_index);
                bytes.push((packet.function_id << 4) | (packet.software_id & 0x0F));
                bytes.extend_from_slice(&packet.parameters);
                bytes
            }
            Self::Long(packet) => {
                let mut bytes = Vec::with_capacity(LONG_PACKET_SIZE);
                bytes.push(REPORT_ID_LONG);
                bytes.push(packet.device_index);
                bytes.push(packet.feature_index);
                bytes.push((packet.function_id << 4) | (packet.software_id & 0x0F));
                bytes.extend_from_slice(&packet.parameters);
                bytes
            }
            Self::Error(packet) => {
                let mut bytes = Vec::with_capacity(SHORT_PACKET_SIZE);
                bytes.push(REPORT_ID_SHORT);
                bytes.push(packet.device_index);
                bytes.push(ERROR_MARKER_SHORT);
                bytes.push(packet.feature_index);
                bytes.push((packet.function_id << 4) | (packet.software_id & 0x0F));
                bytes.push(packet.error_code);
                bytes.push(0x00);
                bytes
            }
        }
    }

    /// Parses a raw report read from the device.
    ///
    /// Reports carrying `0x8F` (short) or `0xFF` (long) in the feature index
    /// position are decoded as [`HidppPacket::Error`] with the HID++ 2.0
    /// error layout: byte 3 holds the feature index of the failed request,
    /// byte 4 its function and software ID, byte 5 the error code.
    ///
    /// # Arguments
    ///
    /// * `bytes` - Raw bytes as returned by the HID read; must start with a
    ///   known report ID and carry at least the report size.
    ///
    /// # Errors
    ///
    /// Returns [`masterror::AppError`] with
    /// [`DeviceErrorKind::InvalidResponse`] when the buffer is empty, the
    /// report ID is unknown, or the buffer is shorter than the report size.
    ///
    /// # Examples
    ///
    /// ```
    /// use logi_mx_driver::hidpp::HidppPacket;
    ///
    /// let packet = HidppPacket::new_short(0xFF, 0x00, 0x01, 0x05, [0x11, 0x22, 0x33]);
    /// let parsed = HidppPacket::from_bytes(&packet.to_bytes())?;
    /// assert_eq!(packet, parsed);
    /// # Ok::<(), masterror::AppError>(())
    /// ```
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.is_empty() {
            return Err(DeviceErrorKind::InvalidResponse.into());
        }

        match bytes[0] {
            REPORT_ID_SHORT => {
                if bytes.len() < SHORT_PACKET_SIZE {
                    return Err(DeviceErrorKind::InvalidResponse.into());
                }
                if bytes[2] == ERROR_MARKER_SHORT {
                    return Ok(Self::parse_error(bytes));
                }
                let mut parameters = [0u8; 3];
                parameters.copy_from_slice(&bytes[4..7]);

                Ok(Self::Short(ShortPacket {
                    device_index: bytes[1],
                    feature_index: bytes[2],
                    function_id: bytes[3] >> 4,
                    software_id: bytes[3] & 0x0F,
                    parameters
                }))
            }
            REPORT_ID_LONG => {
                if bytes.len() < LONG_PACKET_SIZE {
                    return Err(DeviceErrorKind::InvalidResponse.into());
                }
                if bytes[2] == ERROR_MARKER_LONG {
                    return Ok(Self::parse_error(bytes));
                }
                let mut parameters = [0u8; 16];
                parameters.copy_from_slice(&bytes[4..20]);

                Ok(Self::Long(LongPacket {
                    device_index: bytes[1],
                    feature_index: bytes[2],
                    function_id: bytes[3] >> 4,
                    software_id: bytes[3] & 0x0F,
                    parameters
                }))
            }
            _ => Err(DeviceErrorKind::InvalidResponse.into())
        }
    }

    /// Decodes the error report layout shared by short and long reports.
    #[must_use]
    const fn parse_error(bytes: &[u8]) -> Self {
        Self::Error(ErrorPacket {
            device_index:  bytes[1],
            feature_index: bytes[3],
            function_id:   bytes[4] >> 4,
            software_id:   bytes[4] & 0x0F,
            error_code:    bytes[5]
        })
    }

    /// Extracts the parameter bytes of a non-error packet.
    ///
    /// Short reports yield three bytes, long reports sixteen; error reports
    /// carry no parameters and yield an empty slice.
    ///
    /// # Examples
    ///
    /// ```
    /// use logi_mx_driver::hidpp::HidppPacket;
    ///
    /// let packet = HidppPacket::new_short(0xFF, 0x00, 0x01, 0x05, [0x11, 0x22, 0x33]);
    /// assert_eq!(packet.parameters(), &[0x11, 0x22, 0x33]);
    /// ```
    #[must_use]
    pub const fn parameters(&self) -> &[u8] {
        match self {
            Self::Short(p) => &p.parameters,
            Self::Long(p) => &p.parameters,
            Self::Error(_) => &[]
        }
    }

    /// Reports whether the packet is a HID++ error response.
    ///
    /// # Examples
    ///
    /// ```
    /// use logi_mx_driver::hidpp::HidppPacket;
    ///
    /// let error = HidppPacket::from_bytes(&[0x10, 0x02, 0x8F, 0x09, 0x15, 0x02, 0x00])?;
    /// assert!(error.is_error());
    /// # Ok::<(), masterror::AppError>(())
    /// ```
    #[must_use]
    pub const fn is_error(&self) -> bool {
        matches!(self, Self::Error(_))
    }

    /// Extracts the HID++ error code from an error response.
    ///
    /// # Examples
    ///
    /// ```
    /// use logi_mx_driver::hidpp::HidppPacket;
    ///
    /// let error = HidppPacket::from_bytes(&[0x10, 0x02, 0x8F, 0x09, 0x15, 0x02, 0x00])?;
    /// assert_eq!(error.error_code(), Some(0x02));
    /// # Ok::<(), masterror::AppError>(())
    /// ```
    #[must_use]
    pub const fn error_code(&self) -> Option<u8> {
        match self {
            Self::Error(p) => Some(p.error_code),
            _ => None
        }
    }

    /// Reports whether this packet answers the given request.
    ///
    /// A response matches when device index, feature index, function ID and
    /// software ID all agree with the request. Notifications (`software_id`
    /// zero) and reports from other features never match.
    ///
    /// # Examples
    ///
    /// ```
    /// use logi_mx_driver::hidpp::HidppPacket;
    ///
    /// let request = HidppPacket::new_short(0x02, 0x05, 0x03, 0x07, [0; 3]);
    /// let response = HidppPacket::new_short(0x02, 0x05, 0x03, 0x07, [0; 3]);
    /// assert!(response.matches_request(&request));
    ///
    /// let notification = HidppPacket::new_short(0x02, 0x05, 0x03, 0x00, [0; 3]);
    /// assert!(!notification.matches_request(&request));
    /// ```
    #[must_use]
    pub const fn matches_request(&self, request: &Self) -> bool {
        match request {
            Self::Error(_) => false,
            Self::Short(r) => self.matches_fields(
                r.device_index,
                r.feature_index,
                r.function_id,
                r.software_id
            ),
            Self::Long(r) => self.matches_fields(
                r.device_index,
                r.feature_index,
                r.function_id,
                r.software_id
            )
        }
    }

    #[must_use]
    const fn matches_fields(
        &self,
        device_index: u8,
        feature_index: u8,
        function_id: u8,
        software_id: u8
    ) -> bool {
        match self {
            Self::Short(p) => {
                p.device_index == device_index
                    && p.feature_index == feature_index
                    && p.function_id == function_id
                    && p.software_id == software_id
            }
            Self::Long(p) => {
                p.device_index == device_index
                    && p.feature_index == feature_index
                    && p.function_id == function_id
                    && p.software_id == software_id
            }
            Self::Error(p) => {
                p.device_index == device_index
                    && p.feature_index == feature_index
                    && p.function_id == function_id
                    && p.software_id == software_id
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_short_packet_creation() {
        let packet = HidppPacket::new_short(0xFF, 0x00, 0x01, 0x05, [0x11, 0x22, 0x33]);
        let bytes = packet.to_bytes();

        assert_eq!(bytes[0], REPORT_ID_SHORT);
        assert_eq!(bytes[1], 0xFF);
        assert_eq!(bytes[2], 0x00);
        assert_eq!(bytes[3], 0x15);
        assert_eq!(bytes[4], 0x11);
        assert_eq!(bytes[5], 0x22);
        assert_eq!(bytes[6], 0x33);
    }

    #[test]
    fn test_short_packet_parsing() {
        let bytes = vec![0x10, 0xFF, 0x00, 0x15, 0x11, 0x22, 0x33];
        let packet = HidppPacket::from_bytes(&bytes).unwrap();

        match packet {
            HidppPacket::Short(p) => {
                assert_eq!(p.device_index, 0xFF);
                assert_eq!(p.feature_index, 0x00);
                assert_eq!(p.function_id, 0x01);
                assert_eq!(p.software_id, 0x05);
                assert_eq!(p.parameters, [0x11, 0x22, 0x33]);
            }
            HidppPacket::Long(_) | HidppPacket::Error(_) => panic!("Expected short packet")
        }
    }

    #[test]
    fn test_long_packet_roundtrip() {
        let params = [
            0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x0E,
            0x0F, 0x10
        ];
        let packet = HidppPacket::new_long(0x02, 0x05, 0x03, 0x07, params);
        let bytes = packet.to_bytes();
        let parsed = HidppPacket::from_bytes(&bytes).unwrap();

        assert_eq!(packet, parsed);
    }

    #[test]
    fn test_error_detection() {
        let bytes = vec![0x10, 0x02, 0x8F, 0x09, 0x15, 0x02, 0x00];
        let error_packet = HidppPacket::from_bytes(&bytes).unwrap();

        assert!(error_packet.is_error());
        assert_eq!(error_packet.error_code(), Some(0x02));
    }

    #[test]
    fn test_error_report_layout() {
        let bytes = vec![
            0x11, 0x02, 0xFF, 0x09, 0x15, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ];
        let packet = HidppPacket::from_bytes(&bytes).unwrap();

        match packet {
            HidppPacket::Error(p) => {
                assert_eq!(p.device_index, 0x02);
                assert_eq!(p.feature_index, 0x09);
                assert_eq!(p.function_id, 0x01);
                assert_eq!(p.software_id, 0x05);
                assert_eq!(p.error_code, 0x02);
            }
            _ => panic!("Expected error packet")
        }
    }

    #[test]
    fn test_error_report_matches_request() {
        let request = HidppPacket::new_short(0x02, 0x09, 0x01, 0x05, [0; 3]);
        let bytes = vec![0x10, 0x02, 0x8F, 0x09, 0x15, 0x02, 0x00];
        let error = HidppPacket::from_bytes(&bytes).unwrap();

        assert!(error.is_error());
        assert!(error.matches_request(&request));
    }

    #[test]
    fn test_notification_does_not_match() {
        let request = HidppPacket::new_short(0x02, 0x05, 0x03, 0x07, [0; 3]);
        let notification = HidppPacket::new_short(0x02, 0x05, 0x03, 0x00, [0; 3]);
        assert!(!notification.matches_request(&request));
    }

    #[test]
    fn test_mismatched_response_does_not_match() {
        let request = HidppPacket::new_short(0x02, 0x05, 0x03, 0x07, [0; 3]);
        let other = HidppPacket::new_short(0x02, 0x06, 0x03, 0x07, [0; 3]);
        assert!(!other.matches_request(&request));
    }

    #[test]
    fn test_parameters_helper() {
        let short = HidppPacket::new_short(0xFF, 0x00, 0x01, 0x05, [1, 2, 3]);
        assert_eq!(short.parameters(), &[1, 2, 3]);

        let long = HidppPacket::new_long(0xFF, 0x00, 0x01, 0x05, [7; 16]);
        assert_eq!(long.parameters().len(), 16);

        let error = HidppPacket::from_bytes(&[0x10, 0x02, 0x8F, 0x09, 0x15, 0x02, 0x00]).unwrap();
        assert!(error.parameters().is_empty());
    }
}

#[cfg(test)]
#[path = "packet_tests.rs"]
mod packet_tests;

#[cfg(test)]
#[path = "packet_proptests.rs"]
mod packet_proptests;
