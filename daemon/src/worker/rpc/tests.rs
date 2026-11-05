// SPDX-FileCopyrightText: 2025-2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Tests for the RPC validation gates and the config mirroring mapping.

use logi_mx_driver::prelude::{Action, ButtonId, Config, PieConfig, PieSlot};

use super::{
    settings::apply_setting,
    validate::{action_rejection, ring_rejection, thumb_button_rejection}
};
use crate::ipc;

#[test]
fn test_apply_setting_mirrors_device_writes_into_config() {
    let mut config = Config::default();
    assert!(apply_setting(
        &mut config,
        &ipc::Request::SetHires {
            enabled:  true,
            inverted: true
        }
    ));
    assert!(config.devices[0].hiresscroll.enabled);
    assert!(config.devices[0].hiresscroll.inverted);

    assert!(apply_setting(
        &mut config,
        &ipc::Request::SetDpi {
            value: 1600
        }
    ));
    assert_eq!(config.devices[0].dpi, 1600);

    assert!(apply_setting(
        &mut config,
        &ipc::Request::SetSmartShift {
            enabled:   false,
            threshold: 13
        }
    ));
    assert!(!config.devices[0].smartshift.enabled);
    assert_eq!(config.devices[0].smartshift.threshold, 13);
}

#[test]
fn ring_rejection_checks_the_gate_and_the_slot_cap() {
    let base = PieConfig::default();
    let slots = |count: usize| (0..count).map(|_| PieSlot::default()).collect::<Vec<_>>();
    assert!(ring_rejection(&base, &slots(2)).is_none());
    assert!(ring_rejection(&base, &slots(8)).is_none());
    assert!(
        ring_rejection(&base, &slots(9)).is_some(),
        "nine wedges are too thin to hit"
    );
    assert!(
        ring_rejection(&base, &[]).is_none(),
        "an empty ring is legal to store"
    );
    let wide_gate = PieConfig {
        activation_radius: 40,
        dead_zone: 41,
        ..PieConfig::default()
    };
    assert!(
        ring_rejection(&wide_gate, &slots(4)).is_some(),
        "the stored gate rules ride along with the ring check"
    );
}

#[test]
fn apply_setting_writes_pie_rings_and_profiles() {
    let mut config = Config::default();

    assert!(apply_setting(
        &mut config,
        &ipc::Request::SetPieSectors {
            sectors: (0..4).map(|_| PieSlot::default()).collect()
        }
    ));
    assert_eq!(config.devices[0].pie.sectors.len(), 4);

    assert!(apply_setting(
        &mut config,
        &ipc::Request::SetAppPieSectors {
            app_id:  "minecraft".to_string(),
            sectors: (0..6).map(|_| PieSlot::default()).collect()
        }
    ));
    assert_eq!(config.devices[0].pie_profiles["minecraft"].sectors.len(), 6);

    assert!(apply_setting(
        &mut config,
        &ipc::Request::SetAppPieSectors {
            app_id:  "minecraft".to_string(),
            sectors: Vec::new()
        }
    ));
    assert!(
        !config.devices[0].pie_profiles.contains_key("minecraft"),
        "an empty app ring deletes the profile instead of shadowing the global one"
    );

    assert!(apply_setting(
        &mut config,
        &ipc::Request::RemoveAppPieProfile {
            app_id: "firefox".to_string()
        }
    ));
    assert!(!config.devices[0].pie_profiles.contains_key("firefox"));
}

#[test]
fn test_apply_setting_ignores_non_config_requests() {
    let mut config = Config::default();
    let before_dpi = config.devices[0].dpi;
    assert!(
        !apply_setting(&mut config, &ipc::Request::Ping),
        "reads must not touch the configuration"
    );
    assert_eq!(config.devices[0].dpi, before_dpi);

    let mut empty = Config {
        devices: Vec::new()
    };
    assert!(
        !apply_setting(
            &mut empty,
            &ipc::Request::SetDpi {
                value: 800
            }
        ),
        "without a device entry there is nothing to mirror into"
    );
}

fn chord(keys: &[&str]) -> Action {
    Action::Keypress {
        keys: keys.iter().map(|key| (*key).to_string()).collect()
    }
}

#[test]
fn action_rejection_accepts_valid_and_rejects_broken() {
    assert!(
        action_rejection(&Action::Keypress {
            keys: vec!["KEY_LEFTCTRL".to_string(), "KEY_C".to_string()]
        })
        .is_none()
    );
    assert!(
        action_rejection(&Action::Command {
            command: "x".to_string()
        })
        .is_none()
    );
    assert!(action_rejection(&Action::None).is_some());
    assert!(
        action_rejection(&Action::Keypress {
            keys: Vec::new()
        })
        .is_some()
    );
    assert!(
        action_rejection(&Action::Keypress {
            keys: vec!["NOT_A_KEY".to_string()]
        })
        .is_some()
    );
    assert!(
        action_rejection(&Action::Command {
            command: "   ".to_string()
        })
        .is_some()
    );
    assert!(action_rejection(&Action::ToggleSmartShift).is_some());
}

#[test]
fn thumb_button_rejection_accepts_only_thumb_buttons() {
    assert!(thumb_button_rejection(ButtonId::Back).is_none());
    assert!(thumb_button_rejection(ButtonId::Forward).is_none());
    assert!(thumb_button_rejection(ButtonId::LeftClick).is_some());
    assert!(thumb_button_rejection(ButtonId::ThumbGesture).is_some());
}

#[test]
fn apply_setting_writes_button_fields() {
    let mut config = Config::default();

    assert!(apply_setting(
        &mut config,
        &ipc::Request::SetButton {
            button: ButtonId::Back,
            action: Box::new(chord(&["KEY_LEFTALT"]))
        }
    ));
    assert!(apply_setting(
        &mut config,
        &ipc::Request::SetAppButton {
            app_id: "firefox".to_string(),
            button: ButtonId::Forward,
            action: Box::new(chord(&["KEY_LEFTALT"]))
        }
    ));

    let device = &config.devices[0];
    assert!(device.buttons.contains_key(&ButtonId::Back));
    assert_eq!(
        device
            .button_profiles
            .get("firefox")
            .and_then(|profile| profile.get(&ButtonId::Forward)),
        Some(&Action::Keypress {
            keys: vec!["KEY_LEFTALT".to_string()]
        })
    );

    assert!(apply_setting(
        &mut config,
        &ipc::Request::ClearButton {
            button: ButtonId::Back
        }
    ));
    assert!(apply_setting(
        &mut config,
        &ipc::Request::ClearAppButton {
            app_id: "firefox".to_string(),
            button: ButtonId::Forward
        }
    ));

    let device = &config.devices[0];
    assert!(!device.buttons.contains_key(&ButtonId::Back));
    assert!(
        !device.button_profiles.contains_key("firefox"),
        "an emptied profile must not linger in the configuration"
    );
}

#[test]
fn apply_setting_removes_whole_button_profile() {
    let mut config = Config::default();
    assert!(apply_setting(
        &mut config,
        &ipc::Request::SetAppButton {
            app_id: "org.gnome.Nautilus".to_string(),
            button: ButtonId::Back,
            action: Box::new(Action::Command {
                command: "thunar".to_string()
            })
        }
    ));
    assert!(
        config.devices[0]
            .button_profiles
            .contains_key("org.gnome.Nautilus")
    );

    assert!(apply_setting(
        &mut config,
        &ipc::Request::RemoveAppButtonProfile {
            app_id: "org.gnome.Nautilus".to_string()
        }
    ));
    assert!(
        !config.devices[0]
            .button_profiles
            .contains_key("org.gnome.Nautilus")
    );
}

#[test]
fn apply_setting_ignores_button_reads() {
    let mut config = Config::default();
    assert!(
        !apply_setting(&mut config, &ipc::Request::GetButtons),
        "reads must not touch the configuration"
    );
    assert!(!config.devices[0].buttons.contains_key(&ButtonId::Back));
    assert!(config.devices[0].button_profiles.is_empty());
}
