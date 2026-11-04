// SPDX-FileCopyrightText: 2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Pure tests for the pie state machine and sector selection: no sockets,
//! no sleeping, scripted cursor readers and explicit clocks only.

use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering}
    },
    time::{Duration, Instant}
};

use logi_mx_driver::prelude::{Action, Gesture, GestureDirection, GestureMode};
use tokio::sync::broadcast::{Receiver, error::TryRecvError};

use super::{
    DOUBLE_CLICK_WINDOW, HOLD_DELAY, PieEvent, PieRelease, PieRuntime, PieSector, direction_angle,
    sector_among
};

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

/// Advances a clock by whole milliseconds past a fixed origin.
fn ms(origin: Instant, millis: u64) -> Instant {
    origin + Duration::from_millis(millis)
}

fn sector(direction: GestureDirection) -> PieSector {
    PieSector {
        direction,
        label: format!("{direction:?}")
    }
}

#[test]
fn direction_angle_places_cardinals_and_rejects_click() {
    assert_eq!(direction_angle(GestureDirection::Right), Some(0.0));
    assert!(
        (direction_angle(GestureDirection::Down).unwrap_or(0.0) - std::f64::consts::FRAC_PI_2)
            .abs()
            < 1e-12
    );
    assert!(
        (direction_angle(GestureDirection::Left).unwrap_or(0.0) - std::f64::consts::PI).abs()
            < 1e-12
    );
    let up = direction_angle(GestureDirection::Up).unwrap_or(0.0);
    let expected = 3.0 * std::f64::consts::FRAC_PI_2;
    assert!((up - expected).abs() < 1e-12);
    assert_eq!(direction_angle(GestureDirection::None), None);
}

#[test]
fn sector_among_maps_vectors_to_nearest_centre() {
    let eight = vec![
        sector(GestureDirection::Right),
        sector(GestureDirection::DownRight),
        sector(GestureDirection::Down),
        sector(GestureDirection::DownLeft),
        sector(GestureDirection::Left),
        sector(GestureDirection::UpLeft),
        sector(GestureDirection::Up),
        sector(GestureDirection::UpRight),
    ];
    assert_eq!(sector_among(10, 0, &eight), Some(GestureDirection::Right));
    assert_eq!(
        sector_among(10, 10, &eight),
        Some(GestureDirection::DownRight)
    );
    assert_eq!(sector_among(0, 10, &eight), Some(GestureDirection::Down));
    assert_eq!(
        sector_among(-10, 10, &eight),
        Some(GestureDirection::DownLeft)
    );
    assert_eq!(sector_among(-10, 0, &eight), Some(GestureDirection::Left));
    assert_eq!(
        sector_among(-10, -10, &eight),
        Some(GestureDirection::UpLeft)
    );
    assert_eq!(sector_among(0, -10, &eight), Some(GestureDirection::Up));
    assert_eq!(
        sector_among(10, -10, &eight),
        Some(GestureDirection::UpRight)
    );
}

#[test]
fn sector_among_tiles_cardinals_without_gaps() {
    let cardinals = vec![
        sector(GestureDirection::Up),
        sector(GestureDirection::Right),
        sector(GestureDirection::Down),
        sector(GestureDirection::Left),
    ];
    assert_eq!(
        sector_among(50, -49, &cardinals),
        Some(GestureDirection::Right)
    );
    assert_eq!(
        sector_among(49, -50, &cardinals),
        Some(GestureDirection::Up)
    );
    assert_eq!(
        sector_among(-1, -100, &cardinals),
        Some(GestureDirection::Up)
    );
    assert_eq!(
        sector_among(-100, 1, &cardinals),
        Some(GestureDirection::Left)
    );
}

#[test]
fn sector_among_breaks_ties_towards_the_first_sector() {
    let cardinals = vec![
        sector(GestureDirection::Up),
        sector(GestureDirection::Right),
    ];
    assert_eq!(
        sector_among(50, -50, &cardinals),
        Some(GestureDirection::Up),
        "an exact boundary belongs to the earlier sector"
    );
}

#[test]
fn sector_among_rejects_zero_vectors_and_empty_lists() {
    let cardinals = vec![sector(GestureDirection::Up)];
    assert_eq!(sector_among(0, 0, &cardinals), None);
    assert_eq!(sector_among(10, 0, &[]), None);
}

#[test]
fn press_charges_and_a_held_button_opens_after_the_delay() {
    let (mut pie, mut rx) = scripted(&[(100, 100)], vec![key_gesture(GestureDirection::Up)]);
    let t0 = Instant::now();
    assert!(pie.should_open());
    pie.on_press(t0);
    assert_eq!(
        drain(&mut rx),
        vec![PieEvent::Charge {
            anchor: (100, 100)
        }]
    );
    pie.on_tick(ms(t0, 100));
    assert!(
        drain(&mut rx).is_empty(),
        "a tick inside the hold gate keeps charging"
    );
    pie.on_tick(t0 + HOLD_DELAY);
    assert_eq!(
        drain(&mut rx),
        vec![PieEvent::Open {
            anchor:  (100, 100),
            sectors: vec![sector(GestureDirection::Up)]
        }]
    );
    assert!(pie.is_open());
}

#[test]
fn tap_closes_the_charge_and_reports_charging() {
    let (mut pie, mut rx) = scripted(&[(100, 100)], vec![key_gesture(GestureDirection::Up)]);
    let t0 = Instant::now();
    pie.on_press(t0);
    pie.on_tick(ms(t0, 50));
    assert_eq!(pie.on_release(ms(t0, 100)), PieRelease::Charging);
    assert_eq!(
        drain(&mut rx),
        vec![
            PieEvent::Charge {
                anchor: (100, 100)
            },
            PieEvent::Close {
                committed: None
            },
        ]
    );
    assert!(!pie.is_open());
}

#[test]
fn double_click_opens_without_charging_again() {
    let (mut pie, mut rx) = scripted(
        &[(100, 100), (100, 100)],
        vec![key_gesture(GestureDirection::Up)]
    );
    let t0 = Instant::now();
    pie.on_press(t0);
    assert_eq!(pie.on_release(ms(t0, 100)), PieRelease::Charging);
    drain(&mut rx);
    let second = ms(t0, 100) + DOUBLE_CLICK_WINDOW / 2;
    pie.on_press(second);
    assert!(pie.is_open(), "the second press opens the menu at once");
    assert_eq!(
        drain(&mut rx),
        vec![PieEvent::Open {
            anchor:  (100, 100),
            sectors: vec![sector(GestureDirection::Up)]
        }]
    );
}

#[test]
fn a_press_after_the_window_charges_again() {
    let (mut pie, mut rx) = scripted(&[(0, 0), (0, 0)], vec![key_gesture(GestureDirection::Up)]);
    let t0 = Instant::now();
    pie.on_press(t0);
    pie.on_release(ms(t0, 50));
    drain(&mut rx);
    let late = ms(t0, 50) + DOUBLE_CLICK_WINDOW * 2;
    pie.on_press(late);
    assert!(
        !pie.is_open(),
        "a stale tap no longer counts as a double click"
    );
    assert_eq!(
        drain(&mut rx),
        vec![PieEvent::Charge {
            anchor: (0, 0)
        }]
    );
}

#[test]
fn open_and_highlight_fire_only_on_change() {
    let bindings = vec![
        key_gesture(GestureDirection::Up),
        key_gesture(GestureDirection::Right),
    ];
    let (mut pie, mut rx) = scripted(&[(0, 0), (100, 0), (100, 0), (0, -100)], bindings);
    let t0 = Instant::now();
    pie.on_press(t0);
    pie.on_tick(ms(t0, 300));
    pie.on_tick(ms(t0, 310));
    assert!(pie.is_open());
    assert_eq!(
        drain(&mut rx),
        vec![
            PieEvent::Charge {
                anchor: (0, 0)
            },
            PieEvent::Open {
                anchor:  (0, 0),
                sectors: vec![
                    sector(GestureDirection::Up),
                    sector(GestureDirection::Right),
                ]
            },
            PieEvent::Aim {
                angle: 0.0
            },
            PieEvent::Highlight {
                direction: Some(GestureDirection::Right)
            }
        ],
        "the second identical tick must not repeat Charge, Open, Aim or Highlight"
    );
    pie.on_tick(ms(t0, 320));
    pie.on_tick(ms(t0, 330));
    let events = drain(&mut rx);
    assert_eq!(events.len(), 2, "one aim step and one highlight change");
    let expected = -std::f64::consts::FRAC_PI_2;
    assert!(
        matches!(events[0], PieEvent::Aim { angle } if (angle - expected).abs() < 1e-9),
        "the needle follows the cursor to the up direction"
    );
    assert_eq!(
        events[1],
        PieEvent::Highlight {
            direction: Some(GestureDirection::Up)
        }
    );
}

#[test]
fn aim_follows_the_cursor_and_ignores_tiny_moves() {
    let (mut pie, mut rx) = scripted(
        &[(0, 0), (100, 0), (100, 3), (100, 20)],
        vec![key_gesture(GestureDirection::Up)]
    );
    let t0 = Instant::now();
    pie.on_press(t0);
    pie.on_tick(ms(t0, 300));
    pie.on_tick(ms(t0, 310));
    drain(&mut rx);
    pie.on_tick(ms(t0, 320));
    assert!(
        drain(&mut rx).is_empty(),
        "a cursor move under the epsilon keeps the stream silent"
    );
    pie.on_tick(ms(t0, 330));
    let events = drain(&mut rx);
    assert_eq!(events.len(), 1, "a real move republishes the needle");
    assert!(
        matches!(events[0], PieEvent::Aim { angle } if (angle - 0.197).abs() < 0.01),
        "the needle carries the exact cursor angle"
    );
}

#[test]
fn release_over_bound_sector_commits_and_closes() {
    let (mut pie, mut rx) = scripted(
        &[(0, 0), (0, -100)],
        vec![key_gesture(GestureDirection::Up)]
    );
    let t0 = Instant::now();
    pie.on_press(t0);
    pie.on_tick(ms(t0, 300));
    pie.on_tick(ms(t0, 310));
    assert_eq!(
        pie.on_release(ms(t0, 400)),
        PieRelease::Opened(Some(GestureDirection::Up))
    );
    assert_eq!(
        drain(&mut rx).last(),
        Some(&PieEvent::Close {
            committed: Some(GestureDirection::Up)
        })
    );
    assert!(!pie.is_open(), "the hold must be settled after release");
}

#[test]
fn swipe_highlights_and_release_commits() {
    let (mut pie, mut rx) = scripted(&[(0, 0)], vec![key_gesture(GestureDirection::Up)]);
    let t0 = Instant::now();
    pie.on_press(t0);
    pie.on_tick(ms(t0, 300));
    pie.on_swipe(GestureDirection::Up);
    assert_eq!(
        drain(&mut rx).last(),
        Some(&PieEvent::Highlight {
            direction: Some(GestureDirection::Up)
        })
    );
    pie.on_swipe(GestureDirection::Up);
    assert!(
        drain(&mut rx).is_empty(),
        "the same direction must not repeat Highlight"
    );
    assert_eq!(
        pie.on_release(ms(t0, 500)),
        PieRelease::Opened(Some(GestureDirection::Up))
    );
    assert_eq!(
        drain(&mut rx).last(),
        Some(&PieEvent::Close {
            committed: Some(GestureDirection::Up)
        })
    );
}

#[test]
fn swipe_highlight_survives_a_still_cursor() {
    let (mut pie, mut rx) = scripted(&[(0, 0)], vec![key_gesture(GestureDirection::Up)]);
    let t0 = Instant::now();
    pie.on_press(t0);
    pie.on_tick(ms(t0, 300));
    pie.on_swipe(GestureDirection::Up);
    drain(&mut rx);
    pie.on_tick(ms(t0, 310));
    pie.on_tick(ms(t0, 320));
    assert!(
        drain(&mut rx).is_empty(),
        "a cursor inside the gate must not clear the swipe highlight"
    );
}

#[test]
fn swipe_over_unbound_direction_closes_without_commit() {
    let (mut pie, mut rx) = scripted(&[(0, 0)], vec![key_gesture(GestureDirection::Up)]);
    let t0 = Instant::now();
    pie.on_press(t0);
    pie.on_tick(ms(t0, 300));
    pie.on_swipe(GestureDirection::Left);
    assert_eq!(pie.on_release(ms(t0, 400)), PieRelease::Opened(None));
    assert_eq!(
        drain(&mut rx).last(),
        Some(&PieEvent::Close {
            committed: None
        })
    );
}

#[test]
fn cursor_inside_the_gate_leaves_the_selection_alone() {
    let (mut pie, mut rx) = scripted(&[(0, 0), (20, 0)], vec![key_gesture(GestureDirection::Up)]);
    let t0 = Instant::now();
    pie.on_press(t0);
    pie.on_tick(ms(t0, 300));
    pie.on_tick(ms(t0, 310));
    assert!(
        drain(&mut rx)
            .iter()
            .all(|event| !matches!(event, PieEvent::Highlight { .. })),
        "travel inside the activation radius must not select a sector"
    );
}

#[test]
fn disabled_config_never_engages_and_never_reads_the_cursor() {
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
    let t0 = Instant::now();
    assert!(!pie.should_open());
    pie.on_press(t0);
    pie.on_tick(ms(t0, 300));
    assert_eq!(pie.on_release(ms(t0, 400)), PieRelease::Idle);
    assert_eq!(
        reads.load(Ordering::Relaxed),
        0,
        "a disabled pie must not query the cursor"
    );
}

#[test]
fn without_bindings_should_open_is_false() {
    let (mut pie, _rx) = scripted(&[(0, 0)], Vec::new());
    let t0 = Instant::now();
    assert!(!pie.should_open());
    pie.on_press(t0);
    assert!(!pie.is_open());
    assert_eq!(pie.on_release(ms(t0, 400)), PieRelease::Idle);
}

#[test]
fn click_only_binding_does_not_open_the_pie() {
    let (mut pie, _rx) = scripted(&[(0, 0)], vec![key_gesture(GestureDirection::None)]);
    let t0 = Instant::now();
    assert!(!pie.should_open(), "a lone click binding is not a sector");
    pie.on_press(t0);
    assert!(!pie.is_open());
    assert_eq!(pie.on_release(ms(t0, 400)), PieRelease::Idle);
}

#[test]
fn cancel_closes_a_charging_ring() {
    let (mut pie, mut rx) = scripted(&[(0, 0)], vec![key_gesture(GestureDirection::Up)]);
    let t0 = Instant::now();
    pie.on_press(t0);
    pie.on_cancel();
    assert_eq!(
        drain(&mut rx).last(),
        Some(&PieEvent::Close {
            committed: None
        })
    );
    assert!(!pie.is_open());
    assert_eq!(pie.on_release(ms(t0, 100)), PieRelease::Idle);
    assert!(
        drain(&mut rx).is_empty(),
        "the settle after cancel is silent"
    );
}

#[test]
fn cancel_closes_an_open_pie_and_resets() {
    let (mut pie, mut rx) = scripted(&[(0, 0)], vec![key_gesture(GestureDirection::Up)]);
    let t0 = Instant::now();
    pie.on_press(t0);
    pie.on_tick(ms(t0, 300));
    pie.on_cancel();
    assert_eq!(
        drain(&mut rx).last(),
        Some(&PieEvent::Close {
            committed: None
        })
    );
    assert!(!pie.is_open());
    assert_eq!(pie.on_release(ms(t0, 400)), PieRelease::Idle);
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
    let t0 = Instant::now();
    pie.on_press(t0);
    pie.on_tick(ms(t0, 300));
    assert!(!pie.is_open());
    assert_eq!(pie.on_release(ms(t0, 400)), PieRelease::Idle);
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
            sector(GestureDirection::Right),
            sector(GestureDirection::Up),
        ],
        "the profile entry replaces the global binding for its direction"
    );
    *focus
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some("other".to_string());
    assert_eq!(
        pie.bound_sectors(),
        vec![
            sector(GestureDirection::Up),
            sector(GestureDirection::Right),
        ],
        "an unfocused profile falls through to the global set"
    );
}
