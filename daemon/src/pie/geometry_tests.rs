// SPDX-FileCopyrightText: 2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Geometry tests: equal wedges tile the circle, vectors land in the right
//! wedge, and the gate comparison stays strict.

use std::f64::consts::{FRAC_PI_2, PI, TAU};

use logi_mx_driver::devices::GestureDirection;

use super::{
    beyond_radius, direction_angle, slot_for_angle, slot_index, wedge_angle, wedge_for,
    wrapped_distance
};

#[test]
fn wedges_tile_the_circle_for_every_legal_count() {
    for count in [2usize, 3, 4, 6, 8] {
        let mut covered = 0.0;
        for index in 0..count {
            let (start, end) = wedge_for(index, count);
            assert!(end > start, "wedge {index} runs forward");
            assert!(
                (end - start - TAU / f64::from(u32::try_from(count).unwrap())).abs() < 1e-9,
                "every wedge spans an equal share"
            );
            let centre = wedge_angle(index, count);
            assert!(
                (f64::midpoint(start, end) - centre).abs() < 1e-9,
                "the wedge is centred on its angle"
            );
            let next = wedge_for((index + 1) % count, count);
            let gap = (next.0 - end).rem_euclid(TAU);
            assert!(
                gap < 1e-9 || (gap - TAU).abs() < 1e-9,
                "wedges butt together without gaps for count {count}"
            );
            covered += end - start;
        }
        assert!(
            (covered - TAU).abs() < 1e-9,
            "count {count} covers the whole circle"
        );
    }
}

#[test]
fn slot_zero_points_at_twelve_oclock() {
    assert!((wedge_angle(0, 4) + FRAC_PI_2).abs() < 1e-9);
    assert_eq!(slot_index(0, -50, 4), Some(0), "straight up");
    assert_eq!(slot_index(0, -50, 8), Some(0), "up is slot 0 for any count");
}

#[test]
fn vectors_land_in_the_expected_wedge() {
    assert_eq!(slot_index(50, 0, 4), Some(1), "right");
    assert_eq!(slot_index(0, 50, 4), Some(2), "down");
    assert_eq!(slot_index(-50, 0, 4), Some(3), "left");
    assert_eq!(slot_index(40, 40, 8), Some(3), "down-right is octant 3");
    assert_eq!(slot_index(-40, -40, 8), Some(7), "up-left is octant 7");
    assert_eq!(
        slot_index(1, -100, 3),
        Some(0),
        "a hair off vertical still picks the top wedge of a three-ring"
    );
    assert_eq!(
        slot_index(100, 60, 3),
        Some(1),
        "lower right of a three-ring"
    );
    assert_eq!(
        slot_index(-100, 60, 3),
        Some(2),
        "lower left of a three-ring"
    );
}

#[test]
fn the_wraparound_seam_stays_inside_its_wedge() {
    assert_eq!(
        slot_index(40, -45, 4),
        Some(0),
        "41 deg right of up stays in the top wedge"
    );
    assert_eq!(
        slot_index(45, -40, 4),
        Some(1),
        "past 45 deg the east wedge owns it"
    );
    assert_eq!(
        slot_index(-40, -45, 4),
        Some(0),
        "41 deg left of up stays in the top wedge"
    );
    assert_eq!(
        slot_index(-45, -40, 4),
        Some(3),
        "past 45 deg left the west wedge owns it"
    );
    assert_eq!(
        slot_index(40, -45, 8),
        Some(1),
        "the same vector crosses into octant 1"
    );
    assert_eq!(
        slot_index(15, -45, 8),
        Some(0),
        "inside 22.5 deg it stays in octant 0"
    );
}

#[test]
fn zero_vector_and_empty_ring_select_nothing() {
    assert_eq!(slot_index(0, 0, 4), None);
    assert_eq!(slot_for_angle(0.0, 0), None);
}

#[test]
fn slot_for_angle_agrees_with_slot_index() {
    for count in [2usize, 3, 4, 6, 8] {
        for index in 0..count {
            let centre = wedge_angle(index, count);
            assert_eq!(slot_for_angle(centre, count), Some(index));
        }
    }
}

#[test]
fn compass_angles_keep_their_contract() {
    assert_eq!(direction_angle(GestureDirection::Right), Some(0.0));
    assert!((direction_angle(GestureDirection::Down).unwrap_or(0.0) - FRAC_PI_2).abs() < 1e-9);
    let up = direction_angle(GestureDirection::Up).unwrap_or(0.0);
    assert!(
        (up - (std::f64::consts::TAU - FRAC_PI_2)).abs() < 1e-9,
        "up is 3pi/2"
    );
    assert_eq!(direction_angle(GestureDirection::None), None);
}

#[test]
fn the_gate_is_strictly_outside_the_radius() {
    assert!(beyond_radius(41, 0, 40));
    assert!(
        !beyond_radius(40, 0, 40),
        "exactly on the radius stays inside"
    );
    assert!(
        !beyond_radius(0, 0, 0),
        "a zero gate still keeps the anchor"
    );
    assert!(beyond_radius(1, 0, 0));
    assert!(!beyond_radius(0, 0, -5), "negative radii clamp to zero");
}

#[test]
fn wrapped_distance_takes_the_short_way_round() {
    assert!(wrapped_distance(0.1, TAU - 0.1).abs() < 0.21);
    assert!((wrapped_distance(PI, 0.0) - PI).abs() < 1e-9);
}
