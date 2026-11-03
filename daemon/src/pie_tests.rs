// SPDX-FileCopyrightText: 2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Pure tests for the pie state machine and the octant function: no sockets,
//! no time, scripted cursor readers only.

use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering}
    }
};

use logi_mx_driver::prelude::{Action, Gesture, GestureDirection, GestureMode};
use tokio::sync::broadcast::{Receiver, error::TryRecvError};

use super::{PieEvent, PieRuntime, PieSector, sector_for};

fn key_gesture(direction: GestureDirection) -> Gesture {
    Gesture {
        direction,
        mode: GestureMode::OnRelease,
        action: Box::new(Action::Keypress {
            keys: vec!["KEY_A".to_string()]
        })
    }
}

fn enabled_config() -> super::PieConfig {
    super::PieConfig {
        enabled:           true,
        activation_radius: 40,
        dead_zone:         12
    }
}

/// Builds an enabled runtime whose cursor reader replays `positions`:
/// every call consumes the next entry and repeats the last one once the
/// script is spent.
fn scripted(positions: &[(i32, i32)], bindings: Vec<Gesture>) -> (PieRuntime, Receiver<PieEvent>) {
    let script = Arc::new(Mutex::new(positions.to_vec()));
    let cursor = move || {
        let mut guard = script
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if guard.len() > 1 {
            Some(guard.remove(0))
        } else {
            guard.first().copied()
        }
    };
    let mut runtime = PieRuntime::with_fetch(cursor);
    let (hub, rx) = tokio::sync::broadcast::channel(64);
    runtime.attach_hub(hub);
    runtime.set_config(enabled_config());
    runtime.set_bindings(bindings, HashMap::new());
    (runtime, rx)
}

fn drain(rx: &mut Receiver<PieEvent>) -> Vec<PieEvent> {
    let mut events = Vec::new();
    while let Ok(event) = rx.try_recv() {
        events.push(event);
    }
    events
}

#[test]
fn sector_for_maps_octant_centers() {
    assert_eq!(sector_for(10, 0, 0), Some(GestureDirection::Right));
    assert_eq!(sector_for(10, 10, 0), Some(GestureDirection::DownRight));
    assert_eq!(sector_for(0, 10, 0), Some(GestureDirection::Down));
    assert_eq!(sector_for(-10, 10, 0), Some(GestureDirection::DownLeft));
    assert_eq!(sector_for(-10, 0, 0), Some(GestureDirection::Left));
    assert_eq!(sector_for(-10, -10, 0), Some(GestureDirection::UpLeft));
    assert_eq!(sector_for(0, -10, 0), Some(GestureDirection::Up));
    assert_eq!(sector_for(10, -10, 0), Some(GestureDirection::UpRight));
}

#[test]
fn sector_for_respects_octant_boundaries() {
    assert_eq!(sector_for(100, 41, 0), Some(GestureDirection::Right));
    assert_eq!(sector_for(100, 42, 0), Some(GestureDirection::DownRight));
    assert_eq!(sector_for(100, 241, 0), Some(GestureDirection::DownRight));
    assert_eq!(sector_for(100, 242, 0), Some(GestureDirection::Down));
    assert_eq!(sector_for(-100, -41, 0), Some(GestureDirection::Left));
    assert_eq!(sector_for(-100, -42, 0), Some(GestureDirection::UpLeft));
    assert_eq!(sector_for(-100, -241, 0), Some(GestureDirection::UpLeft));
    assert_eq!(sector_for(-100, -242, 0), Some(GestureDirection::Up));
    assert_eq!(sector_for(100, -241, 0), Some(GestureDirection::UpRight));
    assert_eq!(sector_for(100, -242, 0), Some(GestureDirection::Up));
    assert_eq!(sector_for(-100, 241, 0), Some(GestureDirection::DownLeft));
    assert_eq!(sector_for(-100, 242, 0), Some(GestureDirection::Down));
}

#[test]
fn sector_for_dead_zone_is_inclusive() {
    assert_eq!(sector_for(3, 4, 5), None);
    assert_eq!(sector_for(-3, -4, 5), None);
    assert_eq!(sector_for(3, 4, 4), Some(GestureDirection::DownRight));
    assert_eq!(sector_for(0, 0, 0), None);
    assert_eq!(sector_for(0, 0, 12), None);
    assert_eq!(sector_for(1, 0, 0), Some(GestureDirection::Right));
    assert_eq!(sector_for(0, 0, -5), None);
    assert_eq!(sector_for(1, 0, -5), Some(GestureDirection::Right));
}

#[test]
fn sector_for_survives_extreme_vectors() {
    assert_eq!(
        sector_for(i32::MAX, i32::MAX, 0),
        Some(GestureDirection::DownRight)
    );
    assert_eq!(
        sector_for(i32::MIN, i32::MIN, 10),
        Some(GestureDirection::UpLeft)
    );
    assert_eq!(sector_for(i32::MIN, 0, 0), Some(GestureDirection::Left));
    assert_eq!(sector_for(0, i32::MIN, 0), Some(GestureDirection::Up));
    assert_eq!(sector_for(i32::MIN, 0, i32::MAX), None);
}

#[test]
fn sector_for_never_returns_direction_none_outside_the_dead_zone() {
    for dx in -60..=60 {
        for dy in -60..=60 {
            let outside = i64::from(dx) * i64::from(dx) + i64::from(dy) * i64::from(dy) > 100;
            let sector = sector_for(dx, dy, 10);
            assert_eq!(
                sector.is_some_and(|dir| dir != GestureDirection::None),
                outside,
                "vector ({dx}, {dy}) must map outside the dead zone and never to None"
            );
        }
    }
}

#[test]
fn tap_emits_no_events_and_commits_nothing() {
    let (mut pie, mut rx) = scripted(&[(100, 100)], vec![key_gesture(GestureDirection::Up)]);
    assert!(pie.should_open());
    pie.on_press();
    pie.on_tick();
    assert!(!pie.is_open(), "a tap never passes the activation radius");
    assert_eq!(pie.on_release(), None);
    assert!(drain(&mut rx).is_empty(), "a tap must not emit any event");
}

#[test]
fn open_and_highlight_fire_only_on_change() {
    let bindings = vec![
        key_gesture(GestureDirection::Up),
        key_gesture(GestureDirection::Right),
    ];
    let (mut pie, mut rx) = scripted(&[(0, 0), (100, 0), (100, 0), (0, 100)], bindings);
    pie.on_press();
    pie.on_tick();
    pie.on_tick();
    assert!(pie.is_open());
    assert_eq!(
        drain(&mut rx),
        vec![
            PieEvent::Open {
                anchor:  (0, 0),
                sectors: vec![
                    PieSector {
                        direction: GestureDirection::Up,
                        label:     "Up".to_string()
                    },
                    PieSector {
                        direction: GestureDirection::Right,
                        label:     "Right".to_string()
                    }
                ]
            },
            PieEvent::Highlight {
                direction: Some(GestureDirection::Right)
            }
        ],
        "the second identical tick must not repeat Open or Highlight"
    );
    pie.on_tick();
    assert_eq!(
        drain(&mut rx),
        vec![PieEvent::Highlight {
            direction: Some(GestureDirection::Down)
        }]
    );
}

#[test]
fn release_over_bound_sector_commits_and_closes() {
    let (mut pie, mut rx) = scripted(
        &[(0, 0), (0, -100)],
        vec![key_gesture(GestureDirection::Up)]
    );
    pie.on_press();
    pie.on_tick();
    assert_eq!(pie.on_release(), Some(GestureDirection::Up));
    assert_eq!(
        drain(&mut rx),
        vec![
            PieEvent::Open {
                anchor:  (0, 0),
                sectors: vec![PieSector {
                    direction: GestureDirection::Up,
                    label:     "Up".to_string()
                }]
            },
            PieEvent::Highlight {
                direction: Some(GestureDirection::Up)
            },
            PieEvent::Close {
                committed: Some(GestureDirection::Up)
            }
        ]
    );
    assert!(
        !pie.armed && !pie.open,
        "the hold must be settled after release"
    );
}

#[test]
fn release_over_unbound_sector_closes_without_commit() {
    let (mut pie, mut rx) = scripted(&[(0, 0), (100, 0)], vec![key_gesture(GestureDirection::Up)]);
    pie.on_press();
    pie.on_tick();
    assert_eq!(pie.on_release(), None);
    assert_eq!(
        drain(&mut rx).last(),
        Some(&PieEvent::Close {
            committed: None
        })
    );
}

#[test]
fn release_without_activation_returns_none_and_emits_nothing() {
    let (mut pie, mut rx) = scripted(&[(0, 0), (20, 10)], vec![key_gesture(GestureDirection::Up)]);
    pie.on_press();
    pie.on_tick();
    assert!(
        !pie.is_open(),
        "the cursor stayed inside the activation radius"
    );
    assert_eq!(pie.on_release(), None);
    assert_eq!(
        drain(&mut rx),
        Vec::new(),
        "a release inside the activation radius must stay silent"
    );
}

#[test]
fn highlight_falls_to_none_inside_the_dead_zone() {
    let (mut pie, mut rx) = scripted(
        &[(0, 0), (100, 0), (5, 0)],
        vec![key_gesture(GestureDirection::Up)]
    );
    pie.on_press();
    pie.on_tick();
    pie.on_tick();
    assert_eq!(
        drain(&mut rx).last(),
        Some(&PieEvent::Highlight {
            direction: None
        }),
        "returning into the dead zone must highlight nothing"
    );
}

#[test]
fn disabled_config_never_arms_and_never_reads_the_cursor() {
    let reads = Arc::new(AtomicUsize::new(0));
    let counter = {
        let reads = Arc::clone(&reads);
        move || {
            reads.fetch_add(1, Ordering::Relaxed);
            Some((0, 0))
        }
    };
    let mut pie = PieRuntime::with_fetch(counter);
    pie.set_config(super::PieConfig {
        enabled: false,
        ..enabled_config()
    });
    pie.set_bindings(vec![key_gesture(GestureDirection::Up)], HashMap::new());
    assert!(!pie.should_open());
    pie.on_press();
    pie.on_tick();
    assert_eq!(pie.on_release(), None);
    assert_eq!(
        reads.load(Ordering::Relaxed),
        0,
        "a disabled pie must not query the cursor"
    );
}

#[test]
fn without_bindings_should_open_is_false() {
    let (mut pie, _rx) = scripted(&[(0, 0)], Vec::new());
    assert!(!pie.should_open());
    pie.on_press();
    assert!(!pie.armed);
}

#[test]
fn click_only_binding_does_not_arm_the_pie() {
    let (mut pie, _rx) = scripted(&[(0, 0)], vec![key_gesture(GestureDirection::None)]);
    assert!(!pie.should_open(), "a lone click binding is not a sector");
    pie.on_press();
    assert!(!pie.armed);
}

#[test]
fn cancel_closes_an_open_pie_and_resets() {
    let (mut pie, mut rx) = scripted(&[(0, 0), (100, 0)], vec![key_gesture(GestureDirection::Up)]);
    pie.on_press();
    pie.on_tick();
    pie.on_cancel();
    assert_eq!(
        drain(&mut rx).last(),
        Some(&PieEvent::Close {
            committed: None
        })
    );
    assert!(!pie.armed && !pie.open);
    assert_eq!(pie.on_release(), None);
    assert!(
        drain(&mut rx).is_empty(),
        "the settle after cancel is silent"
    );
}

#[test]
fn cancel_while_idle_emits_nothing() {
    let (mut pie, mut rx) = scripted(&[(0, 0)], vec![key_gesture(GestureDirection::Up)]);
    pie.on_cancel();
    assert!(matches!(
        rx.try_recv(),
        Err(TryRecvError::Empty | TryRecvError::Closed)
    ));
}

#[test]
fn press_without_cursor_keeps_the_hold_a_plain_swipe() {
    let mut pie = PieRuntime::with_fetch(|| None);
    pie.set_config(enabled_config());
    pie.set_bindings(vec![key_gesture(GestureDirection::Up)], HashMap::new());
    pie.on_press();
    pie.on_tick();
    assert!(!pie.armed && !pie.open);
    assert_eq!(pie.on_release(), None);
}

#[test]
fn profile_overrides_replace_global_sectors() {
    let mut pie = PieRuntime::with_fetch(|| Some((0, 0)));
    pie.set_config(enabled_config());
    pie.set_bindings(
        vec![
            key_gesture(GestureDirection::Up),
            key_gesture(GestureDirection::Right),
        ],
        HashMap::from([(
            "firefox".to_string(),
            vec![key_gesture(GestureDirection::Up)]
        )])
    );
    let focus = pie.focus.clone();
    *focus
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some("firefox".to_string());
    assert!(pie.should_open());
    assert_eq!(
        pie.bound_sectors(),
        vec![
            PieSector {
                direction: GestureDirection::Right,
                label:     "Right".to_string()
            },
            PieSector {
                direction: GestureDirection::Up,
                label:     "Up".to_string()
            },
        ],
        "the profile entry replaces the global binding for its direction"
    );
    *focus
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some("other".to_string());
    assert_eq!(
        pie.bound_sectors(),
        vec![
            PieSector {
                direction: GestureDirection::Up,
                label:     "Up".to_string()
            },
            PieSector {
                direction: GestureDirection::Right,
                label:     "Right".to_string()
            },
        ],
        "an unfocused profile falls through to the global set"
    );
}
