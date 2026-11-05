// SPDX-FileCopyrightText: 2025-2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Software remap of the two thumb side buttons: the stored per-scope
//! mappings, the editable rows and the per-application scope bar.

mod scope;
mod tile;

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc
};

use gtk4::{Box as GtkBox, FlowBox, Orientation, SelectionMode};
use libadwaita::{ApplicationWindow, prelude::*};
use logi_mx_daemon::ipc::{self, Response};
use logi_mx_driver::devices::{Action, ButtonId};

use self::{
    scope::{apply_scope, refresh_scope_model, remap_scope_bar},
    tile::{RemapTile, attach_remap_capture, remap_tile}
};
use super::report;
use crate::hud::toasts::Toasts;

/// Which remap set the thumb rows currently edit.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
enum RemapScope {
    /// The global mappings, active when no profile matches.
    #[default]
    Global,
    /// A per-application profile keyed by the Hyprland window class.
    App(String)
}

/// Everything the Buttons page knows about the stored software remaps.
#[derive(Default)]
struct ButtonStore {
    global:   HashMap<ButtonId, Action>,
    profiles: HashMap<String, HashMap<ButtonId, Action>>,
    scope:    RemapScope
}

impl ButtonStore {
    /// The mapping in force for a button in the currently selected scope.
    fn action(&self, button: ButtonId) -> Option<&Action> {
        match &self.scope {
            RemapScope::Global => self.global.get(&button),
            RemapScope::App(app) => self.profiles.get(app).and_then(|p| p.get(&button))
        }
    }

    /// The request that binds `action` to a button in the current scope.
    fn bind_request(&self, button: ButtonId, action: Action) -> ipc::Request {
        match &self.scope {
            RemapScope::Global => ipc::Request::SetButton {
                button,
                action: Box::new(action)
            },
            RemapScope::App(app_id) => ipc::Request::SetAppButton {
                app_id: app_id.clone(),
                button,
                action: Box::new(action)
            }
        }
    }

    /// The request that restores a button in the current scope.
    fn clear_request(&self, button: ButtonId) -> ipc::Request {
        match &self.scope {
            RemapScope::Global => ipc::Request::ClearButton {
                button
            },
            RemapScope::App(app_id) => ipc::Request::ClearAppButton {
                app_id: app_id.clone(),
                button
            }
        }
    }

    /// Mirrors a successful bind locally so scope reloads stay accurate.
    fn record(&mut self, button: ButtonId, action: Action) {
        match &mut self.scope {
            RemapScope::Global => {
                self.global.insert(button, action);
            }
            RemapScope::App(app) => {
                self.profiles
                    .entry(app.clone())
                    .or_default()
                    .insert(button, action);
            }
        }
    }

    /// Mirrors a successful clear locally.
    fn forget(&mut self, button: ButtonId) {
        match &mut self.scope {
            RemapScope::Global => {
                self.global.remove(&button);
            }
            RemapScope::App(app) => {
                if let Some(profile) = self.profiles.get_mut(app) {
                    profile.remove(&button);
                    if profile.is_empty() {
                        self.profiles.remove(app);
                    }
                }
            }
        }
    }
}

/// Shared handle to the button remap store.
type StoreCell = Rc<RefCell<ButtonStore>>;

/// Pending key capture: the button being bound and the evdev codes pressed
/// so far, in press order.
type RemapCapture = Rc<RefCell<Option<(ButtonId, Vec<u16>)>>>;

/// Shared registry of the built remap rows.
type RemapTiles = Rc<RefCell<HashMap<ButtonId, RemapTile>>>;

/// Thumb buttons offered for software remapping, with display labels.
const REMAP_BUTTONS: [(ButtonId, &str); 2] =
    [(ButtonId::Back, "Back"), (ButtonId::Forward, "Forward")];

/// Action kinds offered on every remap row.
const REMAP_KINDS: [&str; 3] = ["Native", "Keys", "Command"];

/// Builds the thumb remap section: the scope bar and one row per thumb side
/// button, editing the daemon-side software remap rather than the firmware
/// assignment above it.
pub(super) fn thumb_remap_section(toast: &Toasts, window: &ApplicationWindow) -> GtkBox {
    let section = GtkBox::new(Orientation::Vertical, 8);
    let store: StoreCell = Rc::new(RefCell::new(ButtonStore::default()));
    let tiles: RemapTiles = Rc::new(RefCell::new(HashMap::new()));
    let capture: RemapCapture = Rc::new(RefCell::new(None));
    let loading: Rc<Cell<bool>> = Rc::new(Cell::new(false));
    let (scope_group, scope_model) = remap_scope_bar(toast, &store, &tiles, &capture, &loading);

    let flow = FlowBox::new();
    flow.add_css_class("hud-tiles");
    flow.set_selection_mode(SelectionMode::None);
    flow.set_min_children_per_line(1);
    flow.set_max_children_per_line(1);
    flow.set_row_spacing(12);
    for (button, label) in REMAP_BUTTONS {
        let tile = remap_tile(button, label, toast, &capture, &tiles, &store, &loading);
        tiles.borrow_mut().insert(button, tile.clone());
        flow.append(&tile.child);
    }

    section.append(&scope_group);
    section.append(&flow);

    if let Some(Response::ButtonsConfig {
        buttons,
        profiles
    }) = ipc::try_request(&ipc::Request::GetButtons)
    {
        {
            let mut guard = store.borrow_mut();
            guard.global = buttons;
            guard.profiles = profiles;
        }
        refresh_scope_model(&scope_model, &store.borrow());
        apply_scope(&tiles, &store, &loading, &capture);
    } else {
        toast.show("Could not read the stored remaps — editing is disabled");
        for tile in tiles.borrow().values() {
            tile.set_editable(false);
        }
    }

    attach_remap_capture(window, &capture, &tiles, &store, &loading, toast);
    section
}
