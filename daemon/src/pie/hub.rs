// SPDX-FileCopyrightText: 2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Wire vocabulary of the pie: the event fan-out with its mid-hold replay
//! latch, the events themselves, and how a settled hold is reported to the
//! host.

use std::sync::{Arc, Mutex, PoisonError};

use logi_mx_driver::prelude::Action;
use serde::{Deserialize, Serialize};

/// Fan-out carrying pie menu events to subscribed clients.
///
/// Besides the broadcast channel it latches the current charge or open
/// event, so a watcher that connects mid-hold — e.g. an overlay restarted
/// while the button is pressed — receives the state it missed instead of
/// staying blind until the next release.
#[derive(Clone, Debug)]
pub struct PieHub {
    tx:    tokio::sync::broadcast::Sender<PieEvent>,
    latch: Arc<Mutex<Option<PieEvent>>>
}

impl PieHub {
    /// Builds a fan-out with the given broadcast capacity.
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        Self {
            tx:    tokio::sync::broadcast::channel(capacity).0,
            latch: Arc::new(Mutex::new(None))
        }
    }

    /// Subscribes to the event stream.
    #[must_use]
    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<PieEvent> {
        self.tx.subscribe()
    }

    /// Number of live subscribers.
    #[must_use]
    pub fn receiver_count(&self) -> usize {
        self.tx.receiver_count()
    }

    /// Publishes `event` and updates the latch a late watcher replays.
    pub fn send(&self, event: PieEvent) {
        match &event {
            PieEvent::Charge {
                ..
            }
            | PieEvent::Open {
                ..
            } => {
                *self.latch.lock().unwrap_or_else(PoisonError::into_inner) = Some(event.clone());
            }
            PieEvent::Close {
                ..
            } => {
                *self.latch.lock().unwrap_or_else(PoisonError::into_inner) = None;
            }
            PieEvent::Highlight {
                ..
            }
            | PieEvent::Aim {
                ..
            } => {}
        }
        let _ = self.tx.send(event);
    }

    /// The latched charge or open event when a hold is in flight, `None`
    /// while idle.
    #[must_use]
    pub fn replay(&self) -> Option<PieEvent> {
        self.latch
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

/// One event on the pie stream, mirroring the state machine transitions.
///
/// `Charge` starts the thin hold-gate ring on press, `Open` replaces it with
/// the full menu once the hold gate passes (or immediately on a double
/// click), `Highlight` marks every wedge change while open (swipe or
/// cursor), and `Close` ends the visual — exactly once per `Charge` or
/// `Open`, whichever engaged.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PieEvent {
    /// A hold is charging at `anchor`; the overlay fills a thin ring.
    Charge {
        /// Press point in screen coordinates.
        anchor: (i32, i32)
    },
    /// The pie opened at `anchor` with the resolved ring.
    Open {
        /// Press point in screen coordinates.
        anchor:  (i32, i32),
        /// Ring wedges in index order.
        sectors: Vec<super::labels::PieSector>
    },
    /// The selected wedge changed; `None` when nothing is selected.
    Highlight {
        /// Selected wedge index, `None` while the cursor sits inside the
        /// gate.
        index: Option<u8>
    },
    /// The cursor aim angle moved, so the overlay can point a needle at the
    /// exact direction instead of only at whole wedges.
    Aim {
        /// Radians in screen coordinates, 0 along +x, clockwise (y down).
        angle: f64
    },
    /// The pie closed; `committed` is the fired wedge, if any.
    Close {
        /// Wedge fired on release, `None` for a cancel or an empty slot.
        committed: Option<u8>
    }
}

/// How a gesture hold settled, as reported to the host.
#[derive(Debug, PartialEq, Eq)]
pub enum PieRelease {
    /// The ring was open; carries the committed slot's action when the wedge
    /// was bound. The host fires the action and must not fall back to the
    /// swipe release.
    Committed(Option<Action>),
    /// The hold ended while charging: a tap. The host defers the click
    /// binding so a second press can become a double click.
    Charging,
    /// The pie never engaged; the host settles the hold as a plain swipe.
    Idle
}
