// SPDX-FileCopyrightText: 2025-2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! The stored gesture bindings: which scope is being edited, the mapping
//! mirrors and the requests each edit turns into, plus the human-readable
//! rendering of a bound key chord.

use std::{cell::RefCell, collections::HashMap, rc::Rc};

use logi_mx_daemon::ipc;
use logi_mx_driver::devices::{Action, Gesture, GestureDirection, GestureMode};

use super::tile::GestureTile;

/// Gesture tiles in display order: direction and human label. The four
/// diagonals only ever fire from the radial pie menu, but they bind through
/// the same store and IPC path as the cardinals.
pub(super) const GESTURE_TILES: [(GestureDirection, &str); 9] = [
    (GestureDirection::Up, "Swipe up"),
    (GestureDirection::Down, "Swipe down"),
    (GestureDirection::Left, "Swipe left"),
    (GestureDirection::Right, "Swipe right"),
    (GestureDirection::UpLeft, "Swipe up-left"),
    (GestureDirection::UpRight, "Swipe up-right"),
    (GestureDirection::DownLeft, "Swipe down-left"),
    (GestureDirection::DownRight, "Swipe down-right"),
    (GestureDirection::None, "Click")
];

/// Fire moments offered on every gesture tile.
pub(super) const GESTURE_MODES: [&str; 2] = ["On press", "On release"];

/// Action kinds offered on every gesture tile.
pub(super) const GESTURE_KINDS: [&str; 2] = ["Keys", "Command"];

/// Shared registry of the built gesture tiles.
pub(super) type GestureTiles = Rc<RefCell<HashMap<GestureDirection, GestureTile>>>;

/// Which binding set the tiles currently edit.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(super) enum BindingScope {
    /// The global bindings, active when no profile matches.
    #[default]
    Global,
    /// A per-application profile keyed by the Hyprland window class.
    App(String)
}

/// Everything the Gestures page knows about the stored bindings.
#[derive(Default)]
pub(super) struct GestureStore {
    pub(super) global:   Vec<Gesture>,
    pub(super) profiles: HashMap<String, Vec<Gesture>>,
    pub(super) scope:    BindingScope
}

impl GestureStore {
    /// The bindings visible in the currently selected scope.
    pub(super) fn bindings(&self) -> &[Gesture] {
        match &self.scope {
            BindingScope::Global => &self.global,
            BindingScope::App(app) => self.profiles.get(app).map_or(&[], Vec::as_slice)
        }
    }

    /// The request that binds `action` in the current scope.
    pub(super) fn bind_request(
        &self,
        direction: GestureDirection,
        mode: GestureMode,
        action: Action
    ) -> ipc::Request {
        match &self.scope {
            BindingScope::Global => ipc::Request::SetGesture {
                direction,
                mode,
                action: Box::new(action)
            },
            BindingScope::App(app_id) => ipc::Request::SetAppGesture {
                app_id: app_id.clone(),
                direction,
                mode,
                action: Box::new(action)
            }
        }
    }

    /// The request that unbinds `direction` in the current scope.
    pub(super) fn clear_request(&self, direction: GestureDirection) -> ipc::Request {
        match &self.scope {
            BindingScope::Global => ipc::Request::ClearGesture {
                direction
            },
            BindingScope::App(app_id) => ipc::Request::ClearAppGesture {
                app_id: app_id.clone(),
                direction
            }
        }
    }

    /// Mirrors a successful bind locally so scope reloads stay accurate.
    pub(super) fn record(
        &mut self,
        direction: GestureDirection,
        mode: GestureMode,
        action: Action
    ) {
        let list = match &self.scope {
            BindingScope::Global => &mut self.global,
            BindingScope::App(app) => self.profiles.entry(app.clone()).or_default()
        };
        list.retain(|gesture| gesture.direction != direction);
        list.push(Gesture {
            direction,
            mode,
            action: Box::new(action)
        });
    }

    /// Mirrors a successful clear locally.
    pub(super) fn forget(&mut self, direction: GestureDirection) {
        match &self.scope {
            BindingScope::Global => self.global.retain(|gesture| gesture.direction != direction),
            BindingScope::App(app) => {
                if let Some(list) = self.profiles.get_mut(app) {
                    list.retain(|gesture| gesture.direction != direction);
                    if list.is_empty() {
                        self.profiles.remove(app);
                    }
                }
            }
        }
    }
}

/// Shared handle to the gesture binding store.
pub(super) type StoreCell = Rc<RefCell<GestureStore>>;

/// Pending key capture: the direction being bound and the evdev codes
/// pressed so far, in press order.
pub(super) type GestureCapture = Rc<RefCell<Option<(GestureDirection, Vec<u16>)>>>;

/// Renders a key sequence as one line, e.g. `Left Ctrl + Right`.
pub(super) fn binding_text(keys: &[String]) -> String {
    keys.iter()
        .map(|name| pretty_key(name))
        .collect::<Vec<_>>()
        .join(" + ")
}

/// Human form of one evdev key name, e.g. `KEY_LEFTCTRL` → `Left Ctrl`.
pub(super) fn pretty_key(name: &str) -> String {
    name.trim_start_matches("KEY_")
        .split('_')
        .map(|word| {
            let lower = word.to_lowercase();
            let mut chars = lower.chars();
            let head: String = chars
                .next()
                .map_or(String::new(), |c| c.to_uppercase().collect());
            head + chars.as_str()
        })
        .collect::<Vec<_>>()
        .join(" ")
}
