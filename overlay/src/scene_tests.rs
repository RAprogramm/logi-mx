// SPDX-FileCopyrightText: 2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Unit tests for the pure renderer state: monitor mapping, label
//! truncation and the scene state machine.

use logi_mx_daemon::pie::{PieEvent, labels::PieSector};

use super::{MonitorGeom, PieScene, anchor_monitor, logical_anchor, truncate_label};

/// One wedge of a streamed ring, addressed by its index.
fn sector(index: u8, label: &str, bound: bool) -> PieSector {
    PieSector {
        index,
        label: label.to_string(),
        command: None,
        bound
    }
}

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
            index: Some(0)
        },
        &monitors
    );
    assert_eq!(
        scene.highlight, None,
        "a highlight cannot select a wedge while charging"
    );

    scene.apply(
        &PieEvent::Open {
            anchor:  (100, 200),
            sectors: vec![sector(0, "Meta+E", true), sector(1, "", false)]
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
            index: Some(1)
        },
        &monitors
    );
    assert_eq!(scene.highlight, Some(1), "the empty wedge highlights too");

    scene.apply(
        &PieEvent::Close {
            committed: Some(0)
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
            sectors: vec![sector(0, "Meta+E", true), sector(1, "E", true)]
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
            index: Some(0)
        },
        &monitors
    );
    assert!(!scene.is_active());
    assert_eq!(scene.highlight, None);
    scene.apply(
        &PieEvent::Close {
            committed: Some(0)
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
            sectors: vec![sector(0, "Meta+E", true), sector(1, "E", true)]
        },
        &monitors
    );
    assert!(!scene.is_active());
    assert_eq!(scene.sectors, Vec::new());
}
