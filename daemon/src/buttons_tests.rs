// SPDX-FileCopyrightText: 2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

use super::*;
use crate::focus::new_shared;

fn key_action(keys: &[&str]) -> Action {
    Action::Keypress {
        keys: keys.iter().map(|name| (*name).to_string()).collect()
    }
}

fn focus_on(runtime: &mut ButtonRuntime, app: &str) {
    let focus = new_shared();
    *focus
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(app.to_string());
    runtime.attach_focus(focus);
}

fn live_bit(runtime: &ButtonRuntime, bit: u32) -> bool {
    runtime
        .live
        .virtual_bits
        .load(std::sync::atomic::Ordering::Relaxed)
        & bit
        != 0
}

#[test]
fn test_empty_config_diverts_nothing() {
    let runtime = ButtonRuntime::from_config(&Config::default());
    assert!(
        !runtime.should_divert(),
        "the default config must leave both thumb buttons native"
    );
    assert!(!runtime.wants_pump());
}

#[test]
fn test_global_non_native_diverts_and_native_does_not() {
    let mut runtime = ButtonRuntime::default();
    runtime.set_config(
        HashMap::from([(ButtonId::Back, key_action(&["KEY_LEFTALT", "KEY_LEFT"]))]),
        HashMap::new()
    );
    assert!(runtime.should_divert(), "a global chord must divert");

    runtime.set_config(
        HashMap::from([(ButtonId::Back, Action::None)]),
        HashMap::new()
    );
    assert!(
        !runtime.should_divert(),
        "an explicit native mapping must not divert"
    );
}

#[test]
fn test_profile_only_mapping_diverts() {
    let mut runtime = ButtonRuntime::default();
    runtime.set_config(
        HashMap::new(),
        HashMap::from([(
            "firefox".to_string(),
            HashMap::from([(
                ButtonId::Forward,
                Action::Command {
                    command: "xdg-forward".to_string()
                }
            )])
        )])
    );
    assert!(
        runtime.should_divert(),
        "a profile-only chord must keep the button diverted"
    );
    assert!(
        runtime.needs_divert(ButtonId::Forward),
        "only Forward needs the divert"
    );
    assert!(
        !runtime.needs_divert(ButtonId::Back),
        "Back stays native while another app is focused"
    );
}

#[test]
fn test_profile_wins_per_button_and_falls_through() {
    let mut runtime = ButtonRuntime::default();
    runtime.set_config(
        HashMap::from([
            (ButtonId::Back, key_action(&["KEY_A"])),
            (ButtonId::Forward, key_action(&["KEY_B"]))
        ]),
        HashMap::from([(
            "dota2".to_string(),
            HashMap::from([(ButtonId::Back, key_action(&["KEY_C"]))])
        )])
    );

    assert_eq!(
        runtime.effective_action(ButtonId::Back),
        Some(&key_action(&["KEY_A"])),
        "without focus the global map applies"
    );

    focus_on(&mut runtime, "dota2");
    assert_eq!(
        runtime.effective_action(ButtonId::Back),
        Some(&key_action(&["KEY_C"])),
        "the focused profile must win for its button"
    );
    assert_eq!(
        runtime.effective_action(ButtonId::Forward),
        Some(&key_action(&["KEY_B"])),
        "the untouched button falls through to the global map"
    );
}

#[test]
fn test_profile_native_entry_overrides_global_chord() {
    let mut runtime = ButtonRuntime::default();
    runtime.set_config(
        HashMap::from([(ButtonId::Back, key_action(&["KEY_A"]))]),
        HashMap::from([(
            "firefox".to_string(),
            HashMap::from([(ButtonId::Back, Action::None)])
        )])
    );
    focus_on(&mut runtime, "firefox");
    assert_eq!(
        runtime.effective_action(ButtonId::Back),
        Some(&Action::None),
        "a profile may force a button back to native"
    );
}

#[test]
fn test_native_press_release_bookkeeping() {
    let mut runtime = ButtonRuntime::default();
    assert_eq!(
        runtime.on_button_edge(ButtonId::Back, true),
        ButtonEffect::InjectPress,
        "a native press is injected back as the real button"
    );
    assert!(live_bit(&runtime, BTN_BIT_BACK), "the HUD bit must rise");
    assert_eq!(
        runtime.on_button_edge(ButtonId::Back, true),
        ButtonEffect::Idle,
        "a repeated report is not a new edge"
    );
    assert_eq!(
        runtime.on_button_edge(ButtonId::Back, false),
        ButtonEffect::InjectRelease
    );
    assert!(!live_bit(&runtime, BTN_BIT_BACK), "the HUD bit must fall");
    assert_eq!(
        runtime.on_button_edge(ButtonId::Back, false),
        ButtonEffect::Idle,
        "a release without a press is ignored"
    );
}

#[test]
fn test_chord_press_fires_once_and_release_is_quiet() {
    let mut runtime = ButtonRuntime::default();
    runtime.set_config(
        HashMap::from([(
            ButtonId::Forward,
            key_action(&["KEY_LEFTCTRL", "KEY_RIGHT"])
        )]),
        HashMap::new()
    );
    assert_eq!(
        runtime.on_button_edge(ButtonId::Forward, true),
        ButtonEffect::Chord(vec!["KEY_LEFTCTRL".to_string(), "KEY_RIGHT".to_string()]),
        "the chord fires on the press edge"
    );
    assert!(live_bit(&runtime, BTN_BIT_FORWARD));
    assert_eq!(
        runtime.on_button_edge(ButtonId::Forward, false),
        ButtonEffect::Idle,
        "a chord is self-contained, nothing to release"
    );
    assert!(!live_bit(&runtime, BTN_BIT_FORWARD));
}

/// The edge decision resolves to the configured command; the detached
/// launch itself lives in `execute` and is covered by the daemon smoke run.
#[test]
fn test_command_press_resolves_to_command_effect() {
    let mut runtime = ButtonRuntime::default();
    runtime.set_config(
        HashMap::from([(
            ButtonId::Back,
            Action::Command {
                command: "true".to_string()
            }
        )]),
        HashMap::new()
    );
    assert_eq!(
        runtime.on_button_edge(ButtonId::Back, true),
        ButtonEffect::Command("true".to_string())
    );
    assert_eq!(
        runtime.on_button_edge(ButtonId::Back, false),
        ButtonEffect::Idle,
        "a command has nothing to release"
    );
}

#[test]
fn test_unsupported_action_falls_back_to_native() {
    let mut runtime = ButtonRuntime::default();
    runtime.set_config(
        HashMap::from([(ButtonId::Back, Action::ToggleSmartShift)]),
        HashMap::new()
    );
    assert_eq!(
        runtime.on_button_edge(ButtonId::Back, true),
        ButtonEffect::InjectPress,
        "an action the runtime cannot run must keep the button usable"
    );
}

#[test]
fn test_is_remapped_decision_table() {
    assert!(!is_remapped(None));
    assert!(!is_remapped(Some(&Action::None)));
    assert!(!is_remapped(Some(&Action::ToggleSmartShift)));
    assert!(is_remapped(Some(&key_action(&["KEY_A"]))));
    assert!(is_remapped(Some(&Action::Command {
        command: "true".to_string()
    })));
}

#[test]
fn test_native_button_codes_cross_over_as_on_the_device() {
    assert_eq!(crate::inject::native_button_code(ButtonId::Back), Some(276));
    assert_eq!(
        crate::inject::native_button_code(ButtonId::Forward),
        Some(275)
    );
    assert_eq!(crate::inject::native_button_code(ButtonId::LeftClick), None);
}

#[test]
fn test_control_and_bit_mapping() {
    assert_eq!(control_for(ButtonId::Back), control_id::BACK_BUTTON);
    assert_eq!(control_for(ButtonId::Forward), control_id::FORWARD_BUTTON);
    assert_eq!(live_bit_for(ButtonId::Back), BTN_BIT_BACK);
    assert_eq!(live_bit_for(ButtonId::Forward), BTN_BIT_FORWARD);
}
