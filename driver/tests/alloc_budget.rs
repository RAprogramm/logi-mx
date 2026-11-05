// SPDX-FileCopyrightText: 2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Heap allocation budgets for the driver hot paths.
//!
//! A counting global allocator turns "no unnecessary allocations in hot
//! paths" into an executable budget: the pure codecs must stay
//! allocation-free and the packet encoder may spend exactly one buffer.
//! The counter itself is verified by a negative control that must observe
//! a fresh `Vec`.

use std::{
    alloc::{GlobalAlloc, Layout, System},
    hint::black_box,
    sync::atomic::{AtomicUsize, Ordering}
};

use logi_mx_driver::{
    devices::{
        HiResScrollConfig, SmartShiftConfig, WheelMode,
        mx_master_3s::{encode_smartshift, encode_wheel_mode_flip},
        parse_battery_status, parse_hires_mode, parse_smartshift
    },
    hidpp::HidppPacket
};

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
fn smartshift_codecs_allocate_nothing() {
    let status = [0x02_u8, 0x14, 0x05];
    let config = SmartShiftConfig {
        enabled:   true,
        threshold: 20
    };

    assert_eq!(
        allocations_of(|| black_box(parse_smartshift(black_box(&status)))),
        0
    );
    assert_eq!(
        allocations_of(|| black_box(encode_smartshift(black_box(&status), black_box(config)))),
        0
    );
    assert_eq!(
        allocations_of(|| black_box(encode_wheel_mode_flip(
            black_box(&status),
            WheelMode::FreeSpin
        ))),
        0
    );
}

#[test]
fn scalar_parsers_allocate_nothing() {
    assert_eq!(
        allocations_of(|| black_box(parse_battery_status(black_box(3)))),
        0
    );
    assert_eq!(
        allocations_of(|| black_box(parse_hires_mode(black_box(0x06)))),
        0
    );
    assert_eq!(
        allocations_of(|| black_box(HiResScrollConfig {
            enabled:  true,
            inverted: false
        })),
        0
    );
}

#[test]
fn packet_encoder_stays_within_one_buffer() {
    let count = allocations_of(|| {
        black_box(
            HidppPacket::new_short(
                black_box(0x02),
                black_box(0x05),
                black_box(0x03),
                black_box(0x07),
                [0xAA, 0xBB, 0xCC]
            )
            .to_bytes()
        );
    });
    assert!(
        count <= 1,
        "the short packet encoder may spend at most one Vec, observed {count}"
    );
}
