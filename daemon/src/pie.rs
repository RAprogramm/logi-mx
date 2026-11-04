// SPDX-FileCopyrightText: 2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Radial pie menu runtime for the diverted gesture button.
//!
//! Pressing the gesture button opens the menu at the cursor: the ring shows
//! one sector per bound direction. The selection follows the wheel — a
//! committed swipe direction highlights its sector — and, as a secondary
//! input, the cursor once it travels further than the activation radius from
//! the anchor. Releasing over a highlighted sector that has a bound action
//! fires it, like a Dota-style radial menu. This module is the daemon-side
//! half of the feature: it runs the state machine and publishes [`PieEvent`]
//! lines for the overlay renderer over a [`PieHub`] broadcast, mirroring the
//! live telemetry fan-out.
//!
//! Everything input-driven is either a pure function ([`sector_for`]) or
//! runs against an injected cursor reader, so the whole state machine is
//! testable without Hyprland. The press point is captured through the
//! Hyprland request socket ([`crate::hyprbind::fetch_cursor_pos`]) because
//! raw-XY deltas carry no absolute position.
//!
//! Swipe semantics stay intact: while the pie is enabled, raw-XY travel
//! still feeds the swipe accumulator, so `OnPress`-mode swipe commits keep
//! firing mid-hold exactly as before. Only the release moment changes: a
//! hold whose pie was open fires the highlighted sector's `OnRelease`
//! bindings instead of the swipe fallback, so one hold never fires twice.

use std::{
    collections::HashMap,
    hash::BuildHasher,
    sync::{Arc, Mutex, PoisonError}
};

use logi_mx_driver::prelude::{Gesture, GestureDirection};
use serde::{Deserialize, Serialize};
use tracing::debug;

use crate::focus::SharedFocus;

/// Fan-out channel carrying pie menu events to subscribed clients.
pub type PieHub = tokio::sync::broadcast::Sender<PieEvent>;

/// Re-export of the stored pie tuning so daemon consumers import it from
/// the pie module; the schema itself lives in the driver crate because the
/// device configuration embeds it.
pub use logi_mx_driver::config::PieConfig;

/// One bound sector of the pie, as advertised on the wire.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PieSector {
    /// Direction this sector commits.
    pub direction: GestureDirection,
    /// Human label, currently the direction name.
    pub label:     String
}

/// One event on the pie stream, mirroring the state machine transitions.
///
/// `Open` is emitted when the gesture button is pressed with the pie
/// enabled and bound sectors, `Highlight` on every sector change while open
/// (wheel swipe or cursor), and `Close` exactly once per `Open`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PieEvent {
    /// The pie opened at `anchor` with the sectors bound in the scope.
    Open {
        /// Press point in screen coordinates.
        anchor:  (i32, i32),
        /// Bound sectors in binding order.
        sectors: Vec<PieSector>
    },
    /// The selected sector changed; `None` when nothing is selected.
    Highlight {
        /// Selected sector, `None` while the cursor sits inside the gate.
        direction: Option<GestureDirection>
    },
    /// The pie closed; `committed` is the fired direction, if any.
    Close {
        /// Direction fired on release, `None` for a cancel or unbound sector.
        committed: Option<GestureDirection>
    }
}

/// Tangent of 22.5 degrees scaled by [`TAN_SCALE`], the cardinal/diagonal
/// octant boundary (tan 22.5° = √2 − 1 ≈ 0.41421356).
const TAN_22_5: i64 = 4142;
/// Tangent of 67.5 degrees scaled by [`TAN_SCALE`] (√2 + 1 ≈ 2.41421356).
const TAN_67_5: i64 = 24142;
/// Fixed-point scale for the tangent comparisons.
const TAN_SCALE: i64 = 10_000;

/// Maps a cursor vector to the octant it points at, in screen coordinates
/// (y grows downward).
///
/// Returns `None` inside the dead zone, i.e. when the Euclidean length is at
/// most `dead_zone` (a negative zone is treated as zero). Otherwise the
/// octant: `Right` spans −22.5°..=22.5°, then clockwise `DownRight`, `Down`,
/// `DownLeft`, `Left`, `UpLeft`, `Up`, `UpRight`. The 22.5° and 67.5°
/// boundaries are compared in fixed-point integer math (scaled tangents,
/// `i64` intermediates), and a vector exactly on a boundary belongs to the
/// cardinal sector. The result is never [`GestureDirection::None`].
///
/// # Arguments
///
/// * `dx` - Horizontal cursor offset from the anchor, positive rightward.
/// * `dy` - Vertical cursor offset from the anchor, positive downward.
/// * `dead_zone` - Radius inside which no sector is selected.
///
/// # Returns
///
/// The octant direction, or `None` inside the dead zone.
///
/// # Examples
///
/// ```
/// use logi_mx_daemon::pie::sector_for;
/// use logi_mx_driver::devices::GestureDirection;
///
/// assert_eq!(sector_for(30, 0, 12), Some(GestureDirection::Right));
/// assert_eq!(sector_for(0, -30, 12), Some(GestureDirection::Up));
/// assert_eq!(sector_for(3, 4, 12), None);
/// ```
#[must_use]
pub fn sector_for(dx: i32, dy: i32, dead_zone: i32) -> Option<GestureDirection> {
    let (abs_x, abs_y) = (
        i64::from(dx.saturating_abs()),
        i64::from(dy.saturating_abs())
    );
    let zone = i64::from(dead_zone.max(0));
    if abs_x * abs_x + abs_y * abs_y <= zone * zone {
        return None;
    }
    let down = dy >= 0;
    if dx >= 0 {
        if abs_y * TAN_SCALE <= abs_x * TAN_22_5 {
            Some(GestureDirection::Right)
        } else if abs_y * TAN_SCALE <= abs_x * TAN_67_5 {
            Some(if down {
                GestureDirection::DownRight
            } else {
                GestureDirection::UpRight
            })
        } else {
            Some(if down {
                GestureDirection::Down
            } else {
                GestureDirection::Up
            })
        }
    } else if abs_y * TAN_SCALE <= abs_x * TAN_22_5 {
        Some(GestureDirection::Left)
    } else if abs_y * TAN_SCALE <= abs_x * TAN_67_5 {
        Some(if down {
            GestureDirection::DownLeft
        } else {
            GestureDirection::UpLeft
        })
    } else {
        Some(if down {
            GestureDirection::Down
        } else {
            GestureDirection::Up
        })
    }
}

/// Merges the global bindings with the focused app's profile overrides.
///
/// A profile entry replaces the global binding for its direction; directions
/// the profile does not mention fall through to the global set.
///
/// Shared with [`crate::gesture::GestureRuntime`] so the pie advertises
/// exactly the sectors the runtime would fire.
#[must_use]
pub fn merge_effective<S: BuildHasher>(
    bindings: &[Gesture],
    profiles: &HashMap<String, Vec<Gesture>, S>,
    focused: Option<&str>
) -> Vec<Gesture> {
    let profile = focused.and_then(|app| profiles.get(app));
    bindings
        .iter()
        .filter(|binding| {
            profile.is_none_or(|overrides| {
                !overrides
                    .iter()
                    .any(|entry| entry.direction == binding.direction)
            })
        })
        .chain(profile.into_iter().flatten())
        .cloned()
        .collect()
}

/// Cursor reader seam: returns the current screen position, `None` when the
/// desktop cannot be queried. Tests inject a scripted reader; production
/// uses [`crate::hyprbind::fetch_cursor_pos`].
type AnchorFetch = Box<dyn Fn() -> Option<(i32, i32)> + Send>;

/// Pie menu state machine for one gesture hold.
///
/// Owns the tuning, the effective binding scope (global bindings plus
/// per-app profiles resolved through [`SharedFocus`]), the hold state
/// (anchor, open flag, last highlight) and the event fan-out. The host
/// drives it with [`Self::on_press`], [`Self::on_swipe`] when a swipe
/// commits, [`Self::on_tick`] per poll while the button is held, and
/// [`Self::on_release`] or [`Self::on_cancel`] to settle the hold.
///
/// A press with the pie enabled opens the ring at the cursor immediately.
/// Selection then follows whichever input moves: a committed swipe direction
/// highlights its sector, and a cursor further than the activation radius
/// from the anchor highlights the sector it points at. While the pie is
/// enabled, swipe recognition keeps running in parallel: `OnPress`-mode
/// swipe commits fire mid-hold unchanged. The pie only takes over the
/// release moment: an open pie commits its sector and the host suppresses
/// the swipe `OnRelease` fallback for that hold.
///
/// # Examples
///
/// ```
/// use std::collections::HashMap;
///
/// use logi_mx_daemon::pie::PieRuntime;
/// use logi_mx_driver::config::PieConfig;
///
/// let mut pie = PieRuntime::with_fetch(|| Some((0, 0)));
/// pie.set_config(PieConfig {
///     enabled: true,
///     ..PieConfig::default()
/// });
/// assert!(!pie.should_open(), "without bindings the pie stays closed");
/// pie.set_bindings(Vec::new(), HashMap::new());
/// ```
pub struct PieRuntime {
    config:       PieConfig,
    focus:        SharedFocus,
    bindings:     Vec<Gesture>,
    profiles:     HashMap<String, Vec<Gesture>>,
    fetch_anchor: AnchorFetch,
    hub:          Option<PieHub>,
    anchor:       Option<(i32, i32)>,
    sectors:      Vec<PieSector>,
    open:         bool,
    highlight:    Option<GestureDirection>
}

impl Default for PieRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl PieRuntime {
    /// Builds a runtime reading the real cursor through Hyprland.
    #[must_use]
    pub fn new() -> Self {
        Self::with_fetch(|| crate::hyprbind::fetch_cursor_pos().ok())
    }

    /// Builds a runtime with an injected cursor reader.
    ///
    /// # Arguments
    ///
    /// * `fetch` - Called on press and on every tick; returns the current
    ///   screen position or `None` when unavailable.
    #[must_use]
    pub fn with_fetch<F>(fetch: F) -> Self
    where
        F: Fn() -> Option<(i32, i32)> + Send + 'static
    {
        Self {
            config:       PieConfig::default(),
            focus:        Arc::new(Mutex::new(None)),
            bindings:     Vec::new(),
            profiles:     HashMap::new(),
            fetch_anchor: Box::new(fetch),
            hub:          None,
            anchor:       None,
            sectors:      Vec::new(),
            open:         false,
            highlight:    None
        }
    }

    /// Attaches the event fan-out the state machine publishes on.
    pub fn attach_hub(&mut self, hub: PieHub) {
        self.hub = Some(hub);
    }

    /// Attaches the shared focus slot consulted when resolving bindings.
    pub fn attach_focus(&mut self, focus: SharedFocus) {
        self.focus = focus;
    }

    /// Replaces the pie tuning.
    pub const fn set_config(&mut self, config: PieConfig) {
        self.config = config;
    }

    /// Replaces the global bindings and per-app profiles the pie resolves
    /// its sectors from; the host keeps them in sync with the gesture
    /// runtime.
    pub fn set_bindings(
        &mut self,
        bindings: Vec<Gesture>,
        profiles: HashMap<String, Vec<Gesture>>
    ) {
        self.bindings = bindings;
        self.profiles = profiles;
    }

    /// Whether a press may open the pie: enabled and at least one directional
    /// sector bound in the effective scope. A lone click binding
    /// ([`GestureDirection::None`]) does not count.
    #[must_use]
    pub fn should_open(&self) -> bool {
        self.config.enabled
            && self
                .effective_bindings()
                .iter()
                .any(|gesture| gesture.direction != GestureDirection::None)
    }

    /// Whether the pie ring is currently on screen.
    #[must_use]
    pub const fn is_open(&self) -> bool {
        self.open
    }

    /// Handles the press edge: captures the cursor as the anchor, freezes
    /// the bound sectors and opens the ring right away. Without bindings or
    /// without a readable cursor the hold stays a plain swipe and nothing
    /// is sent.
    pub fn on_press(&mut self) {
        debug!(
            enabled = self.config.enabled,
            should_open = self.should_open(),
            "pie: press"
        );
        if !self.should_open() {
            return;
        }
        let Some(anchor) = (self.fetch_anchor)() else {
            debug!("pie: cursor position unavailable, the hold stays a plain swipe");
            return;
        };
        self.anchor = Some(anchor);
        self.sectors = self.bound_sectors();
        self.open = true;
        debug!(?anchor, sectors = self.sectors.len(), "pie: open");
        self.send(PieEvent::Open {
            anchor,
            sectors: self.sectors.clone()
        });
    }

    /// Advances the state machine one poll while the button is held.
    ///
    /// While open, a cursor further than the activation radius from the
    /// anchor selects the sector it points at and a change emits
    /// [`PieEvent::Highlight`]; a cursor inside the gate leaves the
    /// selection to [`Self::on_swipe`]. A tick without a readable cursor
    /// keeps the previous state.
    pub fn on_tick(&mut self) {
        if !self.open {
            return;
        }
        let (Some(cursor), Some(anchor)) = ((self.fetch_anchor)(), self.anchor) else {
            return;
        };
        let vector = (
            cursor.0.saturating_sub(anchor.0),
            cursor.1.saturating_sub(anchor.1)
        );
        if !beyond_radius(vector.0, vector.1, self.config.activation_radius) {
            return;
        }
        let sector = sector_for(vector.0, vector.1, self.config.dead_zone);
        if sector != self.highlight {
            self.highlight = sector;
            self.send(PieEvent::Highlight {
                direction: sector
            });
        }
    }

    /// Folds a committed swipe direction into the open pie: the direction's
    /// sector lights up, so scrolling the wheel selects without moving the
    /// mouse. A direction that is not a bound sector still highlights — the
    /// release simply commits nothing.
    ///
    /// # Arguments
    ///
    /// * `direction` - The direction the swipe accumulator just committed.
    pub fn on_swipe(&mut self, direction: GestureDirection) {
        if !self.open || self.highlight == Some(direction) {
            return;
        }
        self.highlight = Some(direction);
        debug!(?direction, "pie: swipe highlight");
        self.send(PieEvent::Highlight {
            direction: Some(direction)
        });
    }

    /// Settles the hold on release.
    ///
    /// If the pie was open, the highlighted sector commits when it has a
    /// bound action, [`PieEvent::Close`] is emitted and the state resets.
    /// A hold whose pie never opened (disabled, unbound, or no readable
    /// cursor) returns `None` and sends nothing, leaving the release to the
    /// swipe runtime.
    ///
    /// # Returns
    ///
    /// The direction whose action the host must fire, if any.
    pub fn on_release(&mut self) -> Option<GestureDirection> {
        if !self.open {
            self.reset();
            return None;
        }
        let committed = self.highlight.filter(|dir| self.is_bound(*dir));
        self.send(PieEvent::Close {
            committed
        });
        self.reset();
        committed
    }

    /// Aborts a hold without committing, e.g. on device detach or daemon
    /// shutdown. An open pie closes with `committed: None`; an idle runtime
    /// stays silent.
    pub fn on_cancel(&mut self) {
        if self.open {
            self.send(PieEvent::Close {
                committed: None
            });
        }
        self.reset();
    }

    fn reset(&mut self) {
        self.anchor = None;
        self.sectors.clear();
        self.open = false;
        self.highlight = None;
    }

    fn focused_app(&self) -> Option<String> {
        self.focus
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn effective_bindings(&self) -> Vec<Gesture> {
        merge_effective(
            &self.bindings,
            &self.profiles,
            self.focused_app().as_deref()
        )
    }

    fn bound_sectors(&self) -> Vec<PieSector> {
        self.effective_bindings()
            .iter()
            .filter(|gesture| gesture.direction != GestureDirection::None)
            .map(|gesture| PieSector {
                direction: gesture.direction,
                label:     format!("{:?}", gesture.direction)
            })
            .collect()
    }

    fn is_bound(&self, direction: GestureDirection) -> bool {
        self.effective_bindings()
            .iter()
            .any(|gesture| gesture.direction == direction)
    }

    fn send(&self, event: PieEvent) {
        match &self.hub {
            Some(hub) => {
                debug!(
                    ?event,
                    subscribers = hub.receiver_count(),
                    "pie: event published"
                );
                let _ = hub.send(event);
            }
            None => {
                debug!(?event, "pie: event dropped, no hub attached");
            }
        }
    }
}

/// Whether a cursor vector left the activation circle: strictly further than
/// `radius` from the anchor, in saturating integer arithmetic.
fn beyond_radius(dx: i32, dy: i32, radius: i32) -> bool {
    let (abs_x, abs_y) = (
        i64::from(dx.saturating_abs()),
        i64::from(dy.saturating_abs())
    );
    let radius = i64::from(radius.max(0));
    abs_x * abs_x + abs_y * abs_y > radius * radius
}

#[cfg(test)]
#[path = "pie_tests.rs"]
mod pie_tests;
