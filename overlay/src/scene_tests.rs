// SPDX-FileCopyrightText: 2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Unit tests for the pure renderer state: monitor mapping, octant angles
//! against the daemon's sector boundaries, label truncation and the scene
//! state machine.

use logi_mx_daemon::pie::{PieEvent, PieSector, sector_for};
use logi_mx_driver::devices::GestureDirection;

use super::{MonitorGeom, PieScene, anchor_monitor, logical_anchor, truncate_label, wedge_for};

/// A scale-2 primary (3840×2160 physical) plus a scale-1 secondary placed
/// to its right, mirroring the maintainer's live layout.
fn monitors() -> Vec<MonitorGeom> {
    vec![
        MonitorGeom {
            x:      0,
            y:      0,
            width:  3840,
            height: 2160,
            scale:  2
        },
        MonitorGeom {
            x:      3840,
            y:      0,
            width:  2560,
            height: 1440,
            scale:  1
        },
    ]
}

#[test]
fn anchor_maps_to_monitor_and_logical_coordinates() {
    let monitors = monitors();
    let index = anchor_monitor((100, 200), &monitors).unwrap_or(usize::MAX);
    assert_eq!(index, 0);
    assert_eq!(logical_anchor((100, 200), &monitors[index]), (50.0, 100.0));

    let second = anchor_monitor((4000, 700), &monitors).unwrap_or(usize::MAX);
    assert_eq!(second, 1);
    assert_eq!(
        logical_anchor((4000, 700), &monitors[second]),
        (160.0, 700.0)
    );
}

#[test]
fn anchor_on_scale_one_monitor_maps_identity() {
    let monitors = [MonitorGeom {
        x:      1920,
        y:      0,
        width:  1920,
        height: 1080,
        scale:  1
    }];
    let index = anchor_monitor((2000, 500), &monitors).unwrap_or(usize::MAX);
    assert_eq!(index, 0);
    assert_eq!(logical_anchor((2000, 500), &monitors[index]), (80.0, 500.0));
}

#[test]
fn anchor_outside_every_monitor_is_none() {
    let monitors = monitors();
    assert_eq!(anchor_monitor((-1, 0), &monitors), None);
    assert_eq!(anchor_monitor((0, -1), &monitors), None);
    assert_eq!(anchor_monitor((6400, 0), &monitors), None);
    assert_eq!(anchor_monitor((0, 2160), &monitors), None);
}

#[test]
fn monitor_edges_are_half_open() {
    let monitors = monitors();
    assert_eq!(anchor_monitor((3840, 0), &monitors), Some(1));
    assert_eq!(anchor_monitor((3839, 2159), &monitors), Some(0));
}

/// The wedge table must reproduce the daemon's octants: the midpoint of
/// every wedge, and points well inside it, resolve to the same direction
/// through `sector_for`.
#[allow(
    clippy::cast_possible_truncation,
    reason = "vectors are bounded to ±100 after rounding"
)]
#[test]
fn wedge_table_matches_daemon_sector_boundaries() {
    let octants = [
        (GestureDirection::Right, 0.0_f64),
        (GestureDirection::DownRight, 45.0),
        (GestureDirection::Down, 90.0),
        (GestureDirection::DownLeft, 135.0),
        (GestureDirection::Left, 180.0),
        (GestureDirection::UpLeft, 225.0),
        (GestureDirection::Up, 270.0),
        (GestureDirection::UpRight, 315.0)
    ];
    for (direction, degrees) in octants {
        let middle = degrees.to_radians();
        let (start, end) = wedge_for(direction).unwrap_or((f64::NAN, f64::NAN));
        assert!(
            (middle - start - std::f64::consts::FRAC_PI_8).abs() < 1e-12
                && (end - middle - std::f64::consts::FRAC_PI_8).abs() < 1e-12,
            "wedge of {direction:?} must be centred on {degrees}°"
        );
        for offset in [-20.0_f64, 0.0, 20.0] {
            let angle = middle + offset.to_radians();
            let dx = (angle.cos() * 100.0).round() as i32;
            let dy = (angle.sin() * 100.0).round() as i32;
            assert_eq!(
                sector_for(dx, dy, 0),
                Some(direction),
                "vector at {}° must resolve to {direction:?}",
                angle.to_degrees()
            );
        }
    }
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "the boundary vector is bounded to 100 after rounding"
)]
#[test]
fn wedge_on_boundary_belongs_to_cardinal_like_daemon() {
    let (start, end) = wedge_for(GestureDirection::Right).unwrap_or((f64::NAN, f64::NAN));
    assert!(end.is_finite() && start.is_finite());
    let boundary = (end.tan() * 100.0).round() as i32;
    assert_eq!(sector_for(100, boundary, 0), Some(GestureDirection::Right));
}

#[test]
fn wedge_for_click_direction_is_none() {
    assert_eq!(wedge_for(GestureDirection::None), None);
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
fn scene_open_highlight_close_cycle() {
    let monitors = monitors();
    let mut scene = PieScene::default();
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
    assert_eq!(scene.monitor, 0);
    assert_eq!(scene.anchor, (50.0, 100.0));
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
fn scene_ignores_events_without_an_open() {
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
