// SPDX-FileCopyrightText: 2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Benchmarks for the HID++ packet codec.
//!
//! The codec sits on the hot path of every device command, so encode and
//! decode costs are tracked explicitly.

use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use logi_mx_driver::hidpp::{HidppPacket, SHORT_PACKET_SIZE};

fn bench_packet_encode(c: &mut Criterion) {
    let mut group = c.benchmark_group("packet_encode");
    group.throughput(Throughput::Bytes(SHORT_PACKET_SIZE as u64));

    group.bench_function("short", |b| {
        b.iter(|| {
            let packet = HidppPacket::new_short(
                black_box(0x02),
                black_box(0x05),
                black_box(0x03),
                black_box(0x07),
                [0xAA, 0xBB, 0xCC]
            );
            black_box(packet.to_bytes());
        });
    });

    group.bench_function("long", |b| {
        b.iter(|| {
            let packet = HidppPacket::new_long(
                black_box(0x02),
                black_box(0x05),
                black_box(0x03),
                black_box(0x07),
                [1u8; 16]
            );
            black_box(packet.to_bytes());
        });
    });

    group.finish();
}

fn bench_packet_decode(c: &mut Criterion) {
    let short = HidppPacket::new_short(0x02, 0x05, 0x03, 0x07, [0xAA, 0xBB, 0xCC]).to_bytes();
    let long = HidppPacket::new_long(0x02, 0x05, 0x03, 0x07, [1u8; 16]).to_bytes();

    let mut group = c.benchmark_group("packet_decode");
    group.throughput(Throughput::Bytes(SHORT_PACKET_SIZE as u64));

    group.bench_function(BenchmarkId::new("short", short.len()), |b| {
        b.iter(|| black_box(HidppPacket::from_bytes(black_box(&short)).ok()));
    });

    group.bench_function(BenchmarkId::new("long", long.len()), |b| {
        b.iter(|| black_box(HidppPacket::from_bytes(black_box(&long)).ok()));
    });

    group.finish();
}

fn bench_packet_roundtrip(c: &mut Criterion) {
    let mut group = c.benchmark_group("packet_roundtrip");

    group.bench_function("short", |b| {
        b.iter(|| {
            let packet = HidppPacket::new_short(
                black_box(0x02),
                black_box(0x05),
                black_box(0x03),
                black_box(0x07),
                [0xAA, 0xBB, 0xCC]
            );
            let bytes = packet.to_bytes();
            black_box(HidppPacket::from_bytes(black_box(&bytes)).ok());
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_packet_encode,
    bench_packet_decode,
    bench_packet_roundtrip
);
criterion_main!(benches);
