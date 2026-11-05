# Benchmarks

Continuous performance tracking for `logi-mx`. CPU time and memory usage are
measured on every pull request and on `main`, so a regression is caught before
it merges instead of after.

[![CodSpeed Badge](https://img.shields.io/endpoint?url=https://app.codspeed.io//badge.json)](https://app.codspeed.io//RAprogramm/logi-mx?utm_source=badge)

## What is measured

| Suite | File | Covers |
|---|---|---|
| `packet_*` | `driver/benches/packet.rs` | HID++ short/long packet encode, decode, roundtrip |
| `smartshift_codec`, `hires_codec`, `battery_and_name` | `driver/benches/codec.rs` | MX Master 3S wire codecs on the device-command hot path |
| `config_toml` | `driver/benches/config.rs` | TOML serialization and deserialization of the full config |
| `ipc_json`, `pie_geometry`, `swipe` | `daemon/benches/hot.rs` | IPC JSON framing, pie wedge selection, swipe classification |

Allocation budgets live beside the tests as executable checks with a counting
global allocator (`driver/tests/alloc_budget.rs`, `daemon/tests/alloc_budget.rs`):
the pure codecs and the pie/swipe hot paths must allocate nothing, the packet
encoder may spend exactly one buffer. These run in the normal test suite and
are deterministic, so they gate every PR without timing noise.

## Running locally

```console
$ cargo bench --workspace            # criterion 0.8, HTML reports in target/criterion
$ cargo bench -p logi-mx-driver -- --test   # smoke: run each benchmark once
```

Comparing two checkouts:

```console
$ cargo install critcmp
# in each checkout, after cargo bench:
$ critcmp store new
```

## The `codspeed` feature

Local benches always run on the real [`criterion`](https://crates.io/crates/criterion)
0.8. The CodSpeed compatibility layer
([`codspeed-criterion-compat`](https://crates.io/crates/codspeed-criterion-compat))
is a separate dev-dependency selected by the `codspeed` feature of
`logi-mx-driver` and `logi-mx-daemon`, which only switches the import in the
bench files:

```rust
#[cfg(not(feature = "codspeed"))]
use criterion::{Criterion, criterion_group, criterion_main};
#[cfg(feature = "codspeed")]
use codspeed_criterion_compat::{Criterion, criterion_group, criterion_main};
```

This keeps local measurements on upstream criterion (the compat fork lags
upstream on new APIs) while CI can build instrumented binaries:

```console
$ cargo bench --no-run --features logi-mx-driver/codspeed,logi-mx-daemon/codspeed
```

## CI policy

`.github/workflows/codspeed.yml` runs on every pull request, every push to
`main`, and on `workflow_dispatch` (used by CodSpeed to backtest history):

1. `cargo codspeed build --features <pkg>/codspeed -p logi-mx-driver -p logi-mx-daemon`
   builds instrumented benchmark binaries.
2. `CodSpeedHQ/action@v5` with `mode: simulation,memory` measures CPU
   instructions in a deterministic simulation (variance below 1%, so a 1–2%
   regression gate is meaningful) and heap allocations per benchmark.
3. Authentication uses GitHub OIDC — no repository secret is required.

The `bench` job in `ci.yml` only compiles the benchmark targets
(`cargo bench --no-run`): wall-clock numbers from shared CI runners are noise,
so timing decisions belong to CodSpeed's simulation, not to `cargo bench` in CI.

## Enabling the dashboard

The repository must be imported at
[app.codspeed.io](https://app.codspeed.io) (GitHub App install, free for public
repositories). The first `workflow_dispatch` run backtests recent commits and
populates the charts; every later PR gets a report comment with per-benchmark
deltas and a flamegraph on regressions.

## Rules for new benchmarks

- One file per subsystem under `benches/`, registered with `harness = false`.
- Benchmark names are time-series identities: renaming one discards history.
- Use `black_box` for inputs and results; prefer `Throughput` so deltas read
  as bytes/sec.
- Anything that must stay allocation-free gets a budget in
  `tests/alloc_budget.rs` instead of a timing assertion.
- The compat layer does not support `iter_custom` or `with_filter`; stick to
  `iter`, `iter_batched` and `bench_function`.
