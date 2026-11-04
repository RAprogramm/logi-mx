// SPDX-FileCopyrightText: 2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Unit tests for the pure renderer state: monitor mapping, wedge tiling
//! against the daemon's sector centres, label truncation and the scene
//! state machine.

use logi_mx_daemon::pie::{PieEvent, PieSector, direction_angle};
use logi_mx_driver::devices::GestureDirection;

use super::{MonitorGeom, PieScene, anchor_monitor, logical_anchor, truncate_label, wedge_for};

/// A scale-2 primary (1920×1080 logical) plus a scale-1 secondary placed
/// to its right, mirroring the maintainer's live layout.
fn monitors() -> Vec<MonitorGeom> {
    vec![
        MonitorGeom {
            x:      0,
            y:      0,
            width:  1920,
            height: 1080,
            scale:  2
        },
        MonitorGeom {
            x:      1920,
            y:      0,
            width:  2560,
            height: 1440,
            scale:  1
        },
    ]
}

#[test]
fn anchor_maps_to_monitor_and_local_coordinates() {
    let monitors = monitors();
    let index = anchor_monitor((100, 200), &monitors).unwrap_or(usize::MAX);
    assert_eq!(index, 0);
    assert_eq!(logical_anchor((100, 200), &monitors[index]), (100.0, 200.0));

    let second = anchor_monitor((2000, 700), &monitors).unwrap_or(usize::MAX);
    assert_eq!(second, 1);
    assert_eq!(
        logical_anchor((2000, 700), &monitors[second]),
        (80.0, 700.0)
    );
}

#[test]
fn anchor_outside_every_monitor_is_none() {
    let monitors = monitors();
    assert_eq!(anchor_monitor((-1, 0), &monitors), None);
    assert_eq!(anchor_monitor((0, -1), &monitors), None);
    assert_eq!(anchor_monitor((4480, 0), &monitors), None);
    assert_eq!(anchor_monitor((0, 1080), &monitors), None);
}

#[test]
fn monitor_edges_are_half_open() {
    let monitors = monitors();
    assert_eq!(anchor_monitor((1920, 0), &monitors), Some(1));
    assert_eq!(anchor_monitor((1919, 1079), &monitors), Some(0));
}

#[test]
fn wedges_tile_the_full_circle_without_gaps() {
    let cardinals = [
        GestureDirection::Right,
        GestureDirection::Down,
        GestureDirection::Left,
        GestureDirection::Up
    ];
    let quarter = std::f64::consts::TAU / 4.0;
    let mut covered = 0.0;
    for direction in cardinals {
        let (start, end) = wedge_for(direction, 4).unwrap_or((f64::NAN, f64::NAN));
        let centre = direction_angle(direction).unwrap_or(f64::NAN);
        assert!(
            (end - start - quarter).abs() < 1e-12,
            "each of four sectors spans a quarter turn"
        );
        assert!(
            (f64::midpoint(start, end) - centre).abs() < 1e-12,
            "the wedge of {direction:?} must be centred on its angle"
        );
        covered += end - start;
    }
    assert!(
        (covered - std::f64::consts::TAU).abs() < 1e-12,
        "four wedges must cover the whole circle"
    );
}

#[test]
fn eight_sectors_reproduce_the_octants() {
    let all = [
        GestureDirection::Right,
        GestureDirection::DownRight,
        GestureDirection::Down,
        GestureDirection::DownLeft,
        GestureDirection::Left,
        GestureDirection::UpLeft,
        GestureDirection::Up,
        GestureDirection::UpRight
    ];
    let octant = std::f64::consts::TAU / 8.0;
    for direction in all {
        let (start, end) = wedge_for(direction, 8).unwrap_or((f64::NAN, f64::NAN));
        assert!(
            (end - start - octant).abs() < 1e-12,
            "each of eight sectors spans an octant"
        );
    }
}

#[test]
fn wedge_for_click_direction_is_none() {
    assert_eq!(wedge_for(GestureDirection::None, 4), None);
}

#[test]
fn labels_truncate_only_when_too_long() {
    assert_eq!(truncate_label("DownRight", 12), "DownRight");
    assert_eq!(truncate_label("DownRight", 9), "DownRight");
    assert_eq!(truncate_label("DownRight", 6), "DownR…");
    assert_eq!(truncate_label("Up", 0), "");
    assert_eq!(truncate_label("", 5), "");
}

#[test]
fn labels_truncate_by_characters_not_bytes() {
    assert_eq!(truncate_label("→→→→→→→", 4), "→→→…");
}

#[test]
fn scene_charge_open_highlight_close_cycle() {
    let monitors = monitors();
    let mut scene = PieScene::default();
    scene.apply(
        &PieEvent::Charge {
            anchor: (100, 200)
        },
        &monitors
    );
    assert!(scene.is_active());
    assert!(scene.charging);
    assert_eq!(scene.anchor, (100.0, 200.0));
    scene.apply(
        &PieEvent::Highlight {
            direction: Some(GestureDirection::Up)
        },
        &monitors
    );
    assert_eq!(
        scene.highlight, None,
        "a highlight cannot select a sector while charging"
    );

    scene.apply(
        &PieEvent::Open {
            anchor:  (100, 200),
            sectors: vec![
                PieSector {
                    direction: GestureDirection::Up,
                    label:     "Up".to_string()
                },
                PieSector {
                    direction: GestureDirection::DownLeft,
                    label:     "DownLeft".to_string()
                },
            ]
        },
        &monitors
    );
    assert!(scene.is_active());
    assert!(!scene.charging);
    assert_eq!(scene.monitor, 0);
    assert_eq!(scene.sectors.len(), 2);
    assert_eq!(scene.highlight, None);

    scene.apply(
        &PieEvent::Highlight {
            direction: Some(GestureDirection::DownLeft)
        },
        &monitors
    );
    assert_eq!(scene.highlight, Some(GestureDirection::DownLeft));

    scene.apply(
        &PieEvent::Close {
            committed: Some(GestureDirection::DownLeft)
        },
        &monitors
    );
    assert!(!scene.is_active());
    assert_eq!(scene.sectors, Vec::new());
}

#[test]
fn aim_updates_only_while_the_menu_is_open() {
    let monitors = monitors();
    let mut scene = PieScene::default();
    scene.apply(
        &PieEvent::Aim {
            angle: 1.0
        },
        &monitors
    );
    assert_eq!(scene.aim, None, "an aim without a menu is ignored");
    scene.apply(
        &PieEvent::Charge {
            anchor: (100, 200)
        },
        &monitors
    );
    scene.apply(
        &PieEvent::Aim {
            angle: 1.0
        },
        &monitors
    );
    assert_eq!(scene.aim, None, "a charge ring has nothing to aim at");
    scene.apply(
        &PieEvent::Open {
            anchor:  (100, 200),
            sectors: vec![PieSector {
                direction: GestureDirection::Up,
                label:     "Up".to_string()
            }]
        },
        &monitors
    );
    scene.apply(
        &PieEvent::Aim {
            angle: 1.5
        },
        &monitors
    );
    assert_eq!(scene.aim, Some(1.5), "the open menu tracks the exact aim");
    scene.apply(
        &PieEvent::Close {
            committed: None
        },
        &monitors
    );
    assert_eq!(scene.aim, None, "closing forgets the aim");
}

#[test]
fn scene_ignores_events_without_a_charge() {
    let monitors = monitors();
    let mut scene = PieScene::default();
    scene.apply(
        &PieEvent::Highlight {
            direction: Some(GestureDirection::Up)
        },
        &monitors
    );
    assert!(!scene.is_active());
    assert_eq!(scene.highlight, None);
    scene.apply(
        &PieEvent::Close {
            committed: Some(GestureDirection::Up)
        },
        &monitors
    );
    assert!(!scene.is_active());
}

#[test]
fn scene_open_outside_all_monitors_stays_inactive() {
    let monitors = monitors();
    let mut scene = PieScene::default();
    scene.apply(
        &PieEvent::Open {
            anchor:  (9999, 9999),
            sectors: vec![PieSector {
                direction: GestureDirection::Up,
                label:     "Up".to_string()
            }]
        },
        &monitors
    );
    assert!(!scene.is_active());
    assert_eq!(scene.sectors, Vec::new());
}
