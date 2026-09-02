// SPDX-FileCopyrightText: 2025 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Daemon library exports used by CLI and UI IPC clients.

pub mod ipc;
pub mod status;
pub mod worker;

#[cfg(feature = "tray")]
pub mod tray;
