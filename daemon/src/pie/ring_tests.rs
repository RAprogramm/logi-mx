// SPDX-FileCopyrightText: 2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Ring resolution tests: slot precedence (profile over config over gesture
//! seed), compass seeding order, the wire view, and gesture merging.

use std::collections::HashMap;

use logi_mx_driver::{
    config::{PieConfig, PieProfile, PieSlot},
    prelude::{Action, Gesture, GestureDirection, GestureMode}
};

use super::{merge_effective, ring_slots};
use crate::pie::labels::sectors_view;

fn key_action(key: &str) -> Action {
    Action::Keypress {
        keys: vec![format!("KEY_{key}")]
    }
}

fn slot(key: &str) -> PieSlot {
    PieSlot {
        action: Some(key_action(key)),
        label:  None
    }
}

fn gesture(direction: GestureDirection, key: &str) -> Gesture {
    Gesture {
        direction,
        mode: GestureMode::OnRelease,
        action: Box::new(key_action(key))
    }
}

#[test]
fn explicit_slots_win_over_the_seed() {
    let config = PieConfig {
        sectors: vec![slot("A"), slot("B"), slot("C")],
        ..PieConfig::default()
    };
    let gestures = vec![gesture(GestureDirection::Up, "Z")];
    let ring = ring_slots(&config, None, &gestures);
    assert_eq!(ring.len(), 3, "the configured ring wins over gesture seeds");
}

#[test]
fn focused_profile_wins_over_the_global_ring() {
    let config = PieConfig {
        sectors: vec![slot("A"), slot("B")],
        ..PieConfig::default()
    };
    let profile = PieProfile {
        sectors: vec![slot("X"), slot("Y"), slot("Z")]
    };
    assert_eq!(ring_slots(&config, Some(&profile), &[]).len(), 3);
    let empty = PieProfile::default();
    assert_eq!(
        ring_slots(&config, Some(&empty), &[]).len(),
        2,
        "a profile without slots falls back to the global ring"
    );
}

#[test]
fn rings_are_capped_at_eight_wedges() {
    let config = PieConfig {
        sectors: (0..12).map(|i| slot(&format!("K{i}"))).collect(),
        ..PieConfig::default()
    };
    assert_eq!(ring_slots(&config, None, &[]).len(), PieConfig::MAX_SLOTS);
}

#[test]
fn gesture_seed_follows_compass_order_from_the_top() {
    let gestures = vec![
        gesture(GestureDirection::Left, "L"),
        gesture(GestureDirection::Down, "D"),
        gesture(GestureDirection::Up, "U"),
        gesture(GestureDirection::None, "click"),
    ];
    let ring = ring_slots(&PieConfig::default(), None, &gestures);
    let keys: Vec<String> = ring
        .iter()
        .map(|slot| match slot.action.as_ref() {
            Some(Action::Keypress {
                keys
            }) => keys[0].clone(),
            _ => String::new()
        })
        .collect();
    assert_eq!(
        keys,
        vec!["KEY_U", "KEY_D", "KEY_L"],
        "slot 0 is up, then clockwise, and the click binding is not a wedge"
    );
}

#[test]
fn gesture_seed_collapses_duplicate_directions() {
    let gestures = vec![
        gesture(GestureDirection::Up, "A"),
        Gesture {
            direction: GestureDirection::Up,
            mode:      GestureMode::OnPress,
            action:    Box::new(key_action("B"))
        },
    ];
    assert_eq!(
        ring_slots(&PieConfig::default(), None, &gestures).len(),
        1,
        "one direction yields exactly one wedge"
    );
}

#[test]
fn wire_view_labels_icons_and_bound_flags() {
    let ring = vec![
        PieSlot {
            action: Some(Action::Command {
                command: "firefox".to_string()
            }),
            label:  Some("Web".to_string())
        },
        PieSlot {
            action: Some(key_action("E")),
            label:  None
        },
        PieSlot::default(),
    ];
    let view = sectors_view(&ring);
    assert_eq!(view.len(), 3);
    assert_eq!((view[0].index, view[0].label.as_str()), (0, "Web"));
    assert_eq!(view[0].command.as_deref(), Some("firefox"));
    assert!(view[0].bound);
    assert_eq!(view[1].label, "E", "keys fall back to the action summary");
    assert!(!view[2].bound, "an empty slot renders dim");
    assert_eq!(view[2].label, "");
}

#[test]
fn merge_effective_lets_profiles_replace_per_direction() {
    let global = vec![
        gesture(GestureDirection::Up, "G"),
        gesture(GestureDirection::Right, "R"),
    ];
    let profiles = HashMap::from([(
        "firefox".to_string(),
        vec![gesture(GestureDirection::Up, "F")]
    )]);
    let focused = merge_effective(&global, &profiles, Some("firefox"));
    assert_eq!(focused.len(), 2);
    assert!(
        focused.iter().any(
            |g| g.direction == GestureDirection::Up && format!("{:?}", g.action).contains('F')
        ),
        "the profile entry replaces the global binding for its direction"
    );
    let unfocused = merge_effective(&global, &profiles, Some("other"));
    assert_eq!(unfocused.len(), 2, "an unfocused profile falls through");
}
