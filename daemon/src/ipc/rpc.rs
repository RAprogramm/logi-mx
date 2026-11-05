// SPDX-FileCopyrightText: 2025-2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! RPC envelope and the gate that forwards client requests to the device
//! worker thread.

use std::sync::mpsc;

use tokio::sync::oneshot;

use super::{Request, Response};

/// RPC envelope forwarded to the worker thread.
#[derive(Debug)]
pub struct RpcRequest {
    /// Client request.
    pub request:   Request,
    /// Channel to send the response.
    pub responder: oneshot::Sender<Response>
}

/// Gate that hands RPC requests to the device worker as device events.
///
/// Sending through the worker's event channel wakes its poll immediately,
/// so a pending RPC is answered without waiting for the next idle tick.
#[derive(Clone)]
pub struct RpcSender {
    events: mpsc::Sender<crate::worker::DeviceEvent>
}

impl RpcSender {
    /// Wraps the device worker's event sender.
    #[must_use]
    pub const fn new(events: mpsc::Sender<crate::worker::DeviceEvent>) -> Self {
        Self {
            events
        }
    }

    /// Enqueues `request` on the worker's event queue.
    ///
    /// # Arguments
    ///
    /// * `request` - RPC envelope carrying the client's response channel.
    ///
    /// # Returns
    ///
    /// `Ok` while the worker still accepts events; the request is dropped
    /// once the worker has exited.
    ///
    /// # Errors
    ///
    /// [`masterror::AppError`] (internal) when the worker thread has shut
    /// down and its event receiver is gone.
    pub fn dispatch(&self, request: RpcRequest) -> Result<(), masterror::AppError> {
        self.events
            .send(crate::worker::DeviceEvent::Rpc(request))
            .map_err(|_| masterror::AppError::internal("IPC worker channel closed"))
    }
}
