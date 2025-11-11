// SPDX-FileCopyrightText: 2025 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

pub const REPORT_ID_SHORT: u8 = 0x10;
pub const REPORT_ID_LONG: u8 = 0x11;
pub const REPORT_ID_VERY_LONG: u8 = 0x12;

pub const SHORT_PACKET_SIZE: usize = 7;
pub const LONG_PACKET_SIZE: usize = 20;
pub const VERY_LONG_PACKET_SIZE: usize = 64;

pub const DEVICE_INDEX_RECEIVER: u8 = 0xFF;

pub const ERROR_SUCCESS: u8 = 0x00;
pub const ERROR_INVALID_SUBID: u8 = 0x01;
pub const ERROR_INVALID_ADDRESS: u8 = 0x02;
pub const ERROR_INVALID_VALUE: u8 = 0x03;
pub const ERROR_CONNECT_FAIL: u8 = 0x04;
pub const ERROR_TOO_MANY_DEVICES: u8 = 0x05;
pub const ERROR_ALREADY_EXISTS: u8 = 0x06;
pub const ERROR_BUSY: u8 = 0x07;
pub const ERROR_UNKNOWN_DEVICE: u8 = 0x08;
pub const ERROR_RESOURCE_ERROR: u8 = 0x09;
pub const ERROR_REQUEST_UNAVAILABLE: u8 = 0x0A;
pub const ERROR_UNSUPPORTED_PARAM: u8 = 0x0B;
pub const ERROR_WRONG_PIN_CODE: u8 = 0x0C;

pub const FEATURE_ROOT: u16 = 0x0000;
pub const FEATURE_FEATURE_SET: u16 = 0x0001;
pub const FEATURE_FEATURE_INFO: u16 = 0x0002;
pub const FEATURE_DEVICE_NAME: u16 = 0x0005;
pub const FEATURE_BATTERY_STATUS: u16 = 0x1000;
pub const FEATURE_BATTERY_VOLTAGE: u16 = 0x1001;
pub const FEATURE_UNIFIED_BATTERY: u16 = 0x1004;
pub const FEATURE_ADJUSTABLE_DPI: u16 = 0x2201;
pub const FEATURE_SMART_SHIFT: u16 = 0x2110;
pub const FEATURE_HIRES_WHEEL: u16 = 0x2121;
pub const FEATURE_REPROG_CONTROLS: u16 = 0x1B04;

pub const ROOT_INDEX: u8 = 0x00;
