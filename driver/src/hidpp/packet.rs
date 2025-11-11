// SPDX-FileCopyrightText: 2025 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

use super::constants::*;
use crate::error::{DeviceErrorKind, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HidppPacket {
    Short(ShortPacket),
    Long(LongPacket)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShortPacket {
    pub device_index:  u8,
    pub feature_index: u8,
    pub function_id:   u8,
    pub software_id:   u8,
    pub parameters:    [u8; 3]
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LongPacket {
    pub device_index:  u8,
    pub feature_index: u8,
    pub function_id:   u8,
    pub software_id:   u8,
    pub parameters:    [u8; 16]
}

impl HidppPacket {
    pub fn new_short(
        device_index: u8,
        feature_index: u8,
        function_id: u8,
        software_id: u8,
        parameters: [u8; 3]
    ) -> Self {
        HidppPacket::Short(ShortPacket {
            device_index,
            feature_index,
            function_id,
            software_id,
            parameters
        })
    }

    pub fn new_long(
        device_index: u8,
        feature_index: u8,
        function_id: u8,
        software_id: u8,
        parameters: [u8; 16]
    ) -> Self {
        HidppPacket::Long(LongPacket {
            device_index,
            feature_index,
            function_id,
            software_id,
            parameters
        })
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        match self {
            HidppPacket::Short(packet) => {
                let mut bytes = Vec::with_capacity(SHORT_PACKET_SIZE);
                bytes.push(REPORT_ID_SHORT);
                bytes.push(packet.device_index);
                bytes.push(packet.feature_index);
                bytes.push((packet.function_id << 4) | (packet.software_id & 0x0F));
                bytes.extend_from_slice(&packet.parameters);
                bytes
            }
            HidppPacket::Long(packet) => {
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

                Ok(HidppPacket::Short(ShortPacket {
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

                Ok(HidppPacket::Long(LongPacket {
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

    pub fn is_error(&self) -> bool {
        match self {
            HidppPacket::Short(p) => p.feature_index == 0x8F || p.feature_index == 0xFF,
            HidppPacket::Long(p) => p.feature_index == 0x8F || p.feature_index == 0xFF
        }
    }

    pub fn get_error_code(&self) -> Option<u8> {
        if !self.is_error() {
            return None;
        }
        match self {
            HidppPacket::Short(p) => Some(p.parameters[0]),
            HidppPacket::Long(p) => Some(p.parameters[0])
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
            _ => panic!("Expected short packet")
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
