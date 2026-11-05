// SPDX-FileCopyrightText: 2025-2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! One editable thumb remap row: widgets, kind picker wiring, key capture
//! and the daemon round-trips that bind or restore a button.

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
    devices::{Action, ButtonId},
    keys::key_name
};

use super::{REMAP_KINDS, RemapCapture, RemapTiles, StoreCell, report};
use crate::hud::{keybinds::open_keybind_picker, toasts::Toasts};

/// One editable software remap: button, current action and its widgets.
#[derive(Clone)]
pub(super) struct RemapTile {
    pub(super) child:   FlowBoxChild,
    pub(super) command: Rc<RefCell<String>>,
    pub(super) binding: GtkBox,
    pub(super) entry:   Entry,
    pub(super) kind:    DropDown,
    pub(super) capture: Button,
    pub(super) hypr:    Button,
    pub(super) clear:   Button
}

impl RemapTile {
    /// Locks or unlocks every control that would write to the daemon. Used
    /// when the initial `GetButtons` read failed: without the stored state
    /// the rows would show Native while real bindings stay active, and a
    /// clear would delete a binding the user never saw.
    pub(super) fn set_editable(&self, editable: bool) {
        self.kind.set_sensitive(editable);
        self.capture.set_sensitive(editable);
        self.hypr.set_sensitive(editable);
        self.entry.set_sensitive(editable);
        self.clear.set_sensitive(editable);
    }

    /// Reflects a bound key sequence as monospace chips.
    pub(super) fn set_keys(&self, keys: &[String]) {
        while let Some(child) = self.binding.first_child() {
            self.binding.remove(&child);
        }
        if keys.is_empty() {
            let placeholder = Label::new(Some("Native"));
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
        self.binding.set_tooltip_text(Some(&keys.join(" + ")));
        self.kind.set_selected(1);
        self.show_command(false);
    }

    /// Reflects a bound shell command and switches the row to command mode.
    pub(super) fn set_command(&self, command: String) {
        self.entry.set_text(&command);
        *self.command.borrow_mut() = command;
        self.kind.set_selected(2);
        self.show_command(true);
    }

    /// Reflects the native (unmapped) state.
    pub(super) fn set_native(&self) {
        self.set_keys(&[]);
        self.kind.set_selected(0);
    }

    /// Switches the editable control between key chips and command entry.
    pub(super) fn show_command(&self, command: bool) {
        self.binding.set_visible(!command);
        self.capture.set_visible(!command);
        self.hypr.set_visible(!command);
        self.entry.set_visible(command);
    }

    /// Reflects a stored action, or native when there is none.
    pub(super) fn apply(&self, action: Option<&Action>) {
        match action {
            Some(Action::Keypress {
                keys
            }) => self.set_keys(keys),
            Some(Action::Command {
                command
            }) => self.set_command(command.clone()),
            _ => self.set_native()
        }
    }
}

/// Human form of one evdev key name, e.g. `KEY_LEFTCTRL` → `Left Ctrl`.
fn pretty_key(name: &str) -> String {
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

/// Disarms a pending capture and restores the row button.
pub(super) fn disarm_capture(capture: &RemapCapture, tiles: &RemapTiles) {
    if let Some((button, _)) = capture.borrow_mut().take()
        && let Some(tile) = tiles.borrow().get(&button)
    {
        tile.capture.set_label("Capture");
        tile.capture.remove_css_class("suggested");
    }
}

/// Arms the key capture for `button`, disarming any other pending capture.
pub(super) fn arm_capture(button: ButtonId, capture: &RemapCapture, tiles: &RemapTiles) {
    disarm_capture(capture, tiles);
    *capture.borrow_mut() = Some((button, Vec::new()));
    if let Some(tile) = tiles.borrow().get(&button) {
        tile.capture.set_label("Press keys…");
        tile.capture.add_css_class("suggested");
    }
}

/// Wires the kind picker: Native clears through the daemon, Keys arms the
/// capture, Command reveals the shell entry.
fn wire_remap_kind(
    kind: &DropDown,
    button: ButtonId,
    toast: &Toasts,
    capture: &RemapCapture,
    tiles: &RemapTiles,
    store: &StoreCell,
    loading: &Rc<Cell<bool>>
) {
    let tiles_pick = Rc::clone(tiles);
    let store_pick = Rc::clone(store);
    let toast_pick = toast.clone();
    let loading_pick = Rc::clone(loading);
    let capture_pick = Rc::clone(capture);
    kind.connect_selected_notify(move |picker| {
        if loading_pick.get() {
            return;
        }
        match picker.selected() {
            0 => {
                let request = store_pick.borrow().clear_request(button);
                let response = ipc::try_request(&request);
                if matches!(response, Some(Response::Ok)) {
                    store_pick.borrow_mut().forget(button);
                    if let Some(tile) = tiles_pick.borrow().get(&button) {
                        loading_pick.set(true);
                        tile.set_native();
                        loading_pick.set(false);
                    }
                } else if let Some(tile) = tiles_pick.borrow().get(&button) {
                    loading_pick.set(true);
                    tile.apply(store_pick.borrow().action(button));
                    loading_pick.set(false);
                }
                report(response, &toast_pick, "Native behaviour restored");
            }
            1 => arm_capture(button, &capture_pick, &tiles_pick),
            _ => {
                if let Some(tile) = tiles_pick.borrow().get(&button) {
                    tile.show_command(true);
                }
            }
        }
    });
}

/// Wires the shell entry: Enter binds the command in the store's scope.
fn wire_remap_command(
    entry: &Entry,
    button: ButtonId,
    toast: &Toasts,
    tiles: &RemapTiles,
    store: &StoreCell
) {
    let toast_cmd = toast.clone();
    let tiles_cmd = Rc::clone(tiles);
    let store_cmd = Rc::clone(store);
    entry.connect_activate(move |field| {
        let text = field.text().trim().to_string();
        if text.is_empty() {
            return;
        }
        let action = Action::Command {
            command: text.clone()
        };
        let request = store_cmd.borrow().bind_request(button, action.clone());
        let response = ipc::try_request(&request);
        if matches!(response, Some(Response::Ok)) {
            store_cmd.borrow_mut().record(button, action);
            if let Some(tile) = tiles_cmd.borrow().get(&button) {
                tile.set_command(text);
            }
        }
        report(response, &toast_cmd, "Button bound");
    });
}

/// Wires the trash button: restores the native behaviour in the scope.
fn wire_remap_clear(
    clear_btn: &Button,
    button: ButtonId,
    toast: &Toasts,
    tiles: &RemapTiles,
    store: &StoreCell,
    loading: &Rc<Cell<bool>>
) {
    let toast_reset = toast.clone();
    let tiles_reset = Rc::clone(tiles);
    let store_reset = Rc::clone(store);
    let loading_reset = Rc::clone(loading);
    clear_btn.connect_clicked(move |_| {
        let request = store_reset.borrow().clear_request(button);
        let response = ipc::try_request(&request);
        if matches!(response, Some(Response::Ok)) {
            store_reset.borrow_mut().forget(button);
            if let Some(tile) = tiles_reset.borrow().get(&button) {
                loading_reset.set(true);
                *tile.command.borrow_mut() = String::new();
                tile.set_native();
                loading_reset.set(false);
            }
        }
        report(response, &toast_reset, "Native behaviour restored");
    });
}

/// Builds one remap row: button name, current binding, kind picker, command
/// entry, capture and clear controls.
#[allow(
    clippy::too_many_arguments,
    reason = "tile builders receive the full set of shared wiring targets"
)]
pub(super) fn remap_tile(
    button: ButtonId,
    label: &str,
    toast: &Toasts,
    capture: &RemapCapture,
    tiles: &RemapTiles,
    store: &StoreCell,
    loading: &Rc<Cell<bool>>
) -> RemapTile {
    let child = FlowBoxChild::new();
    child.add_css_class("hud-tile");
    child.set_hexpand(true);
    let column = GtkBox::new(Orientation::Vertical, 8);

    let head = GtkBox::new(Orientation::Horizontal, 8);
    let heading = Label::new(Some(label));
    heading.add_css_class("title-4");
    heading.set_xalign(0.0);
    let binding = GtkBox::new(Orientation::Horizontal, 4);
    binding.set_halign(gtk4::Align::End);
    binding.set_hexpand(true);
    let entry = Entry::new();
    entry.add_css_class("hud-command");
    entry.set_placeholder_text(Some("shell command, Enter applies"));
    entry.set_hexpand(true);
    entry.set_valign(gtk4::Align::Center);
    entry.set_visible(false);
    head.append(&heading);
    head.append(&binding);
    head.append(&entry);

    let controls = GtkBox::new(Orientation::Horizontal, 8);
    let kind = DropDown::from_strings(&REMAP_KINDS);
    kind.set_valign(gtk4::Align::Center);
    kind.set_tooltip_text(Some("What the button should do"));
    let spring = Label::new(None);
    spring.set_hexpand(true);
    let hypr_btn = Button::with_label("Hyprland…");
    hypr_btn.set_tooltip_text(Some("Pick an existing Hyprland keybind"));
    let capture_btn = Button::with_label("Capture");
    let clear_btn = Button::from_icon_name("user-trash-symbolic");
    clear_btn.add_css_class("flat");
    clear_btn.set_tooltip_text(Some("Restore native behaviour"));
    controls.append(&kind);
    controls.append(&spring);
    controls.append(&hypr_btn);
    controls.append(&capture_btn);
    controls.append(&clear_btn);

    column.append(&head);
    column.append(&controls);
    child.set_child(Some(&column));

    wire_remap_kind(&kind, button, toast, capture, tiles, store, loading);
    wire_remap_command(&entry, button, toast, tiles, store);
    wire_remap_clear(&clear_btn, button, toast, tiles, store, loading);

    let capture_arm = Rc::clone(capture);
    let tiles_arm = Rc::clone(tiles);
    capture_btn.connect_clicked(move |_| arm_capture(button, &capture_arm, &tiles_arm));

    let tiles_hypr = Rc::clone(tiles);
    let store_hypr = Rc::clone(store);
    let loading_hypr = Rc::clone(loading);
    let toast_hypr = toast.clone();
    hypr_btn.connect_clicked(move |_| {
        let tiles_cb = Rc::clone(&tiles_hypr);
        let store_cb = Rc::clone(&store_hypr);
        let loading_cb = Rc::clone(&loading_hypr);
        let toast_pick = toast_hypr.clone();
        let toast_cb = toast_hypr.clone();
        open_keybind_picker(&toast_pick, move |keys| {
            commit_remap_keys(button, &tiles_cb, &store_cb, &loading_cb, &toast_cb, &keys);
        });
    });

    let row = RemapTile {
        child,
        command: Rc::new(RefCell::new(String::new())),
        binding,
        entry,
        kind,
        capture: capture_btn,
        hypr: hypr_btn,
        clear: clear_btn
    };
    loading.set(true);
    row.set_native();
    loading.set(false);
    row
}

/// Binds a finished key chord to `button` through the daemon and reflects
/// it on the row; shared by the window key capture and the Hyprland keybind
/// picker. The loading guard keeps the programmatic kind switch from
/// re-arming the capture.
pub(super) fn commit_remap_keys(
    button: ButtonId,
    tiles: &RemapTiles,
    store: &StoreCell,
    loading: &Cell<bool>,
    toast: &Toasts,
    names: &[String]
) {
    let Some(tile) = tiles.borrow().get(&button).cloned() else {
        return;
    };
    let action = Action::Keypress {
        keys: names.to_vec()
    };
    let request = store.borrow().bind_request(button, action.clone());
    let response = ipc::try_request(&request);
    if matches!(response, Some(Response::Ok)) {
        store.borrow_mut().record(button, action);
        loading.set(true);
        tile.set_keys(names);
        loading.set(false);
    }
    report(response, toast, "Button bound");
}

/// Installs the window-level capture listener for button binding.
///
/// Runs in the capture phase so chords never reach the diagram shortcuts or
/// the Escape-to-close controller while a binding is armed.
pub(super) fn attach_remap_capture(
    window: &ApplicationWindow,
    capture: &RemapCapture,
    tiles: &RemapTiles,
    store: &StoreCell,
    loading: &Rc<Cell<bool>>,
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
    let loading_release = Rc::clone(loading);
    let toast_release = toast.clone();
    controller.connect_key_released(move |_controller, _key, _code, _state| {
        let Some((button, pressed)) = capture_release.borrow().clone() else {
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
        commit_remap_keys(
            button,
            &tiles_release,
            &store_release,
            loading_release.as_ref(),
            &toast_release,
            &names
        );
    });

    window.add_controller(controller);
}
