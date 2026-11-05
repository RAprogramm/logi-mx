// SPDX-FileCopyrightText: 2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Benchmarks for the daemon hot paths: IPC JSON framing, pie geometry and
//! the swipe accumulator.
//!
//! Every request from the UI or CLI crosses the JSON line protocol, the pie
//! ring recomputes wedge selection on each raw-XY report, and the swipe
//! accumulator runs per diverted movement event, so all three are tracked
//! explicitly.
//!
//! The `codspeed` feature path allows `clippy::significant_drop_tightening`
//! because the compatibility layer's `criterion_group!` macro keeps the
//! harness temporary alive across benchmark targets, which is outside our
//! code.
#![cfg_attr(feature = "codspeed", allow(clippy::significant_drop_tightening))]

use std::{hint::black_box, time::Instant};

#[cfg(feature = "codspeed")]
use codspeed_criterion_compat::{Criterion, Throughput, criterion_group, criterion_main};
#[cfg(not(feature = "codspeed"))]
use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use logi_mx_daemon::{
    gesture::{SwipeAccumulator, detect_swipe},
    ipc::{Request, Response},
    pie::geometry::{beyond_radius, direction_angle, slot_for_angle, slot_index, wedge_for}
};
use logi_mx_driver::devices::{GestureDirection, HiResScrollConfig, SmartShiftConfig};

fn bench_ipc_json(c: &mut Criterion) {
    let mut group = c.benchmark_group("ipc_json");
    let ping = Request::Ping;
    let set_smartshift = Request::SetSmartShift {
        enabled:   true,
        threshold: 20
    };
    let info = Response::Info {
        name:       "MX Master 3S".to_string(),
        dpi:        1000,
        smartshift: SmartShiftConfig {
            enabled:   true,
            threshold: 20
        },
        hires:      HiResScrollConfig {
            enabled:  false,
            inverted: false
        }
    };
    let ping_text = serde_json::to_string(&ping).unwrap_or_default();
    let info_text = serde_json::to_string(&info).unwrap_or_default();
    group.throughput(Throughput::Bytes(info_text.len() as u64));

    group.bench_function("serialize_unit_variant", |b| {
        b.iter(|| black_box(serde_json::to_string(black_box(&ping)).unwrap_or_default()));
    });
    group.bench_function("serialize_struct_variant", |b| {
        b.iter(|| {
            black_box(serde_json::to_string(black_box(&set_smartshift)).unwrap_or_default())
        });
    });
    group.bench_function("serialize_info_response", |b| {
        b.iter(|| black_box(serde_json::to_string(black_box(&info)).unwrap_or_default()));
    });
    group.bench_function("deserialize_unit_variant", |b| {
        b.iter(|| black_box(serde_json::from_str::<Request>(black_box(&ping_text)).is_ok()));
    });
    group.bench_function("deserialize_info_response", |b| {
        b.iter(|| black_box(serde_json::from_str::<Response>(black_box(&info_text)).is_ok()));
    });
    group.finish();
}

fn bench_pie_geometry(c: &mut Criterion) {
    let mut group = c.benchmark_group("pie_geometry");

    group.bench_function("slot_index_sweep", |b| {
        b.iter(|| {
            for dy in (-200_i32..=200).step_by(7) {
                for dx in (-200_i32..=200).step_by(7) {
                    black_box(slot_index(black_box(dx), black_box(dy), black_box(6)));
                }
            }
        });
    });
    group.bench_function("slot_for_angle_sweep", |b| {
        b.iter(|| {
            for degree in (0..360).step_by(3) {
                black_box(slot_for_angle(black_box(f64::from(degree)), black_box(8)));
            }
        });
    });
    group.bench_function("wedge_for_sweep", |b| {
        b.iter(|| {
            for index in 0..8_usize {
                black_box(wedge_for(black_box(index), black_box(8)));
            }
        });
    });
    group.bench_function("direction_angle_all", |b| {
        let directions = [
            GestureDirection::None,
            GestureDirection::Up,
            GestureDirection::Down,
            GestureDirection::Left,
            GestureDirection::Right,
            GestureDirection::UpLeft,
            GestureDirection::UpRight,
            GestureDirection::DownLeft,
            GestureDirection::DownRight
        ];
        b.iter(|| {
            for direction in directions {
                black_box(direction_angle(black_box(direction)));
            }
        });
    });
    group.bench_function("beyond_radius", |b| {
        b.iter(|| {
            black_box(beyond_radius(
                black_box(140),
                black_box(-90),
                black_box(120)
            ))
        });
    });
    group.finish();
}

fn bench_swipe(c: &mut Criterion) {
    let mut group = c.benchmark_group("swipe");

    group.bench_function("detect_swipe_sweep", |b| {
        b.iter(|| {
            for dy in (-300_i32..=300).step_by(5) {
                for dx in (-300_i32..=300).step_by(5) {
                    black_box(detect_swipe(black_box(dx), black_box(dy)));
                }
            }
        });
    });
    group.bench_function("accumulator_hold_64_reports", |b| {
        let mut accumulator = SwipeAccumulator::default();
        let now = Instant::now();
        b.iter(|| {
            accumulator.begin(now);
            for step in 0..64_i32 {
                black_box(accumulator.accumulate(black_box(step * 4), black_box(0), now));
            }
        });
    });
    group.finish();
}

criterion_group!(benches, bench_ipc_json, bench_pie_geometry, bench_swipe);
criterion_main!(benches);
