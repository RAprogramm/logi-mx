// SPDX-FileCopyrightText: 2025-2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! RPC handlers executed on the device worker thread: gesture and button
//! configuration requests, read-only device queries and device-bound writes.

mod buttons;
mod device;
mod gestures;
mod settings;
mod validate;

#[cfg(test)]
mod tests;

pub(in crate::worker) use buttons::handle_button_request;
pub(in crate::worker) use device::handle_device_request;
pub(in crate::worker) use gestures::handle_gesture_request;
pub(in crate::worker) use settings::apply_setting;
