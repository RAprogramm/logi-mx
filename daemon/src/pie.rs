// SPDX-FileCopyrightText: 2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Radial pie menu runtime for the diverted gesture button.
//!
//! Pressing and holding the gesture button charges a thin ring around the
//! cursor; once the hold gate passes, the ring blooms into the full menu at
//! the cursor. A quick tap never charges: it releases the click binding
//! instead, and a second press within the double-click window skips the
//! charge and opens the menu straight away. The menu shows one sector per
//! bound direction, tiling the full circle. Selection follows the wheel — a
//! committed swipe direction highlights its sector — and, as a secondary
//! input, the cursor once it travels further than the activation radius
//! from the anchor. Releasing over a highlighted sector fires its action,
//! like a Dota-style radial menu. This module is the daemon-side half of
//! the feature: it runs the state machine and publishes [`PieEvent`] lines
//! for the overlay renderer over a [`PieHub`] broadcast, mirroring the live
//! telemetry fan-out.
//!
//! Every input is either a pure function ([`sector_among`]) or runs against
//! an injected cursor reader and an explicit clock, so the whole state
//! machine is testable without Hyprland and without sleeping. The press
//! point is captured through the Hyprland request socket
//! ([`crate::hyprbind::fetch_cursor_pos`]) because raw-XY deltas carry no
//! absolute position.
//!
//! Swipe semantics stay intact: while the pie is enabled, raw-XY travel
//! still feeds the swipe accumulator, so `OnPress`-mode swipe commits keep
//! firing mid-hold exactly as before. Only the release moment changes: an
//! open pie fires the highlighted sector's `OnRelease` bindings instead of
//! the swipe fallback, so one hold never fires twice.

use std::{
    collections::HashMap,
    hash::BuildHasher,
    sync::{Arc, Mutex, PoisonError},
    time::{Duration, Instant}
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
/// `Charge` starts the thin hold-gate ring on press, `Open` replaces it with
/// the full menu once the hold gate passes (or immediately on a double
/// click), `Highlight` marks every sector change while open (wheel swipe or
/// cursor), and `Close` ends the visual — exactly once per `Charge` or
/// `Open`, whichever engaged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PieEvent {
    /// A hold is charging at `anchor`; the overlay fills a thin ring.
    Charge {
        /// Press point in screen coordinates.
        anchor: (i32, i32)
    },
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

/// How long the gesture button must stay pressed before the charge ring
/// blooms into the full menu.
pub const HOLD_DELAY: Duration = Duration::from_millis(250);
/// How long after a released tap a fresh press counts as a double click
/// and opens the menu without charging first.
///
/// Matches the GNOME default `gtk-double-click-time`; the tap's own action
/// is deferred by this window so the two never fire together.
pub const DOUBLE_CLICK_WINDOW: Duration = Duration::from_millis(400);

/// What a released hold leaves for the host to settle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PieRelease {
    /// The pie was open; the host fires the committed direction's
    /// `OnRelease` bindings, if any, and suppresses the swipe fallback.
    Opened(Option<GestureDirection>),
    /// The hold ended while charging (a tap); the host settles a click
    /// binding, delayed so a second press can become a double click.
    Charging,
    /// The pie never engaged; the host settles the hold as a plain swipe.
    Idle
}

/// The centre angle of a direction in cairo radians, screen coordinates
/// (y grows downward): `Right` at 0, then clockwise in 45° steps.
///
/// Shared contract with the overlay: wedges are centred on these angles and
/// [`sector_among`] picks the nearest one, so the painted sector and the
/// selected sector always agree.
///
/// # Arguments
///
/// * `direction` - Sector direction; the click direction has no angle.
///
/// # Returns
///
/// The centre angle in `0..2π`, or `None` for [`GestureDirection::None`].
///
/// # Examples
///
/// ```
/// use logi_mx_daemon::pie::direction_angle;
/// use logi_mx_driver::devices::GestureDirection;
///
/// assert_eq!(direction_angle(GestureDirection::Right), Some(0.0));
/// assert!(
///     (direction_angle(GestureDirection::Up).unwrap_or(0.0) - 3.0 * std::f64::consts::FRAC_PI_2)
///         .abs()
///         < 1e-12
/// );
/// assert_eq!(direction_angle(GestureDirection::None), None);
/// ```
#[must_use]
pub fn direction_angle(direction: GestureDirection) -> Option<f64> {
    let steps: f64 = match direction {
        GestureDirection::Right => 0.0,
        GestureDirection::DownRight => 1.0,
        GestureDirection::Down => 2.0,
        GestureDirection::DownLeft => 3.0,
        GestureDirection::Left => 4.0,
        GestureDirection::UpLeft => 5.0,
        GestureDirection::Up => 6.0,
        GestureDirection::UpRight => 7.0,
        GestureDirection::None => return None
    };
    Some(steps * std::f64::consts::FRAC_PI_4)
}

/// Wraps an angle difference into `0..=π`, the shorter way around the circle.
fn wrapped_distance(angle: f64, centre: f64) -> f64 {
    let tau = std::f64::consts::TAU;
    let diff = (angle - centre).rem_euclid(tau);
    diff.min(tau - diff)
}

/// Selects the bound sector a cursor vector points at: the one whose centre
/// angle is nearest, so any number of sectors tiles the circle without
/// gaps and selection matches the painted wedges exactly.
///
/// # Arguments
///
/// * `dx` - Horizontal cursor offset from the anchor, positive rightward.
/// * `dy` - Vertical cursor offset from the anchor, positive downward.
/// * `sectors` - The bound sectors to choose among.
///
/// # Returns
///
/// The nearest sector's direction, or `None` for a zero vector or an empty
/// sector list.
///
/// # Examples
///
/// ```
/// use logi_mx_daemon::pie::{PieSector, sector_among};
/// use logi_mx_driver::devices::GestureDirection;
///
/// let sectors = vec![
///     PieSector {
///         direction: GestureDirection::Up,
///         label:     "Up".to_string()
///     },
///     PieSector {
///         direction: GestureDirection::Right,
///         label:     "Right".to_string()
///     },
/// ];
/// assert_eq!(sector_among(0, -50, &sectors), Some(GestureDirection::Up));
/// assert_eq!(sector_among(50, 0, &sectors), Some(GestureDirection::Right));
/// assert_eq!(sector_among(0, 0, &sectors), None);
/// ```
#[must_use]
pub fn sector_among(dx: i32, dy: i32, sectors: &[PieSector]) -> Option<GestureDirection> {
    if dx == 0 && dy == 0 {
        return None;
    }
    let angle = f64::from(dy).atan2(f64::from(dx));
    sectors
        .iter()
        .filter_map(|sector| {
            let centre = direction_angle(sector.direction)?;
            Some((wrapped_distance(angle, centre), sector.direction))
        })
        .min_by(|(left, _), (right, _)| left.total_cmp(right))
        .map(|(_, direction)| direction)
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
/// (anchor, charging/open flags, last highlight, last tap) and the event
/// fan-out. The host drives it with [`Self::on_press`], [`Self::on_swipe`]
/// when a swipe commits, [`Self::on_tick`] per poll while the button is
/// held, and [`Self::on_release`] or [`Self::on_cancel`] to settle the hold.
///
/// A press with the pie enabled charges a thin ring at the cursor; after
/// [`HOLD_DELAY`] the ring blooms into the menu, and a press within
/// [`DOUBLE_CLICK_WINDOW`] of a released tap skips straight to the menu.
/// Selection then follows whichever input moves: a committed swipe direction
/// highlights its sector, and a cursor further than the activation radius
/// from the anchor highlights the nearest sector. While the pie is
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
    charging:     bool,
    charge_since: Option<Instant>,
    last_tap:     Option<Instant>,
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
            charging:     false,
            charge_since: None,
            last_tap:     None,
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

    /// Handles the press edge: captures the cursor as the anchor and either
    /// starts the charge ring or, on a double click, opens the menu right
    /// away. Without bindings or without a readable cursor the hold stays a
    /// plain swipe and nothing is sent.
    ///
    /// # Arguments
    ///
    /// * `now` - Monotonic time of the press edge.
    pub fn on_press(&mut self, now: Instant) {
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
        let double_click = self
            .last_tap
            .is_some_and(|tap| now.saturating_duration_since(tap) <= DOUBLE_CLICK_WINDOW);
        self.last_tap = None;
        if double_click {
            self.open = true;
            debug!(
                ?anchor,
                sectors = self.sectors.len(),
                "pie: open (double click)"
            );
            self.send(PieEvent::Open {
                anchor,
                sectors: self.sectors.clone()
            });
        } else {
            self.charging = true;
            self.charge_since = Some(now);
            debug!(?anchor, "pie: charge");
            self.send(PieEvent::Charge {
                anchor
            });
        }
    }

    /// Advances the state machine one poll while the button is held.
    ///
    /// While charging, a hold that passed [`HOLD_DELAY`] blooms into the
    /// open menu and emits [`PieEvent::Open`]. While open, a cursor further
    /// than the activation radius from the anchor selects the nearest
    /// sector and a change emits [`PieEvent::Highlight`]; a cursor inside
    /// the gate leaves the selection to [`Self::on_swipe`]. A tick without
    /// a readable cursor keeps the previous state.
    ///
    /// # Arguments
    ///
    /// * `now` - Monotonic time of this poll.
    pub fn on_tick(&mut self, now: Instant) {
        if self.charging {
            if self
                .charge_since
                .is_some_and(|start| now.saturating_duration_since(start) >= HOLD_DELAY)
            {
                self.charging = false;
                self.charge_since = None;
                self.open = true;
                if let Some(anchor) = self.anchor {
                    debug!(?anchor, sectors = self.sectors.len(), "pie: open");
                    self.send(PieEvent::Open {
                        anchor,
                        sectors: self.sectors.clone()
                    });
                }
            }
            return;
        }
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
        let gate = self.config.activation_radius.max(self.config.dead_zone);
        if !beyond_radius(vector.0, vector.1, gate) {
            return;
        }
        let sector = sector_among(vector.0, vector.1, &self.sectors);
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

    /// Settles the hold on release and reports what the host must do.
    ///
    /// An open pie commits the highlighted sector when it has a bound
    /// action and emits [`PieEvent::Close`]; a hold that ended while
    /// charging is a tap — the charge ring closes and
    /// [`PieRelease::Charging`] asks the host to delay the click binding so
    /// a second press can become a double click. A hold that never engaged
    /// returns [`PieRelease::Idle`] and sends nothing.
    ///
    /// # Arguments
    ///
    /// * `now` - Monotonic time of the release edge.
    ///
    /// # Returns
    ///
    /// The release outcome for the host.
    pub fn on_release(&mut self, now: Instant) -> PieRelease {
        if self.open {
            let committed = self.highlight.filter(|dir| self.is_bound(*dir));
            self.send(PieEvent::Close {
                committed
            });
            self.reset();
            return PieRelease::Opened(committed);
        }
        if self.charging {
            self.send(PieEvent::Close {
                committed: None
            });
            self.reset();
            self.last_tap = Some(now);
            return PieRelease::Charging;
        }
        self.reset();
        PieRelease::Idle
    }

    /// Aborts a hold without committing, e.g. on device detach or daemon
    /// shutdown. A charging or open pie closes with `committed: None`; an
    /// idle runtime stays silent.
    pub fn on_cancel(&mut self) {
        if self.open || self.charging {
            self.send(PieEvent::Close {
                committed: None
            });
        }
        self.reset();
    }

    fn reset(&mut self) {
        self.anchor = None;
        self.sectors.clear();
        self.charging = false;
        self.charge_since = None;
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
