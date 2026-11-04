// SPDX-FileCopyrightText: 2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! The hold state machine: press/charge/open/select/commit transitions
//! driven by the host with an explicit clock and an injected cursor reader,
//! publishing [`PieEvent`]s on the [`PieHub`] fan-out.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex, PoisonError},
    time::{Duration, Instant}
};

use logi_mx_driver::{
    config::{PieConfig, PieProfile, PieSlot},
    prelude::{Gesture, GestureDirection}
};
use tracing::debug;

use super::{
    PieEvent, PieHub, PieRelease,
    geometry::{
        beyond_radius, direction_angle, slot_for_angle, slot_index, wedge_angle, wrapped_distance
    },
    labels::sectors_view,
    merge_effective,
    ring::ring_slots
};
use crate::focus::SharedFocus;

/// How long the gesture button must stay pressed before the charge ring
/// blooms into the full menu.
pub const HOLD_DELAY: Duration = Duration::from_millis(250);
/// How long after a released tap a fresh press counts as a double click
/// and opens the menu without charging first.
///
/// Matches the GNOME default `gtk-double-click-time`; the tap's own action
/// is deferred by this window so the two never fire together.
pub const DOUBLE_CLICK_WINDOW: Duration = Duration::from_millis(400);
/// Minimum aim movement, in radians, that republishes [`PieEvent::Aim`].
const AIM_EPS: f64 = 0.02;

/// Maps a wedge index to its wire width; rings never exceed `u8`.
fn wire_index(index: Option<usize>) -> Option<u8> {
    index.and_then(|value| u8::try_from(value).ok())
}

/// Cursor reader seam: returns the current screen position, `None` when the
/// desktop cannot be queried. Tests inject a scripted reader; production
/// uses [`crate::hyprbind::fetch_cursor_pos`].
type AnchorFetch = Box<dyn Fn() -> Option<(i32, i32)> + Send>;

/// Pie menu state machine for one gesture hold.
///
/// Owns the tuning, the resolved ring (global slots, per-app profile, or a
/// seed of the effective gesture bindings), the hold state (anchor,
/// charging/open flags, last highlight, last tap) and the event fan-out.
/// The host drives it with [`Self::on_press`], [`Self::on_swipe`] when a
/// swipe commits, [`Self::on_tick`] per poll while the button is held, and
/// [`Self::on_release`] or [`Self::on_cancel`] to settle the hold.
///
/// A press with the pie enabled charges a thin ring at the cursor; after
/// [`HOLD_DELAY`] the ring blooms into the menu, and a press within
/// [`DOUBLE_CLICK_WINDOW`] of a released tap skips straight to the menu.
/// Selection then follows whichever input moves: a committed swipe direction
/// highlights its wedge, and a cursor further than the activation radius
/// from the anchor highlights the wedge under its vector. A swipe that
/// commits before the ring opens is an expert mark: [`Self::on_cancel`]
/// drops the menu for that hold so the directional binding owns it.
///
/// # Examples
///
/// ```
/// use logi_mx_daemon::pie::PieRuntime;
/// use logi_mx_driver::config::PieConfig;
///
/// let mut pie = PieRuntime::with_fetch(|| Some((0, 0)));
/// pie.set_config(PieConfig {
///     enabled: true,
///     ..PieConfig::default()
/// });
/// assert!(!pie.should_open(), "an empty ring never opens");
/// ```
pub struct PieRuntime {
    config:       PieConfig,
    profiles:     HashMap<String, PieProfile>,
    focus:        SharedFocus,
    bindings:     Vec<Gesture>,
    gestures:     HashMap<String, Vec<Gesture>>,
    fetch_anchor: AnchorFetch,
    hub:          Option<PieHub>,
    anchor:       Option<(i32, i32)>,
    ring:         Vec<PieSlot>,
    charging:     bool,
    charge_since: Option<Instant>,
    last_tap:     Option<Instant>,
    open:         bool,
    highlight:    Option<usize>,
    last_aim:     Option<f64>
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
            profiles:     HashMap::new(),
            focus:        Arc::new(Mutex::new(None)),
            bindings:     Vec::new(),
            gestures:     HashMap::new(),
            fetch_anchor: Box::new(fetch),
            hub:          None,
            anchor:       None,
            ring:         Vec::new(),
            charging:     false,
            charge_since: None,
            last_tap:     None,
            open:         false,
            highlight:    None,
            last_aim:     None
        }
    }

    /// Attaches the event fan-out the state machine publishes on.
    pub fn attach_hub(&mut self, hub: PieHub) {
        self.hub = Some(hub);
    }

    /// Attaches the shared focus slot consulted when resolving the ring.
    pub fn attach_focus(&mut self, focus: SharedFocus) {
        self.focus = focus;
    }

    /// Replaces the pie tuning, including the global ring slots.
    pub fn set_config(&mut self, config: PieConfig) {
        self.config = config;
    }

    /// Replaces the per-application pie profiles.
    pub fn set_pie_profiles(&mut self, profiles: HashMap<String, PieProfile>) {
        self.profiles = profiles;
    }

    /// Replaces the global gesture bindings and per-app gesture profiles the
    /// ring seeds itself from when no explicit slots exist; the host keeps
    /// them in sync with the gesture runtime.
    pub fn set_bindings(
        &mut self,
        bindings: Vec<Gesture>,
        gestures: HashMap<String, Vec<Gesture>>
    ) {
        self.bindings = bindings;
        self.gestures = gestures;
    }

    /// Whether a press may open the pie: enabled and a ring of at least
    /// [`PieConfig::MIN_SLOTS`] wedges resolved for the current scope.
    #[must_use]
    pub fn should_open(&self) -> bool {
        self.config.enabled && self.resolve_ring().len() >= PieConfig::MIN_SLOTS
    }

    /// Whether the pie ring is currently on screen.
    #[must_use]
    pub const fn is_open(&self) -> bool {
        self.open
    }

    /// The ring resolved for the current focus scope: explicit slots or the
    /// gesture seed, in wedge order.
    #[must_use]
    pub fn resolve_ring(&self) -> Vec<PieSlot> {
        let focused = self.focused_app();
        let profile = focused.as_deref().and_then(|app| self.profiles.get(app));
        ring_slots(&self.config, profile, &self.effective_bindings())
    }

    /// Handles the press edge: captures the cursor as the anchor, snapshots
    /// the ring for the press scope, and either starts the charge ring or,
    /// on a double click, opens the menu right away. Without a ring of at
    /// least two wedges or without a readable cursor the hold stays a plain
    /// swipe and nothing is sent.
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
        self.ring = self.resolve_ring();
        let double_click = self
            .last_tap
            .is_some_and(|tap| now.saturating_duration_since(tap) <= DOUBLE_CLICK_WINDOW);
        self.last_tap = None;
        if double_click {
            self.open_menu();
        } else {
            self.charging = true;
            self.charge_since = Some(now);
            debug!(?anchor, "pie: charge");
            self.send(PieEvent::Charge {
                anchor
            });
        }
    }

    /// Blooms the charge ring into the full menu: flips the state and emits
    /// [`PieEvent::Open`] with the ring snapshotted at press.
    fn open_menu(&mut self) {
        self.charging = false;
        self.charge_since = None;
        self.open = true;
        if let Some(anchor) = self.anchor {
            debug!(?anchor, wedges = self.ring.len(), "pie: open");
            self.send(PieEvent::Open {
                anchor,
                sectors: sectors_view(&self.ring)
            });
        }
    }

    /// Advances the state machine one poll while the button is held.
    ///
    /// While charging, a hold that passed [`HOLD_DELAY`] blooms into the
    /// open menu and emits [`PieEvent::Open`]. While open, a cursor further
    /// than the gate from the anchor selects the wedge under its vector and
    /// a change emits [`PieEvent::Highlight`]; a cursor inside the gate
    /// leaves the selection to [`Self::on_swipe`]. A tick without a readable
    /// cursor keeps the previous state.
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
                self.open_menu();
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
        let angle = f64::from(vector.1).atan2(f64::from(vector.0));
        if self
            .last_aim
            .is_none_or(|previous| wrapped_distance(angle, previous) > AIM_EPS)
        {
            self.last_aim = Some(angle);
            self.send(PieEvent::Aim {
                angle
            });
        }
        let count = self.ring.len();
        let slot = slot_index(vector.0, vector.1, count);
        if slot != self.highlight {
            self.highlight = slot;
            self.send(PieEvent::Highlight {
                index: wire_index(slot)
            });
        }
    }

    /// Folds a committed swipe direction into the open pie: the wedge that
    /// direction falls into lights up, so a swipe selects without moving the
    /// cursor to the gate. An empty wedge highlights too — the release
    /// simply commits nothing.
    ///
    /// # Arguments
    ///
    /// * `direction` - The direction the swipe accumulator just committed.
    pub fn on_swipe(&mut self, direction: GestureDirection) {
        if !self.open {
            return;
        }
        let Some(centre) = direction_angle(direction) else {
            return;
        };
        if self.last_aim != Some(centre) {
            self.last_aim = Some(centre);
            self.send(PieEvent::Aim {
                angle: centre
            });
        }
        let slot = slot_for_angle(centre, self.ring.len());
        if slot == self.highlight {
            return;
        }
        self.highlight = slot;
        debug!(?direction, ?slot, "pie: swipe highlight");
        self.send(PieEvent::Highlight {
            index: wire_index(slot)
        });
    }

    /// Folds a wheel scroll into the open pie: every detent cycles the
    /// selection to the next wedge, wrapping around the ring. Scrolling from
    /// no selection enters the first wedge in the scroll direction.
    ///
    /// # Arguments
    ///
    /// * `detents` - Wheel detents since the last poll; positive scrolls
    ///   clockwise, negative counter-clockwise.
    pub fn on_scroll(&mut self, detents: i32) {
        if !self.open || detents == 0 {
            return;
        }
        let count = self.ring.len();
        if count == 0 {
            return;
        }
        let count = i64::try_from(count).unwrap_or(1);
        let base = self
            .highlight
            .map_or(if detents >= 0 { -1 } else { count }, |index| {
                i64::try_from(index).unwrap_or(0)
            });
        let next = base.saturating_add(i64::from(detents)).rem_euclid(count);
        let Some(index) = usize::try_from(next).ok() else {
            return;
        };
        let angle = wedge_angle(index, self.ring.len());
        if self.last_aim != Some(angle) {
            self.last_aim = Some(angle);
            self.send(PieEvent::Aim {
                angle
            });
        }
        if self.highlight != Some(index) {
            self.highlight = Some(index);
            debug!(detents, index, "pie: wheel highlight");
            self.send(PieEvent::Highlight {
                index: wire_index(Some(index))
            });
        }
    }

    /// Settles the hold on release and reports what the host must do.
    ///
    /// An open pie commits the highlighted wedge when it is bound and emits
    /// [`PieEvent::Close`]; a hold that ended while charging is a tap — the
    /// charge ring closes and [`PieRelease::Charging`] asks the host to
    /// delay the click binding so a second press can become a double click.
    /// A hold that never engaged returns [`PieRelease::Idle`] and sends
    /// nothing.
    ///
    /// # Arguments
    ///
    /// * `now` - Monotonic time of the release edge.
    ///
    /// # Returns
    ///
    /// The release outcome for the host.
    pub fn on_release(&mut self, now: Instant) -> PieRelease {
        if self.charging
            && self
                .charge_since
                .is_some_and(|start| now.saturating_duration_since(start) >= HOLD_DELAY)
        {
            self.open_menu();
        }
        if self.open {
            let committed = self
                .highlight
                .filter(|index| self.ring.get(*index).is_some_and(PieSlot::is_bound));
            let action = committed
                .and_then(|index| self.ring.get(index))
                .and_then(|slot| slot.action.clone());
            self.send(PieEvent::Close {
                committed: wire_index(committed)
            });
            self.reset();
            return PieRelease::Committed(action);
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

    /// Aborts a hold without committing: an expert mark that fired a swipe
    /// binding, a device detach, or daemon shutdown. A charging or open pie
    /// closes with `committed: None`; an idle runtime stays silent.
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
        self.ring.clear();
        self.charging = false;
        self.charge_since = None;
        self.open = false;
        self.highlight = None;
        self.last_aim = None;
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
            &self.gestures,
            self.focused_app().as_deref()
        )
    }

    fn send(&self, event: PieEvent) {
        match &self.hub {
            Some(hub) => {
                debug!(
                    ?event,
                    subscribers = hub.receiver_count(),
                    "pie: event published"
                );
                hub.send(event);
            }
            None => {
                debug!(?event, "pie: event dropped, no hub attached");
            }
        }
    }
}

#[cfg(test)]
#[path = "runtime_tests.rs"]
mod tests;
