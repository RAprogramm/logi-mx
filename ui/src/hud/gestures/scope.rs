// SPDX-FileCopyrightText: 2025-2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! The gesture scope bar: switching between the global bindings and
//! per-application profiles, adding a profile by window class, adopting the
//! focused window or removing the profile again.

use std::{cell::Cell, rc::Rc};

use gtk4::{Box as GtkBox, Button, DropDown, Entry, Orientation};
use libadwaita::{ActionRow, PreferencesGroup, prelude::*};
use logi_mx_daemon::ipc::{self, Response};
use logi_mx_driver::devices::{Action, GestureMode};

use super::{
    Toasts, report,
    store::{BindingScope, GestureCapture, GestureStore, GestureTiles, StoreCell},
    tile::disarm_capture
};

/// Profile application ids in dropdown order.
fn scope_apps(store: &GestureStore) -> Vec<String> {
    let mut apps: Vec<String> = store.profiles.keys().cloned().collect();
    apps.sort();
    apps
}

/// Refills the scope dropdown after a profile was added or removed: index 0
/// stays "Global", the rest are the profile apps in display order.
pub(super) fn refresh_scope_model(model: &gtk4::StringList, store: &GestureStore) {
    let extra = model.n_items().saturating_sub(1);
    let apps = scope_apps(store);
    let refs: Vec<&str> = apps.iter().map(String::as_str).collect();
    model.splice(1, extra, &refs);
}

/// Reloads every tile from the store bindings of the current scope. Tile
/// updates fire the mode picker, so the shared loading guard suppresses the
/// resulting IPC writes while the tiles are being refilled. A capture armed
/// under the previous scope is disarmed: its chord must not land in the
/// profile the user switched to before releasing the keys.
pub(super) fn apply_scope(
    tiles: &GestureTiles,
    store: &StoreCell,
    loading: &Cell<bool>,
    capture: &GestureCapture
) {
    disarm_capture(capture, tiles);
    let bindings = store.borrow().bindings().to_vec();
    loading.set(true);
    for (direction, tile) in tiles.borrow().iter() {
        tile.set_keys(Vec::new());
        tile.entry.set_text("");
        *tile.command.borrow_mut() = String::new();
        let Some(gesture) = bindings
            .iter()
            .find(|binding| binding.direction == *direction)
        else {
            continue;
        };
        tile.mode
            .set_selected(u32::from(gesture.mode != GestureMode::OnPress));
        match gesture.action.as_ref() {
            Action::Keypress {
                keys
            } => tile.set_keys(keys.clone()),
            Action::Command {
                command
            } => tile.set_command(command.clone()),
            _ => {}
        }
    }
    loading.set(false);
}

/// Creates the profile for `app` when missing and switches the tiles to it.
fn select_app_scope(
    app: &str,
    store: &StoreCell,
    model: &gtk4::StringList,
    picker: &DropDown,
    tiles: &GestureTiles,
    loading: &Cell<bool>,
    capture: &GestureCapture
) {
    let created = {
        let mut guard = store.borrow_mut();
        let created = !guard.profiles.contains_key(app);
        guard.profiles.entry(app.to_string()).or_default();
        guard.scope = BindingScope::App(app.to_string());
        created
    };
    if created {
        refresh_scope_model(model, &store.borrow());
    }
    let index = scope_apps(&store.borrow())
        .iter()
        .position(|candidate| candidate == app)
        .and_then(|pos| u32::try_from(pos).ok())
        .map_or(0, |pos| 1 + pos);
    loading.set(true);
    picker.set_selected(index);
    loading.set(false);
    apply_scope(tiles, store, loading, capture);
}

/// Builds the scope bar: the dropdown switching between the global bindings
/// and per-application profiles, plus controls to add a profile by window
/// class, adopt the focused window's class, or remove the current profile.
pub(super) fn gesture_scope_bar(
    toast: &Toasts,
    store: &StoreCell,
    tiles: &GestureTiles,
    capture: &GestureCapture,
    loading: &Rc<Cell<bool>>
) -> (PreferencesGroup, gtk4::StringList) {
    let group = PreferencesGroup::new();
    group.set_title("Binding scope");
    group.set_description(Some(
        "Edit the global bindings or a per-application override keyed by the window class"
    ));

    let model = gtk4::StringList::new(&["Global"]);
    let picker = DropDown::new(Some(model.clone()), None::<gtk4::Expression>);
    picker.set_valign(gtk4::Align::Center);
    let pick_row = ActionRow::new();
    pick_row.set_title("Editing");
    pick_row.add_suffix(&picker);
    group.add(&pick_row);

    let edit_row = ActionRow::new();
    edit_row.set_title("Add application");
    edit_row.set_subtitle("Hyprland window class of the override profile");
    let controls = GtkBox::new(Orientation::Horizontal, 8);
    controls.set_valign(gtk4::Align::Center);
    let app_entry = Entry::new();
    app_entry.set_placeholder_text(Some("e.g. firefox"));
    app_entry.set_width_chars(14);
    let add_btn = Button::with_label("Add");
    let focused_btn = Button::with_label("Use focused");
    let remove_btn = Button::with_label("Remove");
    controls.append(&app_entry);
    controls.append(&add_btn);
    controls.append(&focused_btn);
    controls.append(&remove_btn);
    edit_row.add_suffix(&controls);
    group.add(&edit_row);

    let store_pick = Rc::clone(store);
    let tiles_pick = Rc::clone(tiles);
    let capture_pick = Rc::clone(capture);
    let model_pick = model.clone();
    let loading_pick = Rc::clone(loading);
    picker.connect_selected_notify(move |pick| {
        if loading_pick.get() {
            return;
        }
        let index = pick.selected();
        let scope = if index == 0 {
            BindingScope::Global
        } else {
            let app = model_pick.string(index).map_or(String::new(), String::from);
            BindingScope::App(app)
        };
        store_pick.borrow_mut().scope = scope;
        apply_scope(
            &tiles_pick,
            &store_pick,
            loading_pick.as_ref(),
            &capture_pick
        );
    });

    let on_add: Rc<dyn Fn(String)> = {
        let store_add = Rc::clone(store);
        let tiles_add = Rc::clone(tiles);
        let capture_add = Rc::clone(capture);
        let model_add = model.clone();
        let picker_add = picker.clone();
        let loading_add = Rc::clone(loading);
        Rc::new(move |raw: String| {
            let app = raw.trim();
            if app.is_empty() {
                return;
            }
            select_app_scope(
                app,
                &store_add,
                &model_add,
                &picker_add,
                &tiles_add,
                loading_add.as_ref(),
                &capture_add
            );
        })
    };
    let entry_add = Rc::clone(&on_add);
    app_entry.connect_activate(move |field| entry_add(field.text().to_string()));
    let btn_add = Rc::clone(&on_add);
    add_btn.connect_clicked(move |_| btn_add(app_entry.text().to_string()));

    wire_scope_profile_buttons(
        ScopeHandles {
            toast,
            store,
            tiles,
            capture,
            loading,
            model: &model,
            picker
        },
        &focused_btn,
        &remove_btn
    );

    (group, model)
}

/// Shared handles the scope bar controls mutate.
struct ScopeHandles<'a> {
    toast:   &'a Toasts,
    store:   &'a StoreCell,
    tiles:   &'a GestureTiles,
    capture: &'a GestureCapture,
    loading: &'a Rc<Cell<bool>>,
    model:   &'a gtk4::StringList,
    picker:  DropDown
}

/// Wires the "Use focused" and "Remove" controls of the scope bar: the first
/// adopts the focused window's class as the active profile, the second drops
/// the current profile and returns to the global bindings.
fn wire_scope_profile_buttons(
    handles: ScopeHandles<'_>,
    focused_btn: &Button,
    remove_btn: &Button
) {
    let ScopeHandles {
        toast,
        store,
        tiles,
        capture,
        loading,
        model,
        picker
    } = handles;
    let store_focus = Rc::clone(store);
    let tiles_focus = Rc::clone(tiles);
    let capture_focus = Rc::clone(capture);
    let model_focus = model.clone();
    let picker_focus = picker.clone();
    let loading_focus = Rc::clone(loading);
    let toast_focus = toast.clone();
    focused_btn.connect_clicked(
        move |_| match ipc::try_request(&ipc::Request::GetFocusedApp) {
            Some(Response::FocusedApp {
                app_id: Some(app)
            }) => select_app_scope(
                &app,
                &store_focus,
                &model_focus,
                &picker_focus,
                &tiles_focus,
                loading_focus.as_ref(),
                &capture_focus
            ),
            _ => toast_focus.show("The desktop reported no focused window")
        }
    );

    let store_remove = Rc::clone(store);
    let tiles_remove = Rc::clone(tiles);
    let capture_remove = Rc::clone(capture);
    let model_remove = model.clone();
    let loading_remove = Rc::clone(loading);
    let toast_remove = toast.clone();
    remove_btn.connect_clicked(move |_| {
        let app = match store_remove.borrow().scope.clone() {
            BindingScope::App(app) => app,
            BindingScope::Global => {
                toast_remove.show("The global bindings cannot be removed");
                return;
            }
        };
        let response = ipc::try_request(&ipc::Request::RemoveAppProfile {
            app_id: app.clone()
        });
        if matches!(response, Some(Response::Ok)) {
            {
                let mut guard = store_remove.borrow_mut();
                guard.profiles.remove(&app);
                guard.scope = BindingScope::Global;
            }
            refresh_scope_model(&model_remove, &store_remove.borrow());
            loading_remove.set(true);
            picker.set_selected(0);
            loading_remove.set(false);
            apply_scope(
                &tiles_remove,
                &store_remove,
                loading_remove.as_ref(),
                &capture_remove
            );
        }
        report(response, &toast_remove, "Profile removed");
    });
}
