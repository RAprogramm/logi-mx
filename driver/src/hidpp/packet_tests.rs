// SPDX-FileCopyrightText: 2025 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

#[cfg(test)]
mod packet_tests {
    use super::*;

    #[test]
    fn test_short_packet_creation_all_parameters() {
        let packet = HidppPacket::new_short(0x02, 0x05, 0x03, 0x07, [0xAA, 0xBB, 0xCC]);
        match packet {
            HidppPacket::Short(p) => {
                assert_eq!(p.device_index, 0x02);
                assert_eq!(p.feature_index, 0x05);
                assert_eq!(p.function_id, 0x03);
                assert_eq!(p.software_id, 0x07);
                assert_eq!(p.parameters, [0xAA, 0xBB, 0xCC]);
            }
            _ => panic!("Expected short packet"),
        }
    }

    #[test]
    fn test_long_packet_creation() {
        let params = [0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C,
                      0x0D, 0x0E, 0x0F, 0x10];
        let packet = HidppPacket::new_long(0xFF, 0x00, 0x01, 0x05, params);

        match packet {
            HidppPacket::Long(p) => {
                assert_eq!(p.device_index, 0xFF);
                assert_eq!(p.feature_index, 0x00);
                assert_eq!(p.function_id, 0x01);
                assert_eq!(p.software_id, 0x05);
                assert_eq!(p.parameters, params);
            }
            _ => panic!("Expected long packet"),
        }
    }

    #[test]
    fn test_short_packet_to_bytes_length() {
        let packet = HidppPacket::new_short(0xFF, 0x00, 0x01, 0x05, [0, 0, 0]);
        let bytes = packet.to_bytes();
        assert_eq!(bytes.len(), SHORT_PACKET_SIZE);
    }

    #[test]
    fn test_long_packet_to_bytes_length() {
        let packet = HidppPacket::new_long(0xFF, 0x00, 0x01, 0x05, [0; 16]);
        let bytes = packet.to_bytes();
        assert_eq!(bytes.len(), LONG_PACKET_SIZE);
    }

    #[test]
    fn test_short_packet_report_id() {
        let packet = HidppPacket::new_short(0xFF, 0x00, 0x01, 0x05, [0, 0, 0]);
        let bytes = packet.to_bytes();
        assert_eq!(bytes[0], REPORT_ID_SHORT);
    }

    #[test]
    fn test_long_packet_report_id() {
        let packet = HidppPacket::new_long(0xFF, 0x00, 0x01, 0x05, [0; 16]);
        let bytes = packet.to_bytes();
        assert_eq!(bytes[0], REPORT_ID_LONG);
    }

    #[test]
    fn test_packet_function_software_id_encoding() {
        let packet = HidppPacket::new_short(0xFF, 0x00, 0x0F, 0x0A, [0, 0, 0]);
        let bytes = packet.to_bytes();
        assert_eq!(bytes[3], 0xFA);
    }

    #[test]
    fn test_parse_invalid_empty_packet() {
        let result = HidppPacket::from_bytes(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_invalid_report_id() {
        let bytes = vec![0xFF, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
        let result = HidppPacket::from_bytes(&bytes);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_short_packet_too_small() {
        let bytes = vec![REPORT_ID_SHORT, 0x00, 0x00];
        let result = HidppPacket::from_bytes(&bytes);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_long_packet_too_small() {
        let bytes = vec![REPORT_ID_LONG, 0x00, 0x00, 0x00, 0x00];
        let result = HidppPacket::from_bytes(&bytes);
        assert!(result.is_err());
    }

    #[test]
    fn test_error_detection_0x8f() {
        let error_packet = HidppPacket::new_short(0xFF, 0x8F, 0x01, 0x05, [0x03, 0x00, 0x00]);
        assert!(error_packet.is_error());
        assert_eq!(error_packet.get_error_code(), Some(0x03));
    }

    #[test]
    fn test_error_detection_0xff() {
        let error_packet = HidppPacket::new_short(0xFF, 0xFF, 0x01, 0x05, [0x08, 0x00, 0x00]);
        assert!(error_packet.is_error());
        assert_eq!(error_packet.get_error_code(), Some(0x08));
    }

    #[test]
    fn test_non_error_packet() {
        let normal_packet = HidppPacket::new_short(0xFF, 0x00, 0x01, 0x05, [0x00, 0x00, 0x00]);
        assert!(!normal_packet.is_error());
        assert_eq!(normal_packet.get_error_code(), None);
    }

    #[test]
    fn test_packet_equality() {
        let packet1 = HidppPacket::new_short(0xFF, 0x00, 0x01, 0x05, [0xAA, 0xBB, 0xCC]);
        let packet2 = HidppPacket::new_short(0xFF, 0x00, 0x01, 0x05, [0xAA, 0xBB, 0xCC]);
        assert_eq!(packet1, packet2);
    }

    #[test]
    fn test_packet_inequality() {
        let packet1 = HidppPacket::new_short(0xFF, 0x00, 0x01, 0x05, [0xAA, 0xBB, 0xCC]);
        let packet2 = HidppPacket::new_short(0xFF, 0x00, 0x01, 0x05, [0xAA, 0xBB, 0xDD]);
        assert_ne!(packet1, packet2);
    }

    #[test]
    fn test_long_packet_error_code() {
        let error_packet = HidppPacket::new_long(0xFF, 0xFF, 0x01, 0x05, {
            let mut params = [0; 16];
            params[0] = 0x0A;
            params
        });
        assert!(error_packet.is_error());
        assert_eq!(error_packet.get_error_code(), Some(0x0A));
    }

    #[test]
    fn test_parse_preserves_all_fields() {
        let original = HidppPacket::new_short(0x03, 0x07, 0x0C, 0x09, [0x11, 0x22, 0x33]);
        let bytes = original.to_bytes();
        let parsed = HidppPacket::from_bytes(&bytes).unwrap();
        assert_eq!(original, parsed);
    }

    #[test]
    fn test_long_packet_preserves_all_params() {
        let params = [0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C,
                      0x0D, 0x0E, 0x0F, 0x10];
        let packet = HidppPacket::new_long(0x02, 0x05, 0x03, 0x07, params);
        let bytes = packet.to_bytes();
        let parsed = HidppPacket::from_bytes(&bytes).unwrap();

        match parsed {
            HidppPacket::Long(p) => {
                assert_eq!(p.parameters, params);
            }
            _ => panic!("Expected long packet"),
        }
    }

    #[test]
    fn test_max_function_id() {
        let packet = HidppPacket::new_short(0xFF, 0x00, 0x0F, 0x00, [0, 0, 0]);
        let bytes = packet.to_bytes();
        let parsed = HidppPacket::from_bytes(&bytes).unwrap();

        match parsed {
            HidppPacket::Short(p) => {
                assert_eq!(p.function_id, 0x0F);
            }
            _ => panic!("Expected short packet"),
        }
    }

    #[test]
    fn test_max_software_id() {
        let packet = HidppPacket::new_short(0xFF, 0x00, 0x00, 0x0F, [0, 0, 0]);
        let bytes = packet.to_bytes();
        let parsed = HidppPacket::from_bytes(&bytes).unwrap();

        match parsed {
            HidppPacket::Short(p) => {
                assert_eq!(p.software_id, 0x0F);
            }
            _ => panic!("Expected short packet"),
        }
    }
}
