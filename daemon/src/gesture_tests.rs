// SPDX-FileCopyrightText: 2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

use super::*;

#[test]
fn test_runtime_gestures_are_opt_in() {
    let config = Config::default();
    let runtime = GestureRuntime::from_config(&config);
    assert!(!runtime.should_divert(), "default config must not divert");
    assert!(!runtime.is_holding());
}

#[test]
fn test_runtime_diverts_only_with_opt_in_and_bindings() {
    let mut config = Config::default();
    config.devices[0].gestures_enabled = true;
    assert!(GestureRuntime::from_config(&config).should_divert());

    config.devices[0].buttons.remove(&ButtonId::ThumbGesture);
    assert!(
        !GestureRuntime::from_config(&config).should_divert(),
        "an empty binding set must not divert the button"
    );
}

#[test]
fn test_runtime_empty_config_is_inert() {
    let runtime = GestureRuntime::from_config(&Config {
        devices: Vec::new()
    });
    assert!(!runtime.should_divert());
}

fn key_gesture(direction: GestureDirection, key: &str) -> Gesture {
    Gesture {
        direction,
        mode: GestureMode::OnRelease,
        action: Box::new(Action::Keypress {
            keys: vec![key.to_string()]
        })
    }
}

#[test]
fn test_profile_replaces_only_its_direction() {
    let mut runtime = GestureRuntime::default();
    runtime.set_config(
        true,
        vec![
            key_gesture(GestureDirection::Left, "KEY_A"),
            key_gesture(GestureDirection::Right, "KEY_B"),
        ],
        HashMap::from([(
            "firefox".to_string(),
            vec![key_gesture(GestureDirection::Left, "KEY_C")]
        )])
    );

    assert_eq!(
        runtime.effective_bindings().len(),
        2,
        "without focus the global set applies"
    );

    runtime.attach_focus(crate::focus::new_shared());
    *runtime
        .focus
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some("firefox".to_string());

    let effective = runtime.effective_bindings();
    let left = effective
        .iter()
        .find(|g| g.direction == GestureDirection::Left)
        .expect("left bound");
    assert!(
        matches!(left.action.as_ref(), Action::Keypress { keys } if keys == &vec!["KEY_C".to_string()]),
        "the focused profile must win for its direction"
    );
    assert_eq!(effective.len(), 2, "the untouched direction falls through");
}

#[test]
fn test_profiles_alone_still_require_the_divert() {
    let mut runtime = GestureRuntime::default();
    runtime.set_config(
        true,
        Vec::new(),
        HashMap::from([(
            "dota2".to_string(),
            vec![key_gesture(GestureDirection::Up, "KEY_UP")]
        )])
    );
    assert!(
        runtime.should_divert(),
        "a non-empty profile must keep the button diverted"
    );
    runtime.set_config(false, Vec::new(), HashMap::new());
    assert!(!runtime.should_divert(), "disabling releases the button");
}

#[test]
fn test_runtime_release_without_press_is_ignored() {
    let mut runtime = GestureRuntime::default();
    runtime.on_gesture_edge(false);
    assert!(!runtime.is_holding());
}

#[test]
fn test_runtime_tracks_press_and_release() {
    let mut runtime = GestureRuntime::default();
    runtime.on_gesture_edge(true);
    assert!(runtime.is_holding());
    runtime.on_gesture_edge(false);
    assert!(!runtime.is_holding());
}

#[test]
fn test_runtime_press_and_release_move_the_live_bit() {
    let mut runtime = GestureRuntime::default();
    runtime.on_gesture_edge(true);
    assert_ne!(
        runtime
            .live
            .virtual_bits
            .load(std::sync::atomic::Ordering::Relaxed)
            & BTN_BIT_GESTURE,
        0,
        "a held gesture button must be visible on live telemetry"
    );
    runtime.on_gesture_edge(false);
    assert_eq!(
        runtime
            .live
            .virtual_bits
            .load(std::sync::atomic::Ordering::Relaxed)
            & BTN_BIT_GESTURE,
        0,
        "releasing must clear the live bit again"
    );
}

#[test]
fn test_runtime_ignores_other_diverted_controls() {
    let mut runtime = GestureRuntime::default();
    runtime.on_gesture_edge(false);
    assert!(
        !runtime.is_holding(),
        "only the gesture button starts a hold"
    );
}
