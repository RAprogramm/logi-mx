// SPDX-FileCopyrightText: 2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! State machine tests: charge/bloom/tap/double-click, wedge selection by
//! cursor and swipe, commit honesty, the expert-mark cancellation, per-app
//! rings and the late-subscriber replay.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant}
};

use logi_mx_driver::{
    config::{PieConfig, PieProfile, PieSlot},
    prelude::{Action, GestureDirection}
};
use tokio::sync::broadcast::Receiver;

use super::{DOUBLE_CLICK_WINDOW, PieEvent, PieHub, PieRelease, PieRuntime};
use crate::pie::labels::sectors_view;

/// A ring of `count` key-chord slots, labelled `K0..Kn` by position.
fn ring(count: usize) -> Vec<PieSlot> {
    (0..count)
        .map(|index| PieSlot {
            action: Some(Action::Keypress {
                keys: vec![format!("KEY_{index}")]
            }),
            label:  None
        })
        .collect()
}

fn enabled_ring_config(count: usize) -> PieConfig {
    PieConfig {
        enabled:           true,
        activation_radius: 40,
        dead_zone:         12,
        sectors:           ring(count)
    }
}

/// Builds an enabled runtime whose cursor reader replays `positions`:
/// every call consumes the next entry and repeats the last one once the
/// script is spent.
fn scripted(positions: &[(i32, i32)], count: usize) -> (PieRuntime, Receiver<PieEvent>) {
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
    let hub = PieHub::new(64);
    let rx = hub.subscribe();
    runtime.attach_hub(hub);
    runtime.set_config(enabled_ring_config(count));
    (runtime, rx)
}

fn drain(rx: &mut Receiver<PieEvent>) -> Vec<PieEvent> {
    let mut events = Vec::new();
    while let Ok(event) = rx.try_recv() {
        events.push(event);
    }
    events
}

fn ms(origin: Instant, millis: u64) -> Instant {
    origin + Duration::from_millis(millis)
}

#[test]
fn an_empty_or_tiny_ring_never_opens() {
    let (mut pie, mut rx) = scripted(&[(0, 0)], 0);
    assert!(!pie.should_open(), "no wedges, no menu");
    pie.set_config(enabled_ring_config(1));
    assert!(!pie.should_open(), "one wedge is nothing to choose from");
    pie.on_press(Instant::now());
    assert!(
        drain(&mut rx).is_empty(),
        "a press without a ring stays silent"
    );
}

#[test]
fn charge_blooms_after_the_hold_gate() {
    let (mut pie, mut rx) = scripted(&[(0, 0)], 4);
    let t0 = Instant::now();
    pie.on_press(t0);
    assert_eq!(
        drain(&mut rx),
        vec![PieEvent::Charge {
            anchor: (0, 0)
        }]
    );
    pie.on_tick(ms(t0, 100));
    assert!(
        drain(&mut rx).is_empty(),
        "below the gate the ring keeps charging"
    );
    pie.on_tick(ms(t0, 260));
    let events = drain(&mut rx);
    assert_eq!(
        events,
        vec![PieEvent::Open {
            anchor:  (0, 0),
            sectors: sectors_view(&ring(4))
        }]
    );
    assert!(pie.is_open());
}

#[test]
fn tap_defers_the_click_binding_and_double_click_opens_at_once() {
    let (mut pie, mut rx) = scripted(&[(0, 0)], 4);
    let t0 = Instant::now();
    pie.on_press(t0);
    assert_eq!(
        pie.on_release(ms(t0, 100)),
        PieRelease::Charging,
        "a quick tap is a click candidate"
    );
    assert_eq!(
        drain(&mut rx),
        vec![
            PieEvent::Charge {
                anchor: (0, 0)
            },
            PieEvent::Close {
                committed: None
            }
        ]
    );
    let t1 = ms(t0, 200);
    pie.on_press(t1);
    assert!(
        pie.is_open(),
        "a second press inside the window skips charging"
    );
    assert_eq!(
        drain(&mut rx).last(),
        Some(&PieEvent::Open {
            anchor:  (0, 0),
            sectors: sectors_view(&ring(4))
        })
    );
    assert!(
        matches!(pie.on_release(ms(t1, 300)), PieRelease::Committed(None)),
        "an unselected double-click release commits nothing"
    );
    let t2 = ms(
        t1,
        u64::try_from(DOUBLE_CLICK_WINDOW.as_millis()).unwrap() + 10
    );
    pie.on_press(t2);
    assert!(
        !pie.is_open(),
        "a press outside the double-click window charges again"
    );
}

#[test]
fn cursor_vectors_select_wedges_and_release_commits_the_action() {
    let (mut pie, mut rx) = scripted(&[(0, 0), (0, -100)], 4);
    let t0 = Instant::now();
    pie.on_press(t0);
    pie.on_tick(ms(t0, 300));
    pie.on_tick(ms(t0, 310));
    let events = drain(&mut rx);
    assert!(
        events.contains(&PieEvent::Highlight {
            index: Some(0)
        }),
        "straight up highlights slot 0: {events:?}"
    );
    assert_eq!(
        pie.on_release(ms(t0, 400)),
        PieRelease::Committed(Some(Action::Keypress {
            keys: vec!["KEY_0".to_string()]
        })),
        "the highlighted wedge's action is handed to the host"
    );
    assert_eq!(
        drain(&mut rx).last(),
        Some(&PieEvent::Close {
            committed: Some(0)
        })
    );
    assert!(!pie.is_open(), "the hold must be settled after release");
}

#[test]
fn swipe_directions_select_the_wedge_they_fall_into() {
    let (mut pie, mut rx) = scripted(&[(0, 0)], 4);
    let t0 = Instant::now();
    pie.on_press(t0);
    pie.on_tick(ms(t0, 300));
    drain(&mut rx);
    pie.on_swipe(GestureDirection::Right);
    assert_eq!(
        drain(&mut rx),
        vec![
            PieEvent::Aim {
                angle: 0.0
            },
            PieEvent::Highlight {
                index: Some(1)
            }
        ],
        "a right swipe lands on the east wedge of the four-ring"
    );
    pie.on_swipe(GestureDirection::Up);
    assert_eq!(
        drain(&mut rx).last(),
        Some(&PieEvent::Highlight {
            index: Some(0)
        })
    );
}

#[test]
fn empty_wedges_highlight_but_commit_nothing() {
    let mut slots = ring(4);
    slots[2].action = None;
    let (mut pie, mut rx) = scripted(&[(0, 0), (0, 100)], 4);
    pie.set_config(PieConfig {
        sectors: slots,
        ..enabled_ring_config(4)
    });
    let t0 = Instant::now();
    pie.on_press(t0);
    pie.on_tick(ms(t0, 300));
    pie.on_tick(ms(t0, 310));
    assert_eq!(
        pie.on_release(ms(t0, 400)),
        PieRelease::Committed(None),
        "releasing over an empty wedge fires nothing"
    );
    assert_eq!(
        drain(&mut rx).last(),
        Some(&PieEvent::Close {
            committed: None
        }),
        "nothing fired, so the close reports no committed wedge"
    );
}

#[test]
fn release_past_the_gate_promotes_a_charging_hold() {
    let (mut pie, mut rx) = scripted(&[(0, 0)], 4);
    let t0 = Instant::now();
    pie.on_press(t0);
    assert!(
        matches!(pie.on_release(ms(t0, 260)), PieRelease::Committed(None)),
        "a release past the hold deadline belongs to the open menu, not a tap"
    );
    let events = drain(&mut rx);
    assert!(
        events.contains(&PieEvent::Open {
            anchor:  (0, 0),
            sectors: sectors_view(&ring(4))
        }),
        "the promoted open must still be advertised: {events:?}"
    );
    assert_eq!(
        events.last(),
        Some(&PieEvent::Close {
            committed: None
        }),
        "no wedge was highlighted, so nothing commits"
    );
}

#[test]
fn expert_mark_cancels_the_menu_for_the_hold() {
    let (mut pie, mut rx) = scripted(&[(0, 0)], 4);
    let t0 = Instant::now();
    pie.on_press(t0);
    drain(&mut rx);
    pie.on_cancel();
    assert_eq!(
        pie.on_release(ms(t0, 400)),
        PieRelease::Idle,
        "after a mark the host settles the hold as a plain swipe"
    );
    assert_eq!(
        drain(&mut rx),
        vec![PieEvent::Close {
            committed: None
        }],
        "the charge ring closes exactly once and never blooms"
    );
}

#[test]
fn the_ring_snapshots_at_press_and_survives_config_edits() {
    let (mut pie, _rx) = scripted(&[(0, 0), (0, -100)], 4);
    let t0 = Instant::now();
    pie.on_press(t0);
    pie.on_tick(ms(t0, 300));
    pie.set_config(PieConfig {
        sectors: ring(2),
        ..enabled_ring_config(2)
    });
    pie.on_tick(ms(t0, 310));
    assert_eq!(
        pie.on_release(ms(t0, 400)),
        PieRelease::Committed(Some(Action::Keypress {
            keys: vec!["KEY_0".to_string()]
        })),
        "the wedge advertised at press commits even after the ring was edited"
    );
}

#[test]
fn focused_app_profile_replaces_the_global_ring() {
    let (mut pie, _rx) = scripted(&[(0, 0)], 4);
    pie.set_pie_profiles(HashMap::from([(
        "minecraft".to_string(),
        PieProfile {
            sectors: ring(6)
        }
    )]));
    assert_eq!(pie.resolve_ring().len(), 4, "global ring while unfocused");
    let focus = crate::focus::SharedFocus::default();
    pie.attach_focus(crate::focus::SharedFocus::clone(&focus));
    *focus
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some("minecraft".to_string());
    assert_eq!(
        pie.resolve_ring().len(),
        6,
        "the focused app carries its own wheel"
    );
}

#[test]
fn wheel_scrolling_cycles_the_open_ring() {
    let (mut pie, mut rx) = scripted(&[(0, 0)], 4);
    let t0 = Instant::now();
    pie.on_press(t0);
    pie.on_tick(ms(t0, 300));
    drain(&mut rx);
    pie.on_scroll(1);
    assert_eq!(
        drain(&mut rx).last(),
        Some(&PieEvent::Highlight {
            index: Some(0)
        }),
        "the first scroll enters the top wedge"
    );
    pie.on_scroll(1);
    assert_eq!(
        drain(&mut rx).last(),
        Some(&PieEvent::Highlight {
            index: Some(1)
        }),
        "scrolling runs clockwise"
    );
    pie.on_scroll(-2);
    assert_eq!(
        drain(&mut rx).last(),
        Some(&PieEvent::Highlight {
            index: Some(3)
        }),
        "scrolling wraps counter-clockwise"
    );
    assert_eq!(
        pie.on_release(ms(t0, 400)),
        PieRelease::Committed(Some(Action::Keypress {
            keys: vec!["KEY_3".to_string()]
        })),
        "the wedge the wheel stopped on commits"
    );
}

#[test]
fn wheel_scrolling_on_a_closed_pie_is_silent() {
    let (mut pie, mut rx) = scripted(&[(0, 0)], 4);
    pie.on_scroll(2);
    assert_eq!(drain(&mut rx), Vec::new(), "a closed pie ignores the wheel");
}

#[test]
fn hub_replays_the_latched_state_until_close() {
    let hub = PieHub::new(8);
    assert_eq!(hub.replay(), None, "an idle hub replays nothing");
    hub.send(PieEvent::Charge {
        anchor: (10, 20)
    });
    assert_eq!(
        hub.replay(),
        Some(PieEvent::Charge {
            anchor: (10, 20)
        })
    );
    hub.send(PieEvent::Highlight {
        index: Some(1)
    });
    assert!(
        matches!(hub.replay(), Some(PieEvent::Charge { .. })),
        "transient events must not replace the latch"
    );
    hub.send(PieEvent::Close {
        committed: None
    });
    assert_eq!(hub.replay(), None, "a closed hold replays nothing");
}

#[test]
fn a_missing_cursor_keeps_the_hold_a_plain_swipe() {
    let mut pie = PieRuntime::with_fetch(|| None);
    pie.set_config(enabled_ring_config(4));
    let t0 = Instant::now();
    pie.on_press(t0);
    assert!(!pie.is_open(), "no cursor, no anchor, no menu");
    assert_eq!(
        pie.on_release(t0 + Duration::from_millis(400)),
        PieRelease::Idle,
        "the hold stays a plain swipe for the host"
    );
}

#[test]
fn cancel_on_an_idle_runtime_stays_silent() {
    let (mut pie, mut rx) = scripted(&[(0, 0)], 4);
    pie.on_cancel();
    assert_eq!(drain(&mut rx), Vec::new());
}

#[test]
fn aim_events_are_suppressed_below_the_epsilon() {
    let (mut pie, mut rx) = scripted(&[(0, 0), (1, -100), (2, -100)], 4);
    let t0 = Instant::now();
    pie.on_press(t0);
    pie.on_tick(ms(t0, 300));
    pie.on_tick(ms(t0, 310));
    let aims = drain(&mut rx)
        .into_iter()
        .filter(|event| matches!(event, PieEvent::Aim { .. }))
        .count();
    assert_eq!(aims, 1, "a one-pixel drift must not republish the needle");
}
