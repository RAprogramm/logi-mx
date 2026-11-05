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

/// One rendered page of the chooser: the entries whose rows exist right now,
/// in list order, so row activation maps back to the right binding.
type Shown = Rc<RefCell<Vec<KeybindEntry>>>;
use gtk4_layer_shell::{KeyboardMode, Layer, LayerShell};
use libadwaita::prelude::*;
use logi_mx_daemon::{
    hyprbind::KeybindEntry,
    ipc::{self, Response}
};

use super::toasts::Toasts;

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
pub(super) fn open_keybind_picker<F>(toast: &Toasts, on_pick: F)
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
    let on_pick: Rc<dyn Fn(Vec<String>)> = Rc::new(on_pick);
    let shown: Shown = Rc::new(RefCell::new(Vec::new()));
    *shown.borrow_mut() = rebuild(&list, &entries, "", &status);
    wire_search(&search, &list, &entries, &shown, &status);

    let scroll = ScrolledWindow::new();
    scroll.set_child(Some(&list));
    scroll.set_vexpand(true);

    wire_activation(&list, &window, &shown, &on_pick);
    wire_escape(&window);

    root.append(&search);
    root.append(&scroll);
    root.append(&status);
    window.set_child(Some(&root));
    window.present();
    search.grab_focus();
}

/// Fetches the replayable bindings from the daemon, reporting failures.
fn load_entries(toast: &Toasts) -> Vec<KeybindEntry> {
    match ipc::try_request(&ipc::Request::GetKeybinds) {
        Some(Response::Keybinds {
            entries
        }) => entries,
        Some(Response::Error {
            message
        }) => {
            toast.show(&message);
            Vec::new()
        }
        _ => {
            toast.show("Daemon unavailable — no keybinds to list");
            Vec::new()
        }
    }
}

/// Rebuilds the list with a row per matching entry and writes the summary
/// line. The search applies to the whole entry set before the row cap, so a
/// binding beyond the cap stays reachable by typing its name.
///
/// # Arguments
///
/// * `list` - The chooser list to refill.
/// * `entries` - Every fetched binding, in daemon order.
/// * `needle` - Lower-cased search text; empty matches everything.
/// * `status` - Summary line under the list.
///
/// # Returns
///
/// The entries that received rows, in list order.
fn rebuild(
    list: &ListBox,
    entries: &[KeybindEntry],
    needle: &str,
    status: &Label
) -> Vec<KeybindEntry> {
    let matched: Vec<&KeybindEntry> = entries
        .iter()
        .filter(|entry| needle.is_empty() || entry.label.to_lowercase().contains(needle))
        .collect();
    let shown: Vec<KeybindEntry> = matched
        .iter()
        .take(MAX_ROWS)
        .map(|entry| (*entry).clone())
        .collect();
    list.remove_all();
    for entry in &shown {
        let label = Label::new(Some(&entry.label));
        label.set_xalign(0.0);
        let row = ListBoxRow::new();
        row.set_child(Some(&label));
        list.append(&row);
    }
    status.set_text(&summary_line(entries.len(), matched.len(), needle));
    shown
}

/// Composes the chooser status line for one filter state.
fn summary_line(total: usize, matched: usize, needle: &str) -> String {
    let tail = "· click or Enter to bind · Esc to cancel";
    if total == 0 {
        return "No replayable keybinds reported".to_string();
    }
    if matched == 0 {
        return format!("No keybind matches {needle:?} {tail}");
    }
    if matched > MAX_ROWS {
        return format!("{matched} matching · showing first {MAX_ROWS} {tail}");
    }
    if needle.is_empty() {
        return format!("{matched} keybinds {tail}");
    }
    format!("{matched} of {total} keybinds match {tail}")
}

/// Refills the list on every search change: rows exist only for matches,
/// so the row cap never hides a searchable binding.
fn wire_search(
    search: &SearchEntry,
    list: &ListBox,
    entries: &Rc<Vec<KeybindEntry>>,
    shown: &Shown,
    status: &Label
) {
    let list_rebuild = list.clone();
    let entries_rebuild = Rc::clone(entries);
    let shown_rebuild = Rc::clone(shown);
    let status_rebuild = status.clone();
    search.connect_search_changed(move |entry| {
        let needle = entry.text().trim().to_lowercase();
        let next = rebuild(&list_rebuild, &entries_rebuild, &needle, &status_rebuild);
        *shown_rebuild.borrow_mut() = next;
    });
}

/// Closes the chooser and hands the chosen chord to the caller. Rows are
/// looked up in the currently shown (filtered) entries, not the full set.
fn wire_activation(
    list: &ListBox,
    window: &Window,
    shown: &Shown,
    on_pick: &Rc<dyn Fn(Vec<String>)>
) {
    let shown_pick = Rc::clone(shown);
    let window_pick = window.clone();
    let on_pick_cb = Rc::clone(on_pick);
    list.connect_row_activated(move |_list, row| {
        let index = usize::try_from(row.index()).unwrap_or(usize::MAX);
        let Some(keys) = shown_pick
            .borrow()
            .get(index)
            .map(|entry| entry.keys.clone())
        else {
            return;
        };
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
