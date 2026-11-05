// SPDX-FileCopyrightText: 2025-2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Daemon-side binding views: gesture configuration, thumb button remaps
//! and the replayable Hyprland keybinds.

use logi_mx_daemon::ipc::{Request, Response, try_request};
use logi_mx_driver::prelude::*;
use masterror::prelude::*;

use crate::{args::GestureCommands, out::Out};

/// Prints or toggles the daemon-side gesture configuration.
pub fn cmd_gestures(action: Option<&GestureCommands>) -> Result<()> {
    match action {
        Some(GestureCommands::Enable) => set_gestures_enabled(true),
        Some(GestureCommands::Disable) => set_gestures_enabled(false),
        _ => show_gestures()
    }
}

/// Enables or disables software gesture recognition through the daemon.
fn set_gestures_enabled(enabled: bool) -> Result<()> {
    match try_request(&Request::SetGesturesEnabled {
        enabled
    }) {
        Some(Response::Ok) => {}
        Some(Response::Error {
            message
        }) => return Err(AppError::internal(message)),
        _ => return Err(AppError::internal("Daemon is not reachable"))
    }
    let mut out = Out::new();
    out.line(format_args!(
        "Gestures {}.",
        if enabled { "enabled" } else { "disabled" }
    ))?;
    out.finish()
}

/// Renders one gesture binding as `DIRECTION (mode): action`.
fn gesture_line(gesture: &Gesture) -> String {
    let action = match gesture.action.as_ref() {
        Action::Keypress {
            keys
        } => keys.join("+"),
        Action::Command {
            command
        } => format!("$ {command}"),
        other => format!("{other:?}")
    };
    format!("{:?} ({:?}): {action}", gesture.direction, gesture.mode)
}

/// Prints the stored gesture opt-in, every binding and the per-app profiles.
fn show_gestures() -> Result<()> {
    let Some(Response::Gestures {
        enabled,
        gestures,
        profiles,
        ..
    }) = try_request(&Request::GetGestures)
    else {
        return Err(AppError::internal(
            "Daemon is not reachable; start it with `systemctl --user start logi-mx-daemon`"
        ));
    };
    let mut out = Out::new();
    out.line(format_args!(
        "Gestures: {}",
        if enabled { "enabled" } else { "disabled" }
    ))?;
    if gestures.is_empty() {
        out.line(format_args!("  No bindings"))?;
    }
    for gesture in &gestures {
        out.line(format_args!("  {}", gesture_line(gesture)))?;
    }
    let mut apps: Vec<&String> = profiles.keys().collect();
    apps.sort_unstable();
    for app in apps {
        out.line(format_args!("[{app}]"))?;
        for gesture in &profiles[app] {
            out.line(format_args!("  {}", gesture_line(gesture)))?;
        }
    }
    out.finish()
}

/// Renders one thumb button remap action as one line.
fn button_action_line(action: &Action) -> String {
    match action {
        Action::Keypress {
            keys
        } => keys.join("+"),
        Action::Command {
            command
        } => format!("$ {command}"),
        other => format!("{other:?}")
    }
}

/// Prints the stored thumb button software remap: the global map and every
/// per-application profile; unbound buttons print as `native`.
pub fn show_button_config() -> Result<()> {
    let (buttons, profiles) = match try_request(&Request::GetButtons) {
        Some(Response::ButtonsConfig {
            buttons,
            profiles
        }) => (buttons, profiles),
        Some(Response::Error {
            message
        }) => return Err(AppError::internal(message)),
        _ => {
            return Err(AppError::internal(
                "Daemon is not reachable; start it with `systemctl --user start logi-mx-daemon`"
            ));
        }
    };
    let mut out = Out::new();
    out.line(format_args!("Thumb button remap:"))?;
    let render =
        |map: &std::collections::HashMap<ButtonId, Action>, button: ButtonId, out: &mut Out| {
            let action = map
                .get(&button)
                .map_or_else(|| "native".to_string(), button_action_line);
            out.line(format_args!("  {button:?}: {action}"))
        };
    for button in [ButtonId::Back, ButtonId::Forward] {
        render(&buttons, button, &mut out)?;
    }
    let mut apps: Vec<&String> = profiles.keys().collect();
    apps.sort_unstable();
    for app in apps {
        out.line(format_args!("[{app}]"))?;
        for button in [ButtonId::Back, ButtonId::Forward] {
            let profile = &profiles[app];
            // A button the profile omits falls back to the global binding
            // at runtime, so print that binding and mark it inherited.
            let inherited = !profile.contains_key(&button);
            let action = profile
                .get(&button)
                .or_else(|| buttons.get(&button))
                .map_or_else(|| "native".to_string(), button_action_line);
            let mark = if inherited && action != "native" {
                " (global)"
            } else {
                ""
            };
            out.line(format_args!("  {button:?}: {action}{mark}"))?;
        }
    }
    out.finish()
}

/// Prints the active Hyprland keyboard bindings the daemon can replay.
///
/// Each line pairs the replayable chord with the binding's human label; the
/// chords are what `Action::Keypress` stores when a gesture adopts a bind.
pub fn cmd_keybinds() -> Result<()> {
    let entries = match try_request(&Request::GetKeybinds) {
        Some(Response::Keybinds {
            entries
        }) => entries,
        Some(Response::Error {
            message
        }) => return Err(AppError::internal(message)),
        _ => {
            return Err(AppError::internal(
                "Daemon is not reachable; start it with `systemctl --user start logi-mx-daemon`"
            ));
        }
    };
    let mut out = Out::new();
    out.line(format_args!("Hyprland keybinds: {}", entries.len()))?;
    let chord_width = entries
        .iter()
        .map(|entry| entry.keys.join("+").chars().count())
        .max()
        .unwrap_or(0);
    for entry in &entries {
        let chord = entry.keys.join("+");
        out.line(format_args!("  {chord:<chord_width$}  {}", entry.label))?;
    }
    out.finish()
}
