// SPDX-FileCopyrightText: 2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

use std::time::{Duration, Instant};

use super::*;

fn base() -> Instant {
    Instant::now()
}

fn at(ms: u64) -> Duration {
    Duration::from_millis(ms)
}

/// Starts a hold and consumes the MX Master 3S pre-press burst report.
fn press(acc: &mut SwipeAccumulator, t0: Instant) {
    acc.begin(t0);
    assert_eq!(acc.accumulate(451, 22, t0), None, "burst must be dropped");
}

#[test]
fn test_detect_swipe_thresholds() {
    assert_eq!(detect_swipe(50, 0), Some(GestureDirection::Right));
    assert_eq!(detect_swipe(-49, 0), None);
    assert_eq!(detect_swipe(0, -60), Some(GestureDirection::Up));
    assert_eq!(detect_swipe(0, 60), Some(GestureDirection::Down));
}

#[test]
fn test_detect_swipe_deadzone_rejects_diagonals() {
    assert_eq!(detect_swipe(60, 60), None);
    assert_eq!(detect_swipe(60, 20), Some(GestureDirection::Right));
    assert_eq!(detect_swipe(200, 60), Some(GestureDirection::Right));
    assert_eq!(detect_swipe(200, 90), None);
}

#[test]
fn test_accumulate_requires_hold() {
    let mut acc = SwipeAccumulator::default();
    assert_eq!(acc.accumulate(100, 0, base()), None);
    assert_eq!(acc.end(), None);
    assert!(!acc.is_holding());
}

#[test]
fn test_plain_press_is_click() {
    let t0 = base();
    let mut acc = SwipeAccumulator::default();
    press(&mut acc, t0);
    assert!(acc.is_holding());
    assert_eq!(acc.end(), Some(GestureDirection::None));
    assert!(!acc.is_holding());
}

#[test]
fn test_hold_gate_blocks_early_commit() {
    let t0 = base();
    let mut acc = SwipeAccumulator::default();
    press(&mut acc, t0);
    assert_eq!(acc.accumulate(80, 0, t0 + at(50)), None, "gate must block");
}

#[test]
fn test_quick_flick_fires_on_release() {
    let t0 = base();
    let mut acc = SwipeAccumulator::default();
    press(&mut acc, t0);
    assert_eq!(acc.accumulate(90, 0, t0 + at(50)), None);
    assert_eq!(acc.end(), Some(GestureDirection::Right));
}

#[test]
fn test_swipe_commits_mid_hold() {
    let t0 = base();
    let mut acc = SwipeAccumulator::default();
    press(&mut acc, t0);
    assert_eq!(acc.accumulate(30, 0, t0 + at(100)), None);
    assert_eq!(
        acc.accumulate(30, 0, t0 + at(200)),
        Some(GestureDirection::Right)
    );
    assert_eq!(acc.end(), None, "a fired hold must not also click");
}

#[test]
fn test_follow_through_does_not_double_fire() {
    let t0 = base();
    let mut acc = SwipeAccumulator::default();
    press(&mut acc, t0);
    assert_eq!(
        acc.accumulate(60, 0, t0 + at(200)),
        Some(GestureDirection::Right)
    );
    for step in 0..5 {
        assert_eq!(
            acc.accumulate(60, 0, t0 + at(250 + step * 20)),
            None,
            "follow-through below repeat distance must not fire"
        );
    }
}

#[test]
fn test_repeat_fires_after_repeat_distance() {
    let t0 = base();
    let mut acc = SwipeAccumulator::default();
    press(&mut acc, t0);
    assert_eq!(
        acc.accumulate(60, 0, t0 + at(200)),
        Some(GestureDirection::Right)
    );
    let mut fired = Vec::new();
    for step in 0..12 {
        if let Some(dir) = acc.accumulate(60, 0, t0 + at(250 + step * 20)) {
            fired.push(dir);
        }
    }
    assert_eq!(fired, vec![GestureDirection::Right]);
}

#[test]
fn test_direction_change_chains_without_undowing() {
    let t0 = base();
    let mut acc = SwipeAccumulator::default();
    press(&mut acc, t0);
    assert_eq!(
        acc.accumulate(300, 0, t0 + at(200)),
        Some(GestureDirection::Right)
    );
    assert_eq!(acc.accumulate(-30, 0, t0 + at(250)), None);
    assert_eq!(
        acc.accumulate(-40, 0, t0 + at(300)),
        Some(GestureDirection::Left),
        "reversal must commit from the pinned pivot, not undo the swipe"
    );
}

#[test]
fn test_rearm_pause_allows_same_direction_again() {
    let t0 = base();
    let mut acc = SwipeAccumulator::default();
    press(&mut acc, t0);
    assert_eq!(
        acc.accumulate(60, 0, t0 + at(200)),
        Some(GestureDirection::Right)
    );
    assert_eq!(
        acc.accumulate(0, 0, t0 + at(500)),
        None,
        "rest ends the stroke"
    );
    assert_eq!(
        acc.accumulate(60, 0, t0 + at(550)),
        Some(GestureDirection::Right),
        "after the pause the same direction may fire at the normal threshold"
    );
}

#[test]
fn test_diagonal_release_is_click() {
    let t0 = base();
    let mut acc = SwipeAccumulator::default();
    press(&mut acc, t0);
    assert_eq!(acc.accumulate(70, 70, t0 + at(200)), None);
    assert_eq!(acc.end(), Some(GestureDirection::None));
}

#[test]
fn test_saturating_travel_never_panics() {
    let t0 = base();
    let mut acc = SwipeAccumulator::default();
    press(&mut acc, t0);
    for step in 0..1000 {
        acc.accumulate(i32::MAX, i32::MIN, t0 + at(step));
    }
    let _ = acc.end();
}

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

#[test]
fn test_runtime_release_without_press_is_ignored() {
    let mut runtime = GestureRuntime::default();
    runtime.on_notification(ReprogNotification::DivertedButtons {
        pressed: vec![]
    });
    assert!(!runtime.is_holding());
}

#[test]
fn test_runtime_tracks_press_and_release() {
    let mut runtime = GestureRuntime::default();
    runtime.on_notification(ReprogNotification::DivertedButtons {
        pressed: vec![control_id::GESTURE_BUTTON]
    });
    assert!(runtime.is_holding());
    runtime.on_notification(ReprogNotification::DivertedButtons {
        pressed: vec![]
    });
    assert!(!runtime.is_holding());
}

#[test]
fn test_runtime_ignores_other_diverted_controls() {
    let mut runtime = GestureRuntime::default();
    runtime.on_notification(ReprogNotification::DivertedButtons {
        pressed: vec![0x00C4]
    });
    assert!(
        !runtime.is_holding(),
        "only the gesture button starts a hold"
    );
}
