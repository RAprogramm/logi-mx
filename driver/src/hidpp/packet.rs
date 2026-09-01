// SPDX-FileCopyrightText: 2025 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! HID++ report packet model.
//!
//! Covers the two report sizes used by HID++ 2.0: short (7 bytes) and long
//! (20 bytes). Provides construction, wire encoding and parsing, plus error
//! packet detection.

use super::constants::{LONG_PACKET_SIZE, REPORT_ID_LONG, REPORT_ID_SHORT, SHORT_PACKET_SIZE};
use crate::error::{DeviceErrorKind, Result};

/// A HID++ request or response packet.
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
    Long(LongPacket)
}

/// Payload of a short HID++ report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShortPacket {
    /// Target device index; `0xFF` for wired devices.
    pub device_index:  u8,
    /// Resolved feature index; error reports use `0x8F`/`0xFF`.
    pub feature_index: u8,
    /// Function within the feature, stored in the high nibble on wire.
    pub function_id:   u8,
    /// Caller identity to discriminate responses on the bus.
    pub software_id:   u8,
    /// Three parameter bytes.
    pub parameters:    [u8; 3]
}

/// Payload of a long HID++ report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LongPacket {
    /// Target device index; `0xFF` for wired devices.
    pub device_index:  u8,
    /// Resolved feature index; error reports use `0x8F`/`0xFF`.
    pub feature_index: u8,
    /// Function within the feature, stored in the high nibble on wire.
    pub function_id:   u8,
    /// Caller identity to discriminate responses on the bus.
    pub software_id:   u8,
    /// Sixteen parameter bytes.
    pub parameters:    [u8; 16]
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
        }
    }

    /// Parses a raw report read from the device.
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

    /// Reports whether the packet is a HID++ error response.
    ///
    /// Error responses carry `0x8F` or `0xFF` in the feature index position.
    ///
    /// # Examples
    ///
    /// ```
    /// use logi_mx_driver::hidpp::HidppPacket;
    ///
    /// let error = HidppPacket::new_short(0xFF, 0x8F, 0x01, 0x05, [0x02, 0, 0]);
    /// assert!(error.is_error());
    /// ```
    #[must_use]
    pub const fn is_error(&self) -> bool {
        match self {
            Self::Short(p) => p.feature_index == 0x8F || p.feature_index == 0xFF,
            Self::Long(p) => p.feature_index == 0x8F || p.feature_index == 0xFF
        }
    }

    /// Extracts the HID++ error code from an error response.
    ///
    /// # Examples
    ///
    /// ```
    /// use logi_mx_driver::hidpp::HidppPacket;
    ///
    /// let error = HidppPacket::new_short(0xFF, 0x8F, 0x01, 0x05, [0x07, 0, 0]);
    /// assert_eq!(error.get_error_code(), Some(0x07));
    /// ```
    #[must_use]
    pub const fn get_error_code(&self) -> Option<u8> {
        if !self.is_error() {
            return None;
        }
        match self {
            Self::Short(p) => Some(p.parameters[0]),
            Self::Long(p) => Some(p.parameters[0])
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
            HidppPacket::Long(_) => panic!("Expected short packet")
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
        let error_packet = HidppPacket::new_short(0xFF, 0x8F, 0x01, 0x05, [0x02, 0x00, 0x00]);
        assert!(error_packet.is_error());
        assert_eq!(error_packet.get_error_code(), Some(0x02));
    }
}

#[cfg(test)]
#[path = "packet_tests.rs"]
mod packet_tests;
