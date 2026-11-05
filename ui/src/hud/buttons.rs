// SPDX-FileCopyrightText: 2025-2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Buttons page: the adaptive flow of button tiles, each pairing a device
//! button with the dropdown that assigns its behaviour in firmware, plus the
//! per-application software remap of the two thumb side buttons.
mod firmware;
mod thumb;

use std::{cell::RefCell, rc::Rc};

pub(super) use firmware::clear_remap_hook;
use gtk4::{ScrolledWindow, glib};
use libadwaita::{ApplicationWindow, prelude::*};

use self::{
    firmware::{buttons_list_section, spawn_remap_loader},
    thumb::thumb_remap_section
};
use super::{
    HudState, MouseButton, ROWS_REGISTRY, RowMap, Tile, page_body, page_scroll, queue_leaders,
    report, sync_row_focus, toasts::Toasts
};

/// Creates the assignment page: adaptive button tiles that highlight the
/// diagram, each carrying its own assignment dropdown, followed by the
/// per-application software remap of the two thumb side buttons.
pub(super) fn buttons_page(
    state: &Rc<RefCell<HudState>>,
    drawing: &gtk4::DrawingArea,
    window: &ApplicationWindow,
    toast: &Toasts
) -> ScrolledWindow {
    let scroll = page_scroll();
    scroll
        .vadjustment()
        .connect_value_changed(move |_| queue_leaders());
    let box_ = page_body();
    let (buttons_section, rows) = buttons_list_section(state, drawing, toast);
    box_.append(&buttons_section);
    box_.append(&thumb_remap_section(toast, window));
    let rows_loader = Rc::clone(&rows);
    let window_weak = window.downgrade();
    glib::idle_add_local_once(move || {
        if window_weak.upgrade().is_some() {
            spawn_remap_loader(&rows_loader);
        }
    });
    scroll.set_child(Some(&box_));
    scroll
}
