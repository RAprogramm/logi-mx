// SPDX-FileCopyrightText: 2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Ring resolution for one press.
//!
//! The slots come from the focused application's profile, the global
//! configuration, or a seed of the effective gesture bindings so legacy
//! setups keep working.

use std::{collections::HashMap, hash::BuildHasher};

use logi_mx_driver::{
    config::{PieConfig, PieProfile, PieSlot},
    prelude::{Gesture, GestureDirection}
};

/// Compass order used to seed a ring from gesture bindings: slot 0 at 12
/// o'clock, running clockwise.
const COMPASS_ORDER: [GestureDirection; 8] = [
    GestureDirection::Up,
    GestureDirection::UpRight,
    GestureDirection::Right,
    GestureDirection::DownRight,
    GestureDirection::Down,
    GestureDirection::DownLeft,
    GestureDirection::Left,
    GestureDirection::UpLeft
];

/// Resolves the ring the pie shows for one press.
///
/// The focused app's profile wins when it carries slots; otherwise the
/// global slots do. When neither is configured the ring is seeded from the
/// effective gesture bindings in compass order, so a setup that only knows
/// swipe bindings still gets a sensible wheel. The result is capped at
/// [`PieConfig::MAX_SLOTS`] wedges.
///
/// # Arguments
///
/// * `config` - Global pie configuration with its explicit slots.
/// * `profile` - The focused application's profile, if any.
/// * `gestures` - Effective gesture bindings for the press scope.
///
/// # Examples
///
/// ```
/// use logi_mx_daemon::pie::ring::ring_slots;
/// use logi_mx_driver::config::{PieConfig, PieSlot};
///
/// let config = PieConfig {
///     sectors: vec![PieSlot::default(); 3],
///     ..PieConfig::default()
/// };
/// assert_eq!(
///     ring_slots(&config, None, &[]).len(),
///     3,
///     "explicit slots win"
/// );
/// assert_eq!(ring_slots(&PieConfig::default(), None, &[]).len(), 0);
/// ```
#[must_use]
pub fn ring_slots(
    config: &PieConfig,
    profile: Option<&PieProfile>,
    gestures: &[Gesture]
) -> Vec<PieSlot> {
    let chosen = profile
        .filter(|profile| !profile.sectors.is_empty())
        .map(|profile| &profile.sectors)
        .or_else(|| (!config.sectors.is_empty()).then_some(&config.sectors));
    if let Some(slots) = chosen {
        return slots.iter().take(PieConfig::MAX_SLOTS).cloned().collect();
    }
    seed_from_gestures(gestures)
}

/// Builds a ring from directional gesture bindings, one slot per direction
/// in [`COMPASS_ORDER`], capped at [`PieConfig::MAX_SLOTS`].
fn seed_from_gestures(gestures: &[Gesture]) -> Vec<PieSlot> {
    COMPASS_ORDER
        .iter()
        .filter_map(|direction| {
            gestures
                .iter()
                .find(|gesture| gesture.direction == *direction)
                .map(|gesture| PieSlot {
                    action: Some(gesture.action.as_ref().clone()),
                    label:  None
                })
        })
        .take(PieConfig::MAX_SLOTS)
        .collect()
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

#[cfg(test)]
#[path = "ring_tests.rs"]
mod tests;
