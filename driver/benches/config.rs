// SPDX-FileCopyrightText: 2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Benchmarks for configuration handling and device response parsing.
//!
//! The configuration is parsed on every daemon start and reload, and the
//! response parsers run for every device query.

// `criterion_group!` from the CodSpeed compat layer holds a guard that trips
// this nursery lint inside the generated function.
#![allow(clippy::significant_drop_tightening)]

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use logi_mx_driver::{
    config::{Config, DeviceConfig},
    devices::{
        SmartShiftConfig,
        mx_master_3s::{append_name_chunk, encode_smartshift, parse_smartshift}
    }
};

fn sample_config() -> Config {
    Config {
        devices: (0..4)
            .map(|index| DeviceConfig {
                name: format!("MX Master 3S #{index}"),
                ..DeviceConfig::default()
            })
            .collect()
    }
}

fn bench_config(c: &mut Criterion) {
    let config = sample_config();
    let serialized = toml::to_string(&config).expect("config serializes");

    let mut group = c.benchmark_group("config");

    group.bench_function("default", |b| {
        b.iter(|| black_box(Config::default()));
    });

    group.bench_function("serialize_toml", |b| {
        b.iter(|| black_box(toml::to_string(black_box(&config)).ok()));
    });

    group.bench_function("parse_toml", |b| {
        b.iter(|| black_box(toml::from_str::<Config>(black_box(&serialized)).ok()));
    });

    group.finish();
}

fn bench_device_parsing(c: &mut Criterion) {
    let mut group = c.benchmark_group("device_parsing");

    group.bench_function("smartshift_roundtrip", |b| {
        b.iter(|| {
            let payload = encode_smartshift(black_box(SmartShiftConfig {
                enabled:   true,
                threshold: 16
            }));
            black_box(parse_smartshift(black_box(&payload)))
        });
    });

    group.bench_function("device_name", |b| {
        let chunks: [&[u8]; 2] = [b"MX Master 3S For", b" Mac\0\0\0\0\0\0\0\0\0\0\0\0"];
        b.iter(|| {
            let mut name = String::new();
            let mut remaining = black_box(20usize);
            for chunk in chunks {
                remaining -= append_name_chunk(&mut name, black_box(chunk), remaining);
            }
            black_box(name)
        });
    });

    group.finish();
}

criterion_group!(benches, bench_config, bench_device_parsing);
criterion_main!(benches);
