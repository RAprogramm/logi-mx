// SPDX-FileCopyrightText: 2025-2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! The settings panel: its column, header, tab strip and the page registry
//! that binds each tab to its builder.

use std::{cell::RefCell, rc::Rc};

use gtk4::{Box as GtkBox, Button, CheckButton, Image, Label, Orientation, ToggleButton};
use libadwaita::{ActionRow, ApplicationWindow, PreferencesGroup, ViewStack, prelude::*};

use super::{
    HudState, PANEL_WIDTH, PanelWidgets,
    background::{BgMode, apply_bg_classes, blur_supported, save_background},
    buttons::buttons_page,
    device::{device_page, pointer_page},
    diagnostics::diagnostics_page,
    gestures::gestures_page,
    pie::pie_page,
    queue_leaders,
    toasts::Toasts,
    wheel::wheel_page
};

/// Page specs shared by the stack and the segmented switcher: stack name,
/// human label and symbolic icon.
const PAGE_SPECS: [(&str, &str, &str); 7] = [
    ("buttons", "Buttons", "input-mouse-symbolic"),
    ("gestures", "Gestures", "input-touchpad-symbolic"),
    ("pie", "Pie", "media-record-symbolic"),
    ("wheel", "Wheel", "view-continuous-symbolic"),
    ("pointer", "Pointer", "preferences-desktop-input-symbolic"),
    ("device", "Device", "battery-symbolic"),
    ("diagnostics", "Diagnostics", "text-x-generic-symbolic")
];

/// Builds the settings panel with the Buttons, Gestures, Pie, Wheel, Pointer
/// and Device pages.
pub(super) fn create_panel(
    state: &Rc<RefCell<HudState>>,
    drawing: &gtk4::DrawingArea,
    window: &ApplicationWindow,
    widgets: &Rc<RefCell<PanelWidgets>>,
    toasts: &Toasts
) -> gtk4::Overlay {
    let root = GtkBox::new(Orientation::Vertical, 14);
    root.set_margin_top(18);
    root.set_margin_bottom(18);
    root.set_margin_start(18);
    root.set_margin_end(18);

    root.append(&panel_header(window, widgets));

    let stack = ViewStack::new();
    let pages = [
        (
            buttons_page(state, drawing, window, toasts),
            "buttons",
            "Buttons",
            "input-mouse-symbolic"
        ),
        (
            gestures_page(toasts, window),
            "gestures",
            "Gestures",
            "input-touchpad-symbolic"
        ),
        (pie_page(toasts), "pie", "Pie", "media-record-symbolic"),
        (
            wheel_page(toasts, widgets),
            "wheel",
            "Wheel",
            "view-continuous-symbolic"
        ),
        (
            pointer_page(toasts, widgets),
            "pointer",
            "Pointer",
            "preferences-desktop-input-symbolic"
        ),
        (
            device_page(state, window, toasts, widgets),
            "device",
            "Device",
            "battery-symbolic"
        ),
        (
            diagnostics_page(toasts),
            "diagnostics",
            "Diagnostics",
            "text-x-generic-symbolic"
        )
    ];
    for (page, name, title, _) in pages {
        stack.add_titled(&page, Some(name), title);
    }

    root.append(&panel_tabs(&stack));
    stack.connect_visible_child_notify(move |_| queue_leaders());

    let stack_box = GtkBox::new(Orientation::Vertical, 0);
    stack_box.set_vexpand(true);
    stack_box.append(&stack);
    root.append(&stack_box);

    let overlay = gtk4::Overlay::new();
    overlay.set_child(Some(&root));
    overlay.add_overlay(&toasts.widget());
    overlay.set_size_request(PANEL_WIDTH, -1);
    overlay.add_css_class("card");
    overlay.add_css_class("hud-panel");
    overlay
}

/// Builds the panel header: device title, live status chip and close button.
///
/// The status chip starts neutral; the snapshot loader repaints it once the
/// daemon answers, so the header never blocks on device I/O.
fn panel_header(window: &ApplicationWindow, widgets: &Rc<RefCell<PanelWidgets>>) -> GtkBox {
    let header = GtkBox::new(Orientation::Horizontal, 8);

    let titles = GtkBox::new(Orientation::Vertical, 0);
    let title = Label::new(Some("MX Master 3S"));
    title.add_css_class("title-3");
    title.set_xalign(0.0);
    let status = Label::new(Some("Connecting…"));
    status.add_css_class("caption");
    status.add_css_class("dim-label");
    status.set_xalign(0.0);
    titles.append(&title);
    titles.append(&status);
    widgets.borrow_mut().status_chip = Some(status);

    let close = Button::from_icon_name("window-close-symbolic");
    close.add_css_class("flat");
    close.add_css_class("hud-close");
    close.set_valign(gtk4::Align::Start);
    close.set_tooltip_text(Some("Close (Esc)"));
    let win_close = window.clone();
    close.connect_clicked(move |_| win_close.close());

    let spring = Label::new(None);
    spring.set_hexpand(true);

    header.append(&titles);
    header.append(&spring);
    header.append(&close);
    header
}

/// The tab glyph, centered so the vertical box skips it in its baseline
/// computation: a filling `GtkImage` reports no baseline and GTK warns
/// about it on every size request.
///
/// # Arguments
///
/// * `icon` - Symbolic icon name from [`PAGE_SPECS`].
fn tab_icon(icon: &str) -> Image {
    let glyph = Image::from_icon_name(icon);
    glyph.set_valign(gtk4::Align::Center);
    glyph
}

/// Builds the segmented page switcher: one pill per page, kept in sync with
/// the stack in both directions.
fn panel_tabs(stack: &ViewStack) -> GtkBox {
    let tabs = GtkBox::new(Orientation::Horizontal, 0);
    tabs.add_css_class("hud-tabs");
    tabs.set_hexpand(true);

    let mut buttons = Vec::with_capacity(PAGE_SPECS.len());
    for (index, (name, label, icon)) in PAGE_SPECS.iter().enumerate() {
        let tab = ToggleButton::new();
        tab.add_css_class("hud-tab");
        tab.set_tooltip_text(Some(label));
        tab.set_hexpand(true);

        let content = GtkBox::new(Orientation::Vertical, 2);
        content.append(&tab_icon(icon));
        let caption = Label::new(Some(label));
        caption.add_css_class("caption");
        content.append(&caption);
        tab.set_child(Some(&content));

        let stack_pick = stack.clone();
        let page_name = (*name).to_string();
        tab.connect_clicked(move |_| stack_pick.set_visible_child_name(&page_name));

        tabs.append(&tab);
        buttons.push((index, tab));
    }
    if let Some((_, first)) = buttons.first() {
        first.set_active(true);
    }

    let sync: Rc<Vec<(usize, ToggleButton)>> = Rc::new(buttons);
    stack.connect_visible_child_name_notify(move |stack| {
        let current = stack.visible_child_name().unwrap_or_default();
        for (index, tab) in sync.iter() {
            let active = PAGE_SPECS
                .get(*index)
                .is_some_and(|(name, _, _)| *name == current);
            tab.set_active(active);
        }
    });
    tabs
}

/// Builds the backdrop section: two independent checkboxes for dimming and
/// compositor blur, so any of the four combinations is reachable. Changing
/// either persists the mode and repaints the window backdrop.
pub(super) fn background_section(
    state: &Rc<RefCell<HudState>>,
    window: &ApplicationWindow
) -> PreferencesGroup {
    let group = PreferencesGroup::new();
    group.set_title("Background");
    group.set_description(Some("Backdrop behind the HUD"));

    let mode = state.borrow().bg;

    let dim_row = ActionRow::new();
    dim_row.set_title("Dim");
    let dim_check = CheckButton::new();
    dim_check.set_active(mode.dim_on());
    dim_check.set_valign(gtk4::Align::Center);
    dim_row.add_suffix(&dim_check);
    dim_row.set_activatable_widget(Some(&dim_check));

    let blur_row = ActionRow::new();
    blur_row.set_title("Blur");
    let blur_check = CheckButton::new();
    blur_check.set_active(mode.wants_blur());
    blur_check.set_valign(gtk4::Align::Center);
    blur_row.add_suffix(&blur_check);
    blur_row.set_activatable_widget(Some(&blur_check));
    if !blur_supported() {
        blur_check.set_sensitive(false);
        blur_row.set_tooltip_text(Some("Compositor blur requires GTK 4.24 or newer"));
        blur_check.set_tooltip_text(Some("Compositor blur requires GTK 4.24 or newer"));
    }

    let state_sync = Rc::clone(state);
    let window_sync = window.clone();
    let dim_sync = dim_check.clone();
    let blur_sync = blur_check.clone();
    let sync = Rc::new(move || {
        let next = BgMode::from_bools(dim_sync.is_active(), blur_sync.is_active());
        if state_sync.borrow().bg == next {
            return;
        }
        state_sync.borrow_mut().bg = next;
        save_background(next);
        apply_bg_classes(&window_sync, next);
    });
    let sync_dim = Rc::clone(&sync);
    let sync_blur = Rc::clone(&sync);
    dim_check.connect_toggled(move |_| sync_dim());
    blur_check.connect_toggled(move |_| sync_blur());

    group.add(&dim_row);
    group.add(&blur_row);
    group
}
