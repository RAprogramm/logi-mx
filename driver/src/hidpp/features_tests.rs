// SPDX-FileCopyrightText: 2025 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

use super::*;

#[test]
fn test_feature_creation() {
    let feature = Feature::new(0x1234, 0x05);
    assert_eq!(feature.id, 0x1234);
    assert_eq!(feature.index, 0x05);
}

#[test]
fn test_feature_root() {
    let root = Feature::root();
    assert_eq!(root.id, FEATURE_ROOT);
    assert_eq!(root.index, ROOT_INDEX);
}

#[test]
fn test_root_function_values() {
    assert_eq!(RootFunction::GetFeature as u8, 0x00);
    assert_eq!(RootFunction::Ping as u8, 0x01);
}

#[test]
fn test_device_name_function_values() {
    assert_eq!(DeviceNameFunction::GetNameLength as u8, 0x00);
    assert_eq!(DeviceNameFunction::GetName as u8, 0x01);
}

#[test]
fn test_battery_status_function_values() {
    assert_eq!(BatteryStatusFunction::GetStatus as u8, 0x00);
}

#[test]
fn test_battery_unified_function_values() {
    assert_eq!(BatteryUnifiedFunction::GetCapabilities as u8, 0x00);
    assert_eq!(BatteryUnifiedFunction::GetStatus as u8, 0x01);
}

#[test]
fn test_dpi_function_values() {
    assert_eq!(DpiFunction::GetSensorCount as u8, 0x00);
    assert_eq!(DpiFunction::GetSensorDpiList as u8, 0x01);
    assert_eq!(DpiFunction::GetSensorDpi as u8, 0x02);
    assert_eq!(DpiFunction::SetSensorDpi as u8, 0x03);
}

#[test]
fn test_smartshift_function_values() {
    assert_eq!(SmartShiftFunction::GetStatus as u8, 0x00);
    assert_eq!(SmartShiftFunction::SetStatus as u8, 0x01);
}

#[test]
fn test_hires_wheel_function_values() {
    assert_eq!(HiresWheelFunction::GetCapabilities as u8, 0x00);
    assert_eq!(HiresWheelFunction::GetWheelMode as u8, 0x01);
    assert_eq!(HiresWheelFunction::SetWheelMode as u8, 0x02);
    assert_eq!(HiresWheelFunction::GetRatchetSwitchState as u8, 0x03);
}

#[test]
fn test_reprog_controls_function_values() {
    assert_eq!(ReprogControlsFunction::GetControlCount as u8, 0x00);
    assert_eq!(ReprogControlsFunction::GetControlInfo as u8, 0x01);
    assert_eq!(ReprogControlsFunction::GetControlReporting as u8, 0x02);
    assert_eq!(ReprogControlsFunction::SetControlReporting as u8, 0x03);
}

#[test]
fn test_thumb_wheel_function_values() {
    assert_eq!(ThumbWheelFunction::GetConfig as u8, 0x00);
    assert_eq!(ThumbWheelFunction::SetConfig as u8, 0x01);
}

#[test]
fn test_change_host_function_values() {
    assert_eq!(ChangeHostFunction::GetHostInfo as u8, 0x00);
    assert_eq!(ChangeHostFunction::SetHost as u8, 0x01);
}

#[test]
fn test_feature_equality() {
    let f1 = Feature::new(0x1000, 0x01);
    let f2 = Feature::new(0x1000, 0x01);
    assert_eq!(f1, f2);
}

#[test]
fn test_feature_inequality() {
    let f1 = Feature::new(0x1000, 0x01);
    let f2 = Feature::new(0x1000, 0x02);
    assert_ne!(f1, f2);
}

#[test]
fn test_feature_copy() {
    let f1 = Feature::new(0x2000, 0x03);
    let f2 = f1;
    assert_eq!(f1, f2);
}
