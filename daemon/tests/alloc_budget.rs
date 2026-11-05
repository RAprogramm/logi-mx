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
    hint::black_box,
    sync::atomic::{AtomicUsize, Ordering}
};

use logi_mx_daemon::{
    gesture::detect_swipe,
    pie::geometry::{beyond_radius, direction_angle, slot_for_angle, slot_index, wedge_for}
};
use logi_mx_driver::devices::GestureDirection;

static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);

/// Forwards every request to the system allocator while counting it.
struct Counting;

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) }
    }
}

#[global_allocator]
static COUNTING: Counting = Counting;

/// Counts the heap allocations one operation performs.
///
/// The operation runs eight times and the smallest observed count wins, so
/// a stray allocation from the test harness can never fail a budget that
/// the code itself does not spend.
fn allocations_of<T>(operation: impl Fn() -> T) -> usize {
    let mut best = usize::MAX;
    for _ in 0..8 {
        drop(operation());
        ALLOCATIONS.store(0, Ordering::Relaxed);
        let measured = black_box(operation());
        let count = ALLOCATIONS.load(Ordering::Relaxed);
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
