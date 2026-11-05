// SPDX-FileCopyrightText: 2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Benchmarks for configuration parsing and serialization.
//!
//! The daemon reads and writes the TOML document on startup and on every
//! persisted setting change, so the round-trip cost is tracked explicitly.
//!
//! The `codspeed` feature path allows `clippy::significant_drop_tightening`
//! because the compatibility layer's `criterion_group!` macro keeps the
//! harness temporary alive across benchmark targets, which is outside our
//! code.
#![cfg_attr(feature = "codspeed", allow(clippy::significant_drop_tightening))]

use std::hint::black_box;

#[cfg(feature = "codspeed")]
use codspeed_criterion_compat::{Criterion, Throughput, criterion_group, criterion_main};
#[cfg(not(feature = "codspeed"))]
use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use logi_mx_driver::config::Config;

fn bench_config_toml(c: &mut Criterion) {
    let mut group = c.benchmark_group("config_toml");
    let config = Config::default();
    let text = toml::to_string(&config).unwrap_or_default();
    group.throughput(Throughput::Bytes(text.len() as u64));

    group.bench_function("serialize", |b| {
        b.iter(|| black_box(toml::to_string(black_box(&config)).unwrap_or_default()));
    });
    group.bench_function("deserialize", |b| {
        b.iter(|| {
            black_box(toml::from_str::<Config>(black_box(&text)).is_ok());
        });
    });
    group.finish();
}

criterion_group!(benches, bench_config_toml);
criterion_main!(benches);
