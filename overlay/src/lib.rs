// SPDX-FileCopyrightText: 2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Passive radial pie menu overlay for the logi-mx daemon.
//!
//! The daemon owns the entire pie decision core ([`logi_mx_daemon::pie`]);
//! this crate only renders what the daemon streams on a `WatchPie`
//! connection: one transparent, click-through layer-shell window per
//! monitor, mapped at startup and drawing nothing while idle, plus the
//! socket pump that folds decoded [`PieEvent`] lines into the GTK main
//! context.
//!
//! The binary `logi-mx-overlay` wires these pieces into a GTK application.
//! The pieces live in this library so the pure halves — monitor mapping,
//! octant geometry, event decoding and the scene state machine — are
//! unit-testable without a compositor or a daemon.
//!
//! [`PieEvent`]: logi_mx_daemon::pie::PieEvent

pub mod draw;
pub mod icons;
pub mod scene;
pub mod stream;
