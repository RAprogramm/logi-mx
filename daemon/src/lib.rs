// SPDX-FileCopyrightText: 2025-2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Daemon library exports used by CLI and UI IPC clients.

pub mod buttons;
pub mod focus;
pub mod gesture;
pub mod hyprbind;
pub mod inject;
pub mod ipc;
pub mod status;
pub mod systemd;
pub mod worker;

#[cfg(feature = "notify")]
pub mod notify;

#[cfg(feature = "tray")]
pub mod tray;
#[cfg(feature = "tray")]
pub mod tray_icon;
