// SPDX-FileCopyrightText: 2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! The slot editor: bind the selected wedge to a command or a picked
//! keybind, clear it, add and remove slots. Every edit rewrites the whole
//! ring through the scope-aware request and mirrors it locally on success.

use std::{
    cell::{Cell, RefCell},
    rc::Rc
};

use gtk4::{Button, Entry, Label};
use libadwaita::{ActionRow, PreferencesGroup, prelude::*};
use logi_mx_daemon::{
    ipc::{self, Response},
    pie::labels::action_summary
};
use logi_mx_driver::{config::PieSlot, prelude::Action};

use super::store::PieStore;
use crate::hud::{keybinds::open_keybind_picker, report, toasts::Toasts};

/// Largest ring the daemon accepts, mirrored from `PieConfig::MAX_SLOTS`.
pub const MAX_SLOTS: usize = 8;

/// Everything a slot editor callback needs to act on the selection.
#[derive(Clone)]
pub struct SlotContext {
    pub toast:    Toasts,
    pub store:    Rc<RefCell<PieStore>>,
    pub selected: Rc<Cell<Option<usize>>>,
    pub repaint:  Rc<dyn Fn()>,
    pub details:  Rc<dyn Fn()>,
    pub editor:   SlotEditor
}

/// Widgets of the slot editor, gated on the current selection.
#[derive(Clone)]
pub struct SlotEditor {
    pub group:      PreferencesGroup,
    pub title:      ActionRow,
    pub command:    Entry,
    pub apply_btn:  Button,
    pub pick_btn:   Button,
    pub clear_btn:  Button,
    pub add_btn:    Button,
    pub remove_btn: Button
}

impl SlotEditor {
    /// Builds the editor group: selection title, command row, keybind row,
    /// clear row and the add/remove rows.
    pub fn new() -> Self {
        let group = PreferencesGroup::new();
        group.set_title("Slot editor");
        group.set_description(Some(
            "Click a wedge on the wheel to select it. The ring shows exactly as many equal \
             wedges as there are slots; it opens from two."
        ));

        let title = ActionRow::new();
        title.set_title("Slot");
        title.set_subtitle("Click a wedge above");

        let command_row = ActionRow::new();
        command_row.set_title("Application command");
        command_row.set_subtitle("Press Enter to bind, e.g. thunar");
        let command = Entry::new();
        command.set_width_chars(18);
        command.set_valign(gtk4::Align::Center);
        let apply_btn = Button::with_label("Bind");
        apply_btn.set_valign(gtk4::Align::Center);
        command_row.add_suffix(&apply_btn);
        command_row.add_suffix(&command);

        let pick_row = ActionRow::new();
        pick_row.set_title("Keyboard shortcut");
        pick_row.set_subtitle("Pick an existing Hyprland keybind");
        let pick_btn = Button::with_label("Pick…");
        pick_btn.set_valign(gtk4::Align::Center);
        pick_row.add_suffix(&pick_btn);
        pick_row.set_activatable_widget(Some(&pick_btn));

        let clear_row = ActionRow::new();
        clear_row.set_title("Clear slot");
        clear_row.set_subtitle("Keeps the wedge but fires nothing");
        let clear_btn = Button::with_label("Clear");
        clear_btn.set_valign(gtk4::Align::Center);
        clear_row.add_suffix(&clear_btn);
        clear_row.set_activatable_widget(Some(&clear_btn));

        let add_row = ActionRow::new();
        add_row.set_title("Add slot");
        add_row.set_subtitle(&format!("Appends an empty wedge, at most {MAX_SLOTS}"));
        let add_btn = Button::with_label("Add");
        add_btn.set_valign(gtk4::Align::Center);
        add_row.add_suffix(&add_btn);
        add_row.set_activatable_widget(Some(&add_btn));

        let remove_row = ActionRow::new();
        remove_row.set_title("Remove slot");
        remove_row.set_subtitle("Deletes the selected wedge and renumbers the rest");
        let remove_btn = Button::with_label("Remove");
        remove_btn.add_css_class("destructive-action");
        remove_btn.set_valign(gtk4::Align::Center);
        remove_row.add_suffix(&remove_btn);
        remove_row.set_activatable_widget(Some(&remove_btn));

        group.add(&title);
        group.add(&command_row);
        group.add(&pick_row);
        group.add(&clear_row);
        group.add(&add_row);
        group.add(&remove_row);
        group.set_sensitive(false);

        Self {
            group,
            title,
            command,
            apply_btn,
            pick_btn,
            clear_btn,
            add_btn,
            remove_btn
        }
    }

    /// Points the editor at `index` of `ring` (or disables the per-slot rows
    /// when `None`), prefilling the command entry for command slots.
    pub fn inspect(&self, ring: &[PieSlot], index: Option<usize>) {
        self.group.set_sensitive(true);
        self.add_btn.set_sensitive(ring.len() < MAX_SLOTS);
        self.remove_btn.set_sensitive(index.is_some());
        self.command.set_sensitive(index.is_some());
        self.apply_btn.set_sensitive(index.is_some());
        self.pick_btn.set_sensitive(index.is_some());
        self.clear_btn.set_sensitive(index.is_some());
        let Some(index) = index else {
            self.title.set_title("Slot");
            self.title.set_subtitle("Click a wedge above");
            self.command.set_text("");
            return;
        };
        let Some(slot) = ring.get(index) else {
            self.title.set_title("Slot");
            self.title.set_subtitle("Click a wedge above");
            self.command.set_text("");
            return;
        };
        let summary = slot
            .label
            .clone()
            .or_else(|| slot.action.as_ref().map(action_summary))
            .unwrap_or_else(|| "empty".to_string());
        self.title
            .set_title(&format!("Slot {} — {summary}", index + 1));
        self.title.set_subtitle("Choose a new action below");
        let command = match slot.action.as_ref() {
            Some(Action::Command {
                command
            }) => command.clone(),
            _ => String::new()
        };
        self.command.set_text(&command);
    }
}

/// Replaces `index` in `ring` with `slot`, leaving the ring untouched when
/// the index is out of range.
#[must_use]
pub fn replace_slot(ring: &[PieSlot], index: usize, slot: PieSlot) -> Vec<PieSlot> {
    let mut next = ring.to_vec();
    if let Some(existing) = next.get_mut(index) {
        *existing = slot;
    }
    next
}

/// Drops `index` from `ring`, leaving it untouched when out of range.
#[must_use]
pub fn remove_slot(ring: &[PieSlot], index: usize) -> Vec<PieSlot> {
    let mut next = ring.to_vec();
    if index < next.len() {
        next.remove(index);
    }
    next
}

/// Sends the new `ring` for the current scope and mirrors it on success.
fn commit(ctx: &SlotContext, ring: Vec<PieSlot>, success: &str) {
    let request = ctx.store.borrow().set_ring_request(&ring);
    let response = ipc::try_request(&request);
    if matches!(response, Some(Response::Ok)) {
        ctx.store.borrow_mut().apply_ring(ring);
        (ctx.repaint)();
        (ctx.details)();
    }
    report(response, &ctx.toast, success);
}

/// Binds `action` to the slot captured at request time, not whatever is
/// selected by the time a callback runs.
fn bind_to(ctx: &SlotContext, index: usize, action: Option<Action>, success: &str) {
    let Some(action) = action else {
        ctx.toast.show("Enter a command first");
        return;
    };
    let ring = ctx.store.borrow().visible_ring();
    let next = replace_slot(
        &ring,
        index,
        PieSlot {
            action: Some(action),
            label:  None
        }
    );
    commit(ctx, next, success);
}

/// Wires the slot editor rows to the daemon and the store.
pub fn wire_slot_editor(ctx: &SlotContext) {
    let bind_command = {
        let ctx = ctx.clone();
        move |entry: &Entry| {
            let Some(index) = ctx.selected.get() else {
                ctx.toast.show("Select a wedge first");
                return;
            };
            let text = entry.text().trim().to_string();
            let action = (!text.is_empty()).then_some(Action::Command {
                command: text
            });
            bind_to(&ctx, index, action, "Slot bound");
        }
    };
    ctx.editor.apply_btn.connect_clicked({
        let command = ctx.editor.command.clone();
        let bind = bind_command.clone();
        move |_| bind(&command)
    });
    ctx.editor
        .command
        .connect_activate(move |entry| bind_command(entry));

    ctx.editor.pick_btn.connect_clicked({
        let ctx = ctx.clone();
        move |_| {
            let Some(index) = ctx.selected.get() else {
                ctx.toast.show("Select a wedge first");
                return;
            };
            let picker_toast = ctx.toast.clone();
            let picked = ctx.clone();
            open_keybind_picker(&picker_toast, move |keys| {
                bind_to(
                    &picked,
                    index,
                    Some(Action::Keypress {
                        keys
                    }),
                    "Slot bound"
                );
            });
        }
    });

    ctx.editor.clear_btn.connect_clicked({
        let ctx = ctx.clone();
        move |_| {
            let Some(index) = ctx.selected.get() else {
                ctx.toast.show("Select a wedge first");
                return;
            };
            let ring = ctx.store.borrow().visible_ring();
            let next = replace_slot(&ring, index, PieSlot::default());
            commit(&ctx, next, "Slot cleared");
        }
    });

    ctx.editor.add_btn.connect_clicked({
        let ctx = ctx.clone();
        move |_| {
            let ring = ctx.store.borrow().visible_ring();
            if ring.len() >= MAX_SLOTS {
                ctx.toast
                    .show(&format!("A ring carries at most {MAX_SLOTS} slots"));
                return;
            }
            let mut next = ring;
            next.push(PieSlot::default());
            commit(&ctx, next, "Slot added");
        }
    });

    ctx.editor.remove_btn.connect_clicked({
        let ctx = ctx.clone();
        move |_| {
            let Some(index) = ctx.selected.get() else {
                ctx.toast.show("Select a wedge first");
                return;
            };
            let ring = ctx.store.borrow().visible_ring();
            let next = remove_slot(&ring, index);
            ctx.selected.set(None);
            commit(&ctx, next, "Slot removed");
        }
    });
}

/// The one-line summary of the selected slot for the details label.
#[must_use]
pub fn slot_summary(ring: &[PieSlot], index: Option<usize>) -> String {
    let Some(index) = index else {
        return "Click a wedge to inspect its slot".to_string();
    };
    let Some(slot) = ring.get(index) else {
        return "Click a wedge to inspect its slot".to_string();
    };
    let summary = slot
        .label
        .clone()
        .or_else(|| slot.action.as_ref().map(action_summary))
        .unwrap_or_else(|| "empty".to_string());
    format!("Slot {} — {summary}", index + 1)
}

/// Builds the details label used by the page.
#[must_use]
pub fn details_label() -> Label {
    let details = Label::new(Some("Click a wedge to inspect its slot"));
    details.add_css_class("dim-label");
    details.set_halign(gtk4::Align::Center);
    details
}

#[cfg(test)]
#[path = "slots_tests.rs"]
mod tests;
