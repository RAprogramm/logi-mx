// SPDX-FileCopyrightText: 2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Heap allocation budgets for the daemon hot paths.
//!
//! The pie geometry and the swipe classifier run on every raw-XY report,
//! so they must stay allocation-free. A counting global allocator makes
//! that an executable budget, verified by a negative control that must
//! observe a fresh `Vec`.

use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
    hint::black_box
};

use logi_mx_daemon::{
    gesture::detect_swipe,
    pie::geometry::{beyond_radius, direction_angle, slot_for_angle, slot_index, wedge_for}
};
use logi_mx_driver::devices::GestureDirection;

thread_local! {
    static MEASURING: Cell<bool> = const { Cell::new(false) };
    static COUNT: Cell<usize> = const { Cell::new(0) };
}

/// Forwards every request to the system allocator while counting it.
///
/// Only the thread that armed its `MEASURING` flag is counted, so tests
/// running in parallel cannot perturb each other's budgets.
struct Counting;

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        COUNT.with(|count| {
            if MEASURING.get() {
                count.set(count.get() + 1);
            }
        });
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) }
    }
}

#[global_allocator]
static COUNTING: Counting = Counting;

/// Counts the heap allocations one operation performs on this thread.
///
/// The operation runs eight times and the smallest observed count wins, so
/// a stray allocation from the test harness can never fail a budget that
/// the code itself does not spend. Counting is armed only around the
/// measured call and only for this thread, so concurrent tests neither
/// pollute a budget nor hide an allocation.
fn allocations_of<T>(operation: impl Fn() -> T) -> usize {
    let mut best = usize::MAX;
    for _ in 0..8 {
        drop(operation());
        COUNT.with(|count| count.set(0));
        MEASURING.with(|measuring| measuring.set(true));
        let measured = black_box(operation());
        let count = COUNT.with(Cell::get);
        MEASURING.with(|measuring| measuring.set(false));
        drop(black_box(measured));
        best = best.min(count);
    }
    best
}

#[test]
fn counter_detects_real_allocations() {
    let count = allocations_of(|| {
        let mut buffer: Vec<u8> = Vec::with_capacity(64);
        for byte in 0..64_u8 {
            buffer.push(black_box(byte));
        }
        black_box(&buffer);
    });
    assert!(
        count >= 1,
        "the counting allocator must observe a freshly allocated Vec"
    );
}

#[test]
fn pie_geometry_allocates_nothing() {
    assert_eq!(
        allocations_of(|| black_box(slot_index(black_box(40), black_box(-90), 6))),
        0
    );
    assert_eq!(
        allocations_of(|| black_box(slot_for_angle(black_box(123.5), 8))),
        0
    );
    assert_eq!(allocations_of(|| black_box(wedge_for(black_box(3), 8))), 0);
    assert_eq!(
        allocations_of(|| black_box(direction_angle(black_box(GestureDirection::UpLeft)))),
        0
    );
    assert_eq!(
        allocations_of(|| black_box(beyond_radius(black_box(140), black_box(-90), 120))),
        0
    );
}

#[test]
fn swipe_detection_allocates_nothing() {
    assert_eq!(
        allocations_of(|| black_box(detect_swipe(black_box(120), black_box(10)))),
        0
    );
    assert_eq!(
        allocations_of(|| black_box(detect_swipe(black_box(3), black_box(2)))),
        0
    );
}
