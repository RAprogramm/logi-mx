// SPDX-FileCopyrightText: 2025-2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Validation gates shared by the RPC handlers: every rejection message a
//! malformed request can produce, checked before anything is persisted.

use logi_mx_driver::prelude::{Action, ButtonId, PieConfig, PieSlot};

use crate::{ipc, worker::pool::DevicePool};

/// Validates a remap action, returning the rejection reason if invalid.
pub(super) fn action_rejection(action: &Action) -> Option<String> {
    match action {
        Action::Keypress {
            keys
        } => crate::inject::resolve_sequence(keys)
            .map_err(|e| e.to_string())
            .err(),
        Action::Command {
            command
        } => (command.trim().is_empty()).then(|| "command must not be empty".to_string()),
        _ => Some("action must be a key sequence or a command".to_string())
    }
}

/// Wraps a rejection message into the error reply every handler branch uses.
pub(super) const fn rejection(message: String) -> ipc::Response {
    ipc::Response::Error {
        message
    }
}

/// Rejects a pie ring the menu cannot act on, checked against the current
/// tuning so the gate rules and the slot cap are validated in one place.
///
/// More than [`PieConfig::MAX_SLOTS`] wedges would be too thin to hit;
/// fewer than [`PieConfig::MIN_SLOTS`] simply never opens and is legal to
/// store so a half-configured ring survives a restart.
pub(super) fn ring_rejection(config: &PieConfig, sectors: &[PieSlot]) -> Option<String> {
    PieConfig {
        sectors: sectors.to_vec(),
        ..config.clone()
    }
    .rejection()
}

/// Rejects everything but the two thumb side buttons: the firmware buttons
/// and the gesture control belong to other runtimes.
pub(super) fn thumb_button_rejection(button: ButtonId) -> Option<String> {
    (!matches!(button, ButtonId::Back | ButtonId::Forward))
        .then(|| "only the Back and Forward buttons can be remapped".to_string())
}

/// Whether the request edits the pie menu: tuning, global ring or profiles.
pub(super) const fn is_pie_request(request: &ipc::Request) -> bool {
    matches!(
        request,
        ipc::Request::SetPie { .. }
            | ipc::Request::SetPieSectors { .. }
            | ipc::Request::SetAppPieSectors { .. }
            | ipc::Request::RemoveAppPieProfile { .. }
    )
}

/// Validates a pie edit against the stored tuning before it is persisted.
///
/// The gate rules ride along from the stored configuration so a ring edit
/// cannot smuggle an unusable gate, and the slot cap keeps every wedge wide
/// enough to hit. Returns the rejection message, `None` when the edit is
/// legal.
pub(super) fn pie_edit_rejection(manager: &DevicePool, request: &ipc::Request) -> Option<String> {
    let current = manager.pie_config();
    match request {
        ipc::Request::SetPie {
            enabled,
            activation_radius,
            dead_zone
        } => PieConfig {
            enabled: *enabled,
            activation_radius: *activation_radius,
            dead_zone: *dead_zone,
            ..current
        }
        .rejection(),
        ipc::Request::SetPieSectors {
            sectors
        }
        | ipc::Request::SetAppPieSectors {
            sectors, ..
        } => ring_rejection(&current, sectors),
        _ => None
    }
}
