// SPDX-FileCopyrightText: 2025-2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! One editable gesture row: widgets, the moment and kind pickers, the
//! command entry, the Hyprland keybind picker and the daemon round-trips
//! that bind or clear a gesture.

use std::{
    cell::{Cell, RefCell},
    rc::Rc
};

use gtk4::{
    Box as GtkBox, Button, DropDown, Entry, EventControllerKey, FlowBoxChild, Label, Orientation,
    glib
};
use libadwaita::{ApplicationWindow, prelude::*};
use logi_mx_daemon::ipc::{self, Response};
use logi_mx_driver::{
    devices::{Action, GestureDirection, GestureMode},
    keys::key_name
};

use super::{
    Toasts, open_keybind_picker, report,
    store::{
        GESTURE_KINDS, GESTURE_MODES, GestureCapture, GestureTiles, StoreCell, binding_text,
        pretty_key
    }
};

/// One editable gesture binding: direction, action and its widgets.
#[derive(Clone)]
pub(super) struct GestureTile {
    pub(super) child:   FlowBoxChild,
    keys:               Rc<RefCell<Vec<String>>>,
    pub(super) command: Rc<RefCell<String>>,
    binding:            GtkBox,
    pub(super) entry:   Entry,
    kind:               DropDown,
    pub(super) mode:    DropDown,
    capture:            Button,
    hypr:               Button
}

impl GestureTile {
    /// Reflects a bound key sequence on the tile as monospace chips.
    pub(super) fn set_keys(&self, keys: Vec<String>) {
        while let Some(child) = self.binding.first_child() {
            self.binding.remove(&child);
        }
        if keys.is_empty() {
            let placeholder = Label::new(Some("Not bound"));
            placeholder.add_css_class("dim-label");
            placeholder.add_css_class("caption");
            self.binding.append(&placeholder);
        } else {
            for (index, key) in keys.iter().enumerate() {
                if index > 0 {
                    let plus = Label::new(Some("+"));
                    plus.add_css_class("hud-chip-sep");
                    self.binding.append(&plus);
                }
                let chip = Label::new(Some(&pretty_key(key)));
                chip.add_css_class("hud-chip");
                self.binding.append(&chip);
            }
        }
        self.binding.set_tooltip_text(Some(&binding_text(&keys)));
        *self.keys.borrow_mut() = keys;
        self.kind.set_selected(0);
        self.show_command(false);
    }

    /// Reflects a bound shell command and switches the tile to command mode.
    pub(super) fn set_command(&self, command: String) {
        self.entry.set_text(&command);
        *self.command.borrow_mut() = command;
        self.kind.set_selected(1);
        self.show_command(true);
    }

    /// Switches the editable control between key capture and command entry.
    fn show_command(&self, command: bool) {
        self.binding.set_visible(!command);
        self.capture.set_visible(!command);
        self.hypr.set_visible(!command);
        self.entry.set_visible(command);
    }

    /// The fire moment currently selected on this tile.
    fn current_mode(&self) -> GestureMode {
        picker_mode(&self.mode)
    }

    /// The action currently edited on this tile, if any is complete.
    fn current_action(&self) -> Option<Action> {
        if self.kind.selected() == 1 {
            let command = self.command.borrow().clone();
            (!command.is_empty()).then_some(Action::Command {
                command
            })
        } else {
            let keys = self.keys.borrow().clone();
            (!keys.is_empty()).then_some(Action::Keypress {
                keys
            })
        }
    }
}

/// Disarms a pending capture and restores the tile button.
pub(super) fn disarm_capture(capture: &GestureCapture, tiles: &GestureTiles) {
    if let Some((direction, _)) = capture.borrow_mut().take()
        && let Some(tile) = tiles.borrow().get(&direction)
    {
        tile.capture.set_label("Capture");
        tile.capture.remove_css_class("suggested");
    }
}

/// Builds one gesture tile: direction, current binding, action kind, fire
/// moment and the capture/clear controls.
#[allow(
    clippy::too_many_arguments,
    reason = "tile builders receive the full set of shared wiring targets"
)]
pub(super) fn gesture_tile(
    direction: GestureDirection,
    label: &str,
    toast: &Toasts,
    capture: &GestureCapture,
    tiles: &GestureTiles,
    store: &StoreCell,
    loading: &Rc<Cell<bool>>
) -> GestureTile {
    let child = FlowBoxChild::new();
    child.add_css_class("hud-tile");
    child.set_hexpand(true);
    let column = GtkBox::new(Orientation::Vertical, 8);

    let head = GtkBox::new(Orientation::Horizontal, 8);
    let title = Label::new(Some(label));
    title.add_css_class("title-4");
    title.set_xalign(0.0);
    let binding = GtkBox::new(Orientation::Horizontal, 4);
    binding.set_halign(gtk4::Align::End);
    binding.set_hexpand(true);
    let entry = Entry::new();
    entry.add_css_class("hud-command");
    entry.set_placeholder_text(Some("shell command, Enter applies"));
    entry.set_hexpand(true);
    entry.set_valign(gtk4::Align::Center);
    entry.set_visible(false);
    head.append(&title);
    head.append(&binding);
    head.append(&entry);

    let controls = GtkBox::new(Orientation::Horizontal, 8);
    let kind = DropDown::from_strings(&GESTURE_KINDS);
    kind.set_valign(gtk4::Align::Center);
    kind.set_tooltip_text(Some("Action kind"));
    let mode = DropDown::from_strings(&GESTURE_MODES);
    mode.set_valign(gtk4::Align::Center);
    let spring = Label::new(None);
    spring.set_hexpand(true);
    let hypr_btn = Button::with_label("Hyprland…");
    hypr_btn.set_tooltip_text(Some("Pick an existing Hyprland keybind"));
    let capture_btn = Button::with_label("Capture");
    let clear_btn = Button::from_icon_name("user-trash-symbolic");
    clear_btn.add_css_class("flat");
    clear_btn.set_tooltip_text(Some("Clear binding"));
    controls.append(&kind);
    controls.append(&mode);
    controls.append(&spring);
    controls.append(&hypr_btn);
    controls.append(&capture_btn);
    controls.append(&clear_btn);

    column.append(&head);
    column.append(&controls);
    child.set_child(Some(&column));

    let keys: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
    let command: Rc<RefCell<String>> = Rc::new(RefCell::new(String::new()));

    let tiles_kind = Rc::clone(tiles);
    kind.connect_selected_notify(move |picker| {
        if let Some(tile) = tiles_kind.borrow().get(&direction) {
            tile.show_command(picker.selected() == 1);
        }
    });
    wire_command_entry(&entry, toast, tiles, store, direction);
    wire_moment_update(&mode, toast, tiles, store, loading, direction);

    let toast_clear = toast.clone();
    let tiles_clear = Rc::clone(tiles);
    let store_clear = Rc::clone(store);
    clear_btn.connect_clicked(move |_| {
        let request = store_clear.borrow().clear_request(direction);
        let response = ipc::try_request(&request);
        if matches!(response, Some(Response::Ok)) {
            store_clear.borrow_mut().forget(direction);
            if let Some(tile) = tiles_clear.borrow().get(&direction) {
                *tile.command.borrow_mut() = String::new();
                tile.set_keys(Vec::new());
            }
        }
        report(response, &toast_clear, "Gesture cleared");
    });

    let capture_arm = Rc::clone(capture);
    let tiles_arm = Rc::clone(tiles);
    capture_btn.connect_clicked(move |_| {
        disarm_capture(&capture_arm, &tiles_arm);
        *capture_arm.borrow_mut() = Some((direction, Vec::new()));
        if let Some(tile) = tiles_arm.borrow().get(&direction) {
            tile.capture.set_label("Press keys…");
            tile.capture.add_css_class("suggested");
        }
    });

    wire_gesture_hypr(&hypr_btn, direction, toast, tiles, store);

    let gesture = GestureTile {
        child,
        keys,
        command,
        binding,
        entry,
        kind,
        mode,
        capture: capture_btn,
        hypr: hypr_btn
    };
    gesture.set_keys(Vec::new());
    gesture
}

/// Wires the Hyprland chooser button of a gesture tile: opens the picker and
/// commits the chosen chord through the shared store path.
fn wire_gesture_hypr(
    button: &Button,
    direction: GestureDirection,
    toast: &Toasts,
    tiles: &GestureTiles,
    store: &StoreCell
) {
    let tiles_hypr = Rc::clone(tiles);
    let store_hypr = Rc::clone(store);
    let toast_hypr = toast.clone();
    button.connect_clicked(move |_| {
        let tiles_cb = Rc::clone(&tiles_hypr);
        let store_cb = Rc::clone(&store_hypr);
        let toast_pick = toast_hypr.clone();
        let toast_cb = toast_hypr.clone();
        open_keybind_picker(&toast_pick, move |keys| {
            commit_gesture_keys(direction, &tiles_cb, &store_cb, &toast_cb, keys);
        });
    });
}

/// Wires the command entry of a gesture tile: Enter binds the shell command
/// for the direction at the tile's current fire moment, in the store's
/// active scope.
fn wire_command_entry(
    entry: &Entry,
    toast: &Toasts,
    tiles: &GestureTiles,
    store: &StoreCell,
    direction: GestureDirection
) {
    let toast_bind = toast.clone();
    let tiles_bind = Rc::clone(tiles);
    let store_bind = Rc::clone(store);
    entry.connect_activate(move |field| {
        let text = field.text().trim().to_string();
        if text.is_empty() {
            return;
        }
        let Some(found) = tiles_bind.borrow().get(&direction).cloned() else {
            return;
        };
        let mode = found.current_mode();
        let action = Action::Command {
            command: text.clone()
        };
        let request = store_bind
            .borrow()
            .bind_request(direction, mode, action.clone());
        let response = ipc::try_request(&request);
        if matches!(response, Some(Response::Ok)) {
            *found.command.borrow_mut() = text;
            store_bind.borrow_mut().record(direction, mode, action);
        }
        report(response, &toast_bind, "Gesture bound");
    });
}

/// Wires the fire-moment picker: changing it re-sends the action currently
/// edited on the tile with the new moment, unless the picker is only being
/// refilled by a scope switch.
fn wire_moment_update(
    mode: &DropDown,
    toast: &Toasts,
    tiles: &GestureTiles,
    store: &StoreCell,
    loading: &Rc<Cell<bool>>,
    direction: GestureDirection
) {
    let toast_pick = toast.clone();
    let tiles_pick = Rc::clone(tiles);
    let store_pick = Rc::clone(store);
    let loading_pick = Rc::clone(loading);
    mode.connect_selected_notify(move |picker| {
        if loading_pick.get() {
            return;
        }
        let Some(found) = tiles_pick.borrow().get(&direction).cloned() else {
            return;
        };
        let Some(action) = found.current_action() else {
            return;
        };
        let moment = picker_mode(picker);
        let request = store_pick
            .borrow()
            .bind_request(direction, moment, action.clone());
        let response = ipc::try_request(&request);
        if matches!(response, Some(Response::Ok)) {
            store_pick.borrow_mut().record(direction, moment, action);
        } else {
            let previous = store_pick
                .borrow()
                .bindings()
                .iter()
                .find(|gesture| gesture.direction == direction)
                .map_or(GestureMode::OnPress, |gesture| gesture.mode);
            loading_pick.set(true);
            picker.set_selected(u32::from(previous != GestureMode::OnPress));
            loading_pick.set(false);
        }
        report(response, &toast_pick, "Gesture updated");
    });
}

/// Maps a fire-moment picker selection to its `GestureMode`.
fn picker_mode(picker: &DropDown) -> GestureMode {
    if picker.selected() == 0 {
        GestureMode::OnPress
    } else {
        GestureMode::OnRelease
    }
}

/// Binds a finished key chord to `direction` through the daemon and
/// reflects it on the tile; shared by the window key capture and the
/// Hyprland keybind picker.
fn commit_gesture_keys(
    direction: GestureDirection,
    tiles: &GestureTiles,
    store: &StoreCell,
    toast: &Toasts,
    names: Vec<String>
) {
    let Some(tile) = tiles.borrow().get(&direction).cloned() else {
        return;
    };
    let moment = tile.current_mode();
    let action = Action::Keypress {
        keys: names.clone()
    };
    let request = store
        .borrow()
        .bind_request(direction, moment, action.clone());
    let response = ipc::try_request(&request);
    if matches!(response, Some(Response::Ok)) {
        store.borrow_mut().record(direction, moment, action);
        tile.set_keys(names);
    }
    report(response, toast, "Gesture bound");
}

/// Installs the window-level capture listener for gesture binding.
///
/// Runs in the capture phase so chords never reach the diagram shortcuts or
/// the Escape-to-close controller while a binding is armed.
pub(super) fn attach_gesture_capture(
    window: &ApplicationWindow,
    capture: &GestureCapture,
    tiles: &GestureTiles,
    store: &StoreCell,
    toast: &Toasts
) {
    let controller = EventControllerKey::new();
    controller.set_propagation_phase(gtk4::PropagationPhase::Capture);

    let capture_press = Rc::clone(capture);
    let tiles_press = Rc::clone(tiles);
    controller.connect_key_pressed(move |_controller, key, code, _state| {
        if capture_press.borrow().is_none() {
            return glib::Propagation::Proceed;
        }
        if key == gtk4::gdk::Key::Escape {
            disarm_capture(&capture_press, &tiles_press);
            return glib::Propagation::Stop;
        }
        if let Some(evdev) = code.checked_sub(8).and_then(|raw| u16::try_from(raw).ok())
            && key_name(evdev).is_some()
            && let Some((_, pressed)) = capture_press.borrow_mut().as_mut()
            && !pressed.contains(&evdev)
        {
            pressed.push(evdev);
        }
        glib::Propagation::Stop
    });

    let capture_release = Rc::clone(capture);
    let tiles_release = Rc::clone(tiles);
    let store_release = Rc::clone(store);
    let toast_release = toast.clone();
    controller.connect_key_released(move |_controller, _key, _code, _state| {
        let Some((direction, pressed)) = capture_release.borrow().clone() else {
            return;
        };
        if pressed.is_empty() {
            return;
        }
        disarm_capture(&capture_release, &tiles_release);
        let names: Vec<String> = pressed
            .iter()
            .filter_map(|code| key_name(*code))
            .map(str::to_string)
            .collect();
        commit_gesture_keys(
            direction,
            &tiles_release,
            &store_release,
            &toast_release,
            names
        );
    });

    window.add_controller(controller);
}
