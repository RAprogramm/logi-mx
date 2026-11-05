// SPDX-FileCopyrightText: 2025-2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Gestures page: the master switch, the binding-scope bar and one editable
//! tile per swipe direction, with window-level key capture for binding.
mod scope;
mod store;
mod tile;

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc
};

use gtk4::{FlowBox, ScrolledWindow, SelectionMode, Switch};
use libadwaita::{ActionRow, ApplicationWindow, PreferencesGroup, prelude::*};
use logi_mx_daemon::ipc::{self, Response};

use self::{
    scope::{apply_scope, gesture_scope_bar, refresh_scope_model},
    store::{GESTURE_TILES, GestureCapture, GestureStore, GestureTiles, StoreCell},
    tile::{attach_gesture_capture, gesture_tile}
};
use super::{keybinds::open_keybind_picker, page_body, page_scroll, report, toasts::Toasts};

/// Builds the Gestures page: a master switch, the scope bar and one tile per
/// direction.
///
/// Capture arms a window-level key listener: the next chord becomes the
/// binding the moment a key is released, Escape cancels. Every change is
/// applied and persisted by the daemon immediately.
pub(super) fn gestures_page(toast: &Toasts, window: &ApplicationWindow) -> ScrolledWindow {
    let scroll = page_scroll();
    let box_ = page_body();

    let enable_group = PreferencesGroup::new();
    enable_group.set_title("Gesture recognition");
    enable_group.set_description(Some(
        "Take over the gesture button and fire key combinations on swipes"
    ));
    let enable_row = ActionRow::new();
    enable_row.set_title("Enable gestures");
    let enable_switch = Switch::new();
    enable_switch.set_valign(gtk4::Align::Center);
    enable_row.add_suffix(&enable_switch);
    enable_row.set_activatable_widget(Some(&enable_switch));
    enable_group.add(&enable_row);

    let tiles: GestureTiles = Rc::new(RefCell::new(HashMap::new()));
    let capture: GestureCapture = Rc::new(RefCell::new(None));
    let store: StoreCell = Rc::new(RefCell::new(GestureStore::default()));
    let loading: Rc<Cell<bool>> = Rc::new(Cell::new(false));
    let (scope_group, scope_model) = gesture_scope_bar(toast, &store, &tiles, &capture, &loading);

    let flow = FlowBox::new();
    flow.add_css_class("hud-tiles");
    flow.set_selection_mode(SelectionMode::None);
    flow.set_min_children_per_line(1);
    flow.set_max_children_per_line(1);
    flow.set_row_spacing(12);
    for (direction, label) in GESTURE_TILES {
        let tile = gesture_tile(direction, label, toast, &capture, &tiles, &store, &loading);
        tiles.borrow_mut().insert(direction, tile.clone());
        flow.append(&tile.child);
    }

    box_.append(&enable_group);
    box_.append(&scope_group);
    box_.append(&flow);
    scroll.set_child(Some(&box_));

    if let Some(Response::Gestures {
        enabled,
        gestures,
        profiles,
        ..
    }) = ipc::try_request(&ipc::Request::GetGestures)
    {
        enable_switch.set_active(enabled);
        {
            let mut guard = store.borrow_mut();
            guard.global = gestures;
            guard.profiles = profiles;
        }
        refresh_scope_model(&scope_model, &store.borrow());
        apply_scope(&tiles, &store, &loading, &capture);
    }

    let toast_enable = toast.clone();
    let enable_guard = Rc::new(Cell::new(false));
    let last_enabled = Rc::new(Cell::new(enable_switch.is_active()));
    enable_switch.connect_active_notify(move |switch| {
        if enable_guard.get() {
            return;
        }
        let enabled = switch.is_active();
        let response = ipc::try_request(&ipc::Request::SetGesturesEnabled {
            enabled
        });
        if matches!(response, Some(Response::Ok)) {
            last_enabled.set(enabled);
        } else {
            enable_guard.set(true);
            switch.set_active(last_enabled.get());
            enable_guard.set(false);
        }
        let outcome = if enabled {
            "Gestures enabled"
        } else {
            "Gestures disabled"
        };
        report(response, &toast_enable, outcome);
    });

    attach_gesture_capture(window, &capture, &tiles, &store, toast);
    scroll
}
