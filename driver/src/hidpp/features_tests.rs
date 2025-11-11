// SPDX-FileCopyrightText: 2025 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

#[cfg(test)]
mod feature_tests {
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
    fn test_battery_function_values() {
        assert_eq!(BatteryFunction::GetStatus as u8, 0x00);
        assert_eq!(BatteryFunction::GetCapability as u8, 0x01);
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
        assert_eq!(SmartShiftFunction::GetRatchetControlMode as u8, 0x00);
        assert_eq!(SmartShiftFunction::SetRatchetControlMode as u8, 0x01);
    }

    #[test]
    fn test_hires_wheel_function_values() {
        assert_eq!(HiresWheelFunction::GetCapabilities as u8, 0x00);
        assert_eq!(HiresWheelFunction::GetMode as u8, 0x01);
        assert_eq!(HiresWheelFunction::SetMode as u8, 0x02);
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
}
