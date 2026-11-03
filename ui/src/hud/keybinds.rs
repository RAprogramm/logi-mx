// SPDX-FileCopyrightText: 2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Searchable chooser over the live Hyprland keyboard bindings.
//!
//! Opens a modal window listing the bindings the daemon reads from Hyprland
//! (`Request::GetKeybinds`), filters them case-insensitively by the search
//! entry, and hands the chosen `KEY_*` chord to the caller. The gesture
//! tiles and the thumb remap tiles use it as an alternative to manual key
//! capture: the chosen chord commits through the same store path a finished
//! capture takes, so Hyprland semantics are preserved — the mouse replays
//! the bind and the compositor decides what it means.

use std::{cell::RefCell, rc::Rc};

use gtk4::{
    Box as GtkBox, EventControllerKey, Label, ListBox, ListBoxRow, Orientation, ScrolledWindow,
    SearchEntry, SelectionMode, Window, glib
};
use gtk4_layer_shell::{KeyboardMode, Layer, LayerShell};
use libadwaita::{Toast, ToastOverlay, prelude::*};
use logi_mx_daemon::{
    hyprbind::KeybindEntry,
    ipc::{self, Response}
};

/// Upper bound on rendered rows; a huge config is truncated rather than
/// freezing the list build.
const MAX_ROWS: usize = 300;

/// Opens the keybind chooser as a layer-shell overlay so it maps above the
/// HUD, grabbing the keyboard for the search entry.
///
/// The list is populated on open through a single daemon round-trip; a
/// daemon or Hyprland failure is reported on the status line and as a toast.
/// Activating a row (click or Enter) closes the window and invokes `on_pick`
/// with the entry's `KEY_*` chord; Escape closes without invoking it.
///
/// # Arguments
///
/// * `toast` - Overlay receiving the error notices.
/// * `on_pick` - Callback receiving the chosen chord.
pub(super) fn open_keybind_picker<F>(toast: &ToastOverlay, on_pick: F)
where
    F: Fn(Vec<String>) + 'static
{
    let window = Window::new();
    window.set_title(Some("Hyprland keybinds"));
    window.set_default_size(520, 480);
    window.set_decorated(false);
    window.init_layer_shell();
    window.set_layer(Layer::Overlay);
    window.set_keyboard_mode(KeyboardMode::Exclusive);

    let root = GtkBox::new(Orientation::Vertical, 8);
    root.set_margin_top(12);
    root.set_margin_bottom(12);
    root.set_margin_start(12);
    root.set_margin_end(12);

    let search = SearchEntry::new();
    search.set_placeholder_text(Some("Filter keybinds…"));

    let list = ListBox::new();
    list.set_selection_mode(SelectionMode::Single);
    list.set_vexpand(true);

    let status = Label::new(Some("Reading Hyprland bindings…"));
    status.add_css_class("caption");
    status.add_css_class("dim-label");
    status.set_wrap(true);

    let entries = Rc::new(load_entries(toast));
    let filter = Rc::new(RefCell::new(String::new()));
    let on_pick: Rc<dyn Fn(Vec<String>)> = Rc::new(on_pick);
    wire_filter(&list, &search, &filter);
    populate(&list, &entries, &status);

    let scroll = ScrolledWindow::new();
    scroll.set_child(Some(&list));
    scroll.set_vexpand(true);

    wire_activation(&list, &window, &entries, &on_pick);
    wire_escape(&window);

    root.append(&search);
    root.append(&scroll);
    root.append(&status);
    window.set_child(Some(&root));
    window.present();
    search.grab_focus();
}

/// Fetches the replayable bindings from the daemon, reporting failures.
fn load_entries(toast: &ToastOverlay) -> Vec<KeybindEntry> {
    match ipc::try_request(&ipc::Request::GetKeybinds) {
        Some(Response::Keybinds {
            entries
        }) => entries,
        Some(Response::Error {
            message
        }) => {
            toast.add_toast(Toast::new(&message));
            Vec::new()
        }
        _ => {
            toast.add_toast(Toast::new("Daemon unavailable — no keybinds to list"));
            Vec::new()
        }
    }
}

/// Fills the list with one row per entry and writes the summary line.
fn populate(list: &ListBox, entries: &[KeybindEntry], status: &Label) {
    for entry in entries.iter().take(MAX_ROWS) {
        let label = Label::new(Some(&entry.label));
        label.set_xalign(0.0);
        let row = ListBoxRow::new();
        row.set_child(Some(&label));
        list.append(&row);
    }
    let summary = match entries.len() {
        0 => "No replayable keybinds reported".to_string(),
        count if count > MAX_ROWS => {
            format!("{count} keybinds · showing first {MAX_ROWS} · click or Enter to bind")
        }
        count => format!("{count} keybinds · click or Enter to bind · Esc to cancel")
    };
    status.set_text(&summary);
}

/// Keeps the visible rows matching the search text, case-insensitively.
fn wire_filter(list: &ListBox, search: &SearchEntry, filter: &Rc<RefCell<String>>) {
    let filter_func = Rc::clone(filter);
    list.set_filter_func(move |row: &ListBoxRow| {
        let needle = filter_func.borrow().clone();
        if needle.is_empty() {
            return true;
        }
        row.child()
            .and_then(|child| child.downcast::<Label>().ok())
            .is_some_and(|label| label.text().to_lowercase().contains(&needle))
    });
    let filter_changed = Rc::clone(filter);
    let list_changed = list.clone();
    search.connect_search_changed(move |entry| {
        *filter_changed.borrow_mut() = entry.text().trim().to_lowercase();
        list_changed.invalidate_filter();
    });
}

/// Closes the chooser and hands the chosen chord to the caller.
fn wire_activation(
    list: &ListBox,
    window: &Window,
    entries: &Rc<Vec<KeybindEntry>>,
    on_pick: &Rc<dyn Fn(Vec<String>)>
) {
    let entries_pick = Rc::clone(entries);
    let window_pick = window.clone();
    let on_pick_cb = Rc::clone(on_pick);
    list.connect_row_activated(move |_list, row| {
        let index = usize::try_from(row.index()).unwrap_or(usize::MAX);
        let Some(entry) = entries_pick.get(index) else {
            return;
        };
        let keys = entry.keys.clone();
        window_pick.close();
        on_pick_cb(keys);
    });
}

/// Makes Escape close the chooser without committing anything.
fn wire_escape(window: &Window) {
    let controller = EventControllerKey::new();
    let window_esc = window.clone();
    controller.connect_key_pressed(move |_controller, key, _code, _state| {
        if key == gtk4::gdk::Key::Escape {
            window_esc.close();
            glib::Propagation::Stop
        } else {
            glib::Propagation::Proceed
        }
    });
    window.add_controller(controller);
}
