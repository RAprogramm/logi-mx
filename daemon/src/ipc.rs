// SPDX-FileCopyrightText: 2025-2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! IPC protocol between daemon, CLI and UI.
//!
//! The daemon exclusively owns the HID++ transport. CLI and UI act as
//! clients that send [`Request`] over a Unix socket and receive
//! [`Response`]. When the daemon is not running, clients fall back to
//! direct HID access.

mod client;
mod live;
mod protocol;
mod rpc;
mod server;
mod socket;

pub use client::IPC_TIMEOUT;
#[allow(
    unused_imports,
    reason = "client helpers are consumed through the library target"
)]
pub use client::{open_live_stream, open_pie_stream, try_request};
pub use live::{
    BTN_BIT_BACK, BTN_BIT_DPI, BTN_BIT_FORWARD, BTN_BIT_GESTURE, BTN_BIT_LEFT, BTN_BIT_MIDDLE,
    BTN_BIT_RIGHT, LiveFrame, LiveHub, LiveState, SharedLiveState
};
pub use protocol::{Request, Response};
pub use rpc::{RpcRequest, RpcSender};
pub use server::run_server;
pub use socket::{is_socket_activated, socket_path};
