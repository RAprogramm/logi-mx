// SPDX-FileCopyrightText: 2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Benchmarks for the MX Master 3S wire codecs.
//!
//! Every codec here runs on a device-command hot path: status reads fire on
//! each daemon poll and every wheel-mode toggle, so their per-call cost is
//! tracked explicitly.
//!
//! The `codspeed` feature path allows `clippy::significant_drop_tightening`
//! because the compatibility layer's `criterion_group!` macro keeps the
//! harness temporary alive across benchmark targets, which is outside our
//! code.
#![cfg_attr(feature = "codspeed", allow(clippy::significant_drop_tightening))]

use std::hint::black_box;

#[cfg(feature = "codspeed")]
use codspeed_criterion_compat::{Criterion, criterion_group, criterion_main};
#[cfg(not(feature = "codspeed"))]
use criterion::{Criterion, criterion_group, criterion_main};
use logi_mx_driver::devices::{
    HiResScrollConfig, SmartShiftConfig, WheelMode,
    mx_master_3s::{
        append_name_chunk, encode_hires_mode, encode_smartshift, encode_wheel_mode_flip,
        validate_smartshift
    },
    parse_battery_status, parse_hires_mode, parse_smartshift
};

fn bench_smartshift_codec(c: &mut Criterion) {
    let mut group = c.benchmark_group("smartshift_codec");
    let status = [0x02_u8, 0x14, 0x05];
    let never = [0x02_u8, 0xFF, 0x05];
    let on = SmartShiftConfig {
        enabled:   true,
        threshold: 20
    };
    let off = SmartShiftConfig {
        enabled:   false,
        threshold: 0
    };

    group.bench_function("parse_enabled", |b| {
        b.iter(|| black_box(parse_smartshift(black_box(&status))));
    });
    group.bench_function("parse_never_disengage", |b| {
        b.iter(|| black_box(parse_smartshift(black_box(&never))));
    });
    group.bench_function("encode_enabled", |b| {
        b.iter(|| black_box(encode_smartshift(black_box(&status), black_box(on))));
    });
    group.bench_function("encode_disabled", |b| {
        b.iter(|| black_box(encode_smartshift(black_box(&status), black_box(off))));
    });
    group.bench_function("flip_wheel_mode", |b| {
        b.iter(|| {
            black_box(encode_wheel_mode_flip(
                black_box(&status),
                WheelMode::FreeSpin
            ))
        });
    });
    group.bench_function("validate_ok", |b| {
        b.iter(|| black_box(validate_smartshift(black_box(&on)).is_ok()));
    });
    group.finish();
}

fn bench_hires_codec(c: &mut Criterion) {
    let mut group = c.benchmark_group("hires_codec");
    let mode = 0x06_u8;
    let config = HiResScrollConfig {
        enabled:  true,
        inverted: true
    };

    group.bench_function("parse", |b| {
        b.iter(|| black_box(parse_hires_mode(black_box(mode))));
    });
    group.bench_function("encode", |b| {
        b.iter(|| black_box(encode_hires_mode(black_box(mode), black_box(config))));
    });
    group.finish();
}

fn bench_battery_and_name(c: &mut Criterion) {
    let mut group = c.benchmark_group("battery_and_name");

    group.bench_function("parse_battery_all_bytes", |b| {
        b.iter(|| {
            for byte in 0_u8..=255 {
                black_box(parse_battery_status(black_box(byte)));
            }
        });
    });
    group.bench_function("append_name_chunk", |b| {
        let chunk = *b"MX Master 3S\0\0\0\0";
        b.iter(|| {
            let mut name = String::new();
            black_box(append_name_chunk(
                &mut name,
                black_box(&chunk),
                black_box(14)
            ));
            black_box(name);
        });
    });
    group.finish();
}

criterion_group!(
    benches,
    bench_smartshift_codec,
    bench_hires_codec,
    bench_battery_and_name
);
criterion_main!(benches);
