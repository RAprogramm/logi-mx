// SPDX-FileCopyrightText: 2025 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Property-based tests for the HID++ packet codec.

use proptest::prelude::*;

use super::*;

proptest! {
    #[test]
    fn prop_short_packet_roundtrip(
        device_index in any::<u8>(),
        feature_index in any::<u8>().prop_filter(
            "feature_index must not collide with error markers 0x8F/0xFF",
            |v| *v != 0x8F && *v != 0xFF
        ),
        function_id in 0u8..0x10,
        software_id in 0u8..0x10,
        parameters in proptest::array::uniform3(any::<u8>())
    ) {
        let packet = HidppPacket::new_short(
            device_index,
            feature_index,
            function_id,
            software_id,
            parameters
        );
        let parsed = HidppPacket::from_bytes(&packet.to_bytes())?;
        prop_assert_eq!(parsed, packet);
    }

    #[test]
    fn prop_long_packet_roundtrip(
        device_index in any::<u8>(),
        feature_index in any::<u8>().prop_filter(
            "feature_index must not collide with error markers 0x8F/0xFF",
            |v| *v != 0x8F && *v != 0xFF
        ),
        function_id in 0u8..0x10,
        software_id in 0u8..0x10,
        parameters in proptest::array::uniform16(any::<u8>())
    ) {
        let packet = HidppPacket::new_long(
            device_index,
            feature_index,
            function_id,
            software_id,
            parameters
        );
        let parsed = HidppPacket::from_bytes(&packet.to_bytes())?;
        prop_assert_eq!(parsed, packet);
    }

    #[test]
    fn prop_from_bytes_never_panics(bytes in proptest::collection::vec(any::<u8>(), 0..64)) {
        let _ = HidppPacket::from_bytes(&bytes);
    }

    #[test]
    fn prop_short_wire_layout(
        device_index in any::<u8>(),
        feature_index in any::<u8>(),
        function_id in 0u8..0x10,
        software_id in 0u8..0x10,
        parameters in proptest::array::uniform3(any::<u8>())
    ) {
        let bytes = HidppPacket::new_short(
            device_index,
            feature_index,
            function_id,
            software_id,
            parameters
        )
        .to_bytes();

        prop_assert_eq!(bytes.len(), SHORT_PACKET_SIZE);
        prop_assert_eq!(bytes[0], REPORT_ID_SHORT);
        prop_assert_eq!(bytes[1], device_index);
        prop_assert_eq!(bytes[2], feature_index);
        prop_assert_eq!(bytes[3], (function_id << 4) | (software_id & 0x0F));
        prop_assert_eq!(&bytes[4..7], &parameters);
    }

    #[test]
    fn prop_long_wire_layout(
        device_index in any::<u8>(),
        feature_index in any::<u8>(),
        function_id in 0u8..0x10,
        software_id in 0u8..0x10,
        parameters in proptest::array::uniform16(any::<u8>())
    ) {
        let bytes = HidppPacket::new_long(
            device_index,
            feature_index,
            function_id,
            software_id,
            parameters
        )
        .to_bytes();

        prop_assert_eq!(bytes.len(), LONG_PACKET_SIZE);
        prop_assert_eq!(bytes[0], REPORT_ID_LONG);
        prop_assert_eq!(bytes[1], device_index);
        prop_assert_eq!(bytes[2], feature_index);
        prop_assert_eq!(bytes[3], (function_id << 4) | (software_id & 0x0F));
        prop_assert_eq!(&bytes[4..20], &parameters);
    }
}
