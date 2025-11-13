// SPDX-FileCopyrightText: 2025 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

use logi_mx_driver::prelude::*;

#[test]
fn test_config_roundtrip() {
    let config = Config::default();

    let toml_str = toml::to_string(&config).unwrap();
    let parsed: Config = toml::from_str(&toml_str).unwrap();

    assert_eq!(config.devices.len(), parsed.devices.len());
    assert_eq!(config.devices[0].name, parsed.devices[0].name);
    assert_eq!(config.devices[0].dpi, parsed.devices[0].dpi);
}

#[test]
fn test_smartshift_config_default() {
    let config = SmartShiftConfig::default();
    assert!(!config.enabled);
    assert_eq!(config.threshold, 0);
}

#[test]
fn test_hires_scroll_config_default() {
    let config = HiResScrollConfig::default();
    assert!(!config.enabled);
    assert!(!config.inverted);
}

#[test]
fn test_battery_status_equality() {
    assert_eq!(BatteryStatus::Charging, BatteryStatus::Charging);
    assert_ne!(BatteryStatus::Charging, BatteryStatus::Discharging);
}

#[test]
fn test_button_id_hashing() {
    use std::collections::HashSet;

    let mut buttons = HashSet::new();
    buttons.insert(ButtonId::LeftClick);
    buttons.insert(ButtonId::RightClick);
    buttons.insert(ButtonId::LeftClick);

    assert_eq!(buttons.len(), 2);
    assert!(buttons.contains(&ButtonId::LeftClick));
    assert!(buttons.contains(&ButtonId::RightClick));
}

#[test]
fn test_action_serialization() {
    let action = Action::ToggleSmartShift;
    let serialized = toml::to_string(&action).unwrap();
    let deserialized: Action = toml::from_str(&serialized).unwrap();
    assert_eq!(action, deserialized);
}

#[test]
fn test_action_keypress() {
    let action = Action::Keypress {
        keys: vec!["KEY_A".to_string(), "KEY_B".to_string()]
    };

    match action {
        Action::Keypress {
            keys
        } => {
            assert_eq!(keys.len(), 2);
            assert_eq!(keys[0], "KEY_A");
        }
        _ => panic!("Expected Keypress action")
    }
}

#[test]
fn test_gesture_direction() {
    let directions = [
        GestureDirection::Up,
        GestureDirection::Down,
        GestureDirection::Left,
        GestureDirection::Right,
        GestureDirection::None
    ];

    assert_eq!(directions.len(), 5);
}

#[test]
fn test_device_config_validation() {
    let config = DeviceConfig::default();

    assert!(!config.name.is_empty());
    assert!(config.dpi > 0);
    assert!(config.dpi <= 8000);
}

#[test]
fn test_config_file_not_exists() {
    use std::path::PathBuf;

    let non_existent = PathBuf::from("/tmp/non_existent_logi_mx.toml");
    let result = load_config_from_path(&non_existent);

    assert!(result.is_err());
}

#[test]
fn test_empty_config_devices() {
    let mut config = Config::default();
    config.devices.clear();

    assert_eq!(config.devices.len(), 0);
}

#[test]
fn test_multiple_devices_config() {
    let mut config = Config::default();
    config.devices.push(DeviceConfig {
        name:         "Second Device".to_string(),
        dpi:          2000,
        smartshift:   SmartShiftConfig {
            enabled:   true,
            threshold: 30
        },
        hiresscroll:  HiResScrollConfig {
            enabled:  false,
            inverted: true
        },
        scroll_wheel: ScrollWheelConfig::default(),
        thumbwheel:   ThumbWheelConfig::default(),
        buttons:      std::collections::HashMap::new()
    });

    assert_eq!(config.devices.len(), 2);
    assert_eq!(config.devices[1].dpi, 2000);
}

#[test]
fn test_battery_info_creation() {
    let battery = BatteryInfo {
        level:  75,
        status: BatteryStatus::Discharging
    };

    assert_eq!(battery.level, 75);
    assert_eq!(battery.status, BatteryStatus::Discharging);
}

#[test]
fn test_gesture_mode() {
    let mode_release = GestureMode::OnRelease;
    let mode_press = GestureMode::OnPress;

    assert_ne!(mode_release, mode_press);
}
