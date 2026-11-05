// SPDX-FileCopyrightText: 2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Hit-test tests for the preview wheel: wedge boundaries, the centre hole
//! and the outside margin.

use super::{INNER_RADIUS, OUTER_MARGIN, WHEEL_SIZE, slot_at};

fn size() -> f64 {
    f64::from(WHEEL_SIZE)
}

fn centre() -> f64 {
    size() / 2.0
}

#[test]
fn wedge_zero_is_straight_up() {
    let probe = centre() - (INNER_RADIUS + 10.0);
    assert_eq!(slot_at(centre(), probe, size(), 4), Some(0));
}

#[test]
fn wedges_run_clockwise() {
    let reach = INNER_RADIUS + 10.0;
    assert_eq!(slot_at(centre() + reach, centre(), size(), 4), Some(1));
    assert_eq!(slot_at(centre(), centre() + reach, size(), 4), Some(2));
    assert_eq!(slot_at(centre() - reach, centre(), size(), 4), Some(3));
}

#[test]
fn the_count_follows_the_ring_not_the_compass() {
    let reach = INNER_RADIUS + 10.0;
    assert_eq!(
        slot_at(centre() + reach, centre(), size(), 3),
        Some(1),
        "east is the second wedge of a three-ring"
    );
}

#[test]
fn the_hole_and_the_margin_select_nothing() {
    assert_eq!(
        slot_at(centre(), centre(), size(), 4),
        None,
        "the centre hole"
    );
    assert_eq!(slot_at(centre(), centre() - 5.0, size(), 4), None);
    assert_eq!(slot_at(1.0, 1.0, size(), 4), None, "outside the outer edge");
    let edge = centre() - OUTER_MARGIN + 1.0;
    assert_eq!(slot_at(centre(), centre() - edge, size(), 4), None);
}

#[test]
fn an_empty_ring_selects_nothing() {
    assert_eq!(slot_at(centre(), centre() - 50.0, size(), 0), None);
}
