// SPDX-FileCopyrightText: 2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Human labels painted on pie wedges: action summaries and the wire view
//! of a resolved ring.

use logi_mx_driver::{config::PieSlot, prelude::Action};
use serde::{Deserialize, Serialize};

/// One wedge of the ring as advertised on the wire.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PieSector {
    /// Zero-based wedge index: 0 at 12 o'clock, running clockwise.
    pub index:   u8,
    /// Human summary painted on the sector, e.g. `Meta+E` or `alacritty`.
    pub label:   String,
    /// Raw shell command when the action launches a program, `None`
    /// otherwise; the overlay resolves it into an application icon.
    #[serde(default)]
    pub command: Option<String>,
    /// Whether the slot carries an action; empty slots render dim and
    /// commit nothing.
    pub bound:   bool
}

/// Builds the wire view of a ring: one entry per slot in wedge order.
///
/// # Arguments
///
/// * `ring` - The resolved slots, at most `PieConfig::MAX_SLOTS`.
///
/// # Returns
///
/// Sector descriptors with labels taken from the slot override or the
/// action summary.
#[must_use]
pub fn sectors_view(ring: &[PieSlot]) -> Vec<PieSector> {
    ring.iter()
        .enumerate()
        .map(|(index, slot)| {
            let command = match slot.action.as_ref() {
                Some(Action::Command {
                    command
                }) => Some(command.clone()),
                _ => None
            };
            let label = slot
                .label
                .clone()
                .unwrap_or_else(|| slot.action.as_ref().map_or(String::new(), action_summary));
            PieSector {
                index: u8::try_from(index).unwrap_or(u8::MAX),
                label,
                command,
                bound: slot.is_bound()
            }
        })
        .collect()
}

/// Short human summary of a bound action, painted on the pie sector.
///
/// Key sequences collapse to a combo (`Meta+E`), commands show the program
/// name, and the remaining actions get a fixed name.
///
/// # Arguments
///
/// * `action` - The action bound to a gesture direction.
///
/// # Returns
///
/// A single-line label of at most 17 characters.
///
/// # Examples
///
/// ```
/// use logi_mx_daemon::pie::labels::action_summary;
/// use logi_mx_driver::prelude::Action;
///
/// let keys = Action::Keypress {
///     keys: vec!["KEY_LEFTMETA".to_string(), "KEY_E".to_string()]
/// };
/// assert_eq!(action_summary(&keys), "Meta+E");
/// let cmd = Action::Command {
///     command: "/usr/bin/alacritty --working-dir ~".to_string()
/// };
/// assert_eq!(action_summary(&cmd), "alacritty");
/// ```
#[must_use]
pub fn action_summary(action: &Action) -> String {
    match action {
        Action::Keypress {
            keys
        } => keys
            .iter()
            .map(|key| pretty_key(key))
            .collect::<Vec<String>>()
            .join("+"),
        Action::Command {
            command
        } => {
            let first = command.split_whitespace().next().unwrap_or(command);
            let base = first.rsplit('/').next().unwrap_or(first);
            let mut summary: String = base.chars().take(16).collect();
            if base.chars().count() > 16 {
                summary.push('…');
            }
            summary
        }
        Action::Gestures {
            ..
        } => "Gestures".to_string(),
        Action::ToggleSmartShift => "SmartShift".to_string(),
        Action::None => String::new()
    }
}

/// Renders one evdev key name as a short label: `KEY_LEFTMETA` becomes
/// `Meta`, `KEY_F5` becomes `F5`, `KEY_A` becomes `A`.
fn pretty_key(name: &str) -> String {
    match name.trim_start_matches("KEY_") {
        "LEFTMETA" | "RIGHTMETA" => "Meta".to_string(),
        "LEFTCTRL" | "RIGHTCTRL" => "Ctrl".to_string(),
        "LEFTSHIFT" | "RIGHTSHIFT" => "Shift".to_string(),
        "LEFTALT" | "RIGHTALT" => "Alt".to_string(),
        "SPACE" => "Space".to_string(),
        core => {
            let lower = core.to_lowercase();
            let mut chars = lower.chars();
            chars.next().map_or(String::new(), |c| {
                c.to_uppercase().collect::<String>() + chars.as_str()
            })
        }
    }
}
