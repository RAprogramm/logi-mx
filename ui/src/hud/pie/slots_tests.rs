// SPDX-FileCopyrightText: 2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Pure ring-edit tests: slot replacement, removal and the summary line.

use logi_mx_driver::{config::PieSlot, prelude::Action};

use super::{remove_slot, replace_slot, slot_summary};

fn key_slot(key: &str) -> PieSlot {
    PieSlot {
        action: Some(Action::Keypress {
            keys: vec![format!("KEY_{key}")]
        }),
        label:  None
    }
}

fn ring() -> Vec<PieSlot> {
    (0..4).map(|i| key_slot(&format!("K{i}"))).collect()
}

#[test]
fn replace_swaps_exactly_one_slot() {
    let next = replace_slot(&ring(), 2, PieSlot::default());
    assert_eq!(next.len(), 4);
    assert!(!next[2].is_bound(), "the cleared slot keeps its wedge");
    assert!(next[0].is_bound() && next[1].is_bound() && next[3].is_bound());
}

#[test]
fn replace_out_of_range_leaves_the_ring_alone() {
    let source = ring();
    assert_eq!(replace_slot(&source, 9, PieSlot::default()), source);
}

#[test]
fn remove_renumbers_the_rest() {
    let next = remove_slot(&ring(), 0);
    assert_eq!(next.len(), 3);
    let summary = slot_summary(&next, Some(0));
    assert!(
        summary.starts_with("Slot 1 — "),
        "wedges renumber after a removal"
    );
}

#[test]
fn remove_out_of_range_leaves_the_ring_alone() {
    let source = ring();
    assert_eq!(remove_slot(&source, 4), source);
}

#[test]
fn summaries_name_the_slot_and_its_action() {
    let source = ring();
    assert_eq!(
        slot_summary(&source, Some(1)),
        "Slot 2 — K1",
        "key names collapse to their short label"
    );
    assert_eq!(
        slot_summary(&source, None),
        "Click a wedge to inspect its slot"
    );
    assert_eq!(
        slot_summary(&source, Some(7)),
        "Click a wedge to inspect its slot",
        "a stale index falls back to the hint"
    );
    let cleared = replace_slot(&source, 1, PieSlot::default());
    assert_eq!(slot_summary(&cleared, Some(1)), "Slot 2 — empty");
}
