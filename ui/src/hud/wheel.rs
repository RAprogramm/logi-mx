// SPDX-FileCopyrightText: 2025-2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Wheel page: `SmartShift` tuning and hi-res / natural scrolling controls.

use std::{cell::RefCell, rc::Rc};

use gtk4::{Button, Orientation, Scale, ScrolledWindow, Switch};
use libadwaita::{ActionRow, PreferencesGroup, prelude::*};
use logi_mx_daemon::ipc;

use super::{PanelWidgets, page_body, page_scroll, report, toasts::Toasts};

/// `SmartShift` threshold slider bounds.
const SMARTSHIFT_MIN: f64 = 1.0;
const SMARTSHIFT_MAX: f64 = 255.0;
const SMARTSHIFT_STEP: f64 = 1.0;

/// Narrows a GTK scale reading to a `SmartShift` threshold value.
#[must_use]
const fn scale_threshold(value: f64) -> u8 {
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "scale bounds are owned by this UI; the cast saturates"
    )]
    let narrowed = value.round() as u8;
    narrowed
}

/// Creates the wheel page: `SmartShift`, hi-res and direction settings.
///
/// Controls start at neutral placeholders and stay insensitive until the
/// snapshot loader fills them in once the daemon answers, so a placeholder
/// can never be applied before real values arrive.
pub(super) fn wheel_page(toast: &Toasts, widgets: &Rc<RefCell<PanelWidgets>>) -> ScrolledWindow {
    let scroll = page_scroll();
    let box_ = page_body();

    let shift_group = PreferencesGroup::new();
    shift_group.set_title("SmartShift");
    shift_group.set_description(Some("Automatic switching between ratchet and freespin"));

    let switch_row = ActionRow::new();
    switch_row.set_title("Enable SmartShift");
    let shift_switch = Switch::new();
    shift_switch.set_valign(gtk4::Align::Center);
    shift_switch.set_sensitive(false);
    switch_row.add_suffix(&shift_switch);
    switch_row.set_activatable_widget(Some(&shift_switch));

    let threshold_row = ActionRow::new();
    threshold_row.set_title("Disengage Sensitivity");
    threshold_row.set_subtitle("Detecting…");
    let threshold_scale = Scale::with_range(
        Orientation::Horizontal,
        SMARTSHIFT_MIN,
        SMARTSHIFT_MAX,
        SMARTSHIFT_STEP
    );
    threshold_scale.set_draw_value(true);
    threshold_scale.set_hexpand(true);
    threshold_scale.set_sensitive(false);
    let tr = threshold_row.clone();
    threshold_scale.connect_value_changed(move |s| {
        tr.set_subtitle(&format!("Current: {}", scale_threshold(s.value())));
    });

    let shift_apply = Button::with_label("Apply");
    shift_apply.add_css_class("suggested-action");
    shift_apply.add_css_class("pill");
    shift_apply.set_sensitive(false);
    let sw = shift_switch.clone();
    let ts = threshold_scale.clone();
    let to = toast.clone();
    shift_apply.connect_clicked(move |_| {
        let request = ipc::Request::SetSmartShift {
            enabled:   sw.is_active(),
            threshold: scale_threshold(ts.value())
        };
        report(ipc::try_request(&request), &to, "SmartShift applied");
    });

    shift_group.add(&switch_row);
    threshold_row.set_child(Some(&threshold_scale));
    shift_group.add(&threshold_row);
    let apply_row = ActionRow::new();
    apply_row.add_suffix(&shift_apply);
    shift_group.add(&apply_row);

    let scroll_group = PreferencesGroup::new();
    scroll_group.set_title("Scrolling");
    scroll_group.set_description(Some("High-resolution and natural scrolling"));

    let hires_row = ActionRow::new();
    hires_row.set_title("Hi-Res Scrolling");
    let hires_switch = Switch::new();
    hires_switch.set_valign(gtk4::Align::Center);
    hires_switch.set_sensitive(false);
    hires_row.add_suffix(&hires_switch);
    hires_row.set_activatable_widget(Some(&hires_switch));

    let inverted_row = ActionRow::new();
    inverted_row.set_title("Natural Scrolling");
    let inverted_switch = Switch::new();
    inverted_switch.set_valign(gtk4::Align::Center);
    inverted_switch.set_sensitive(false);
    inverted_row.add_suffix(&inverted_switch);
    inverted_row.set_activatable_widget(Some(&inverted_switch));

    let scroll_apply = Button::with_label("Apply");
    scroll_apply.add_css_class("suggested-action");
    scroll_apply.add_css_class("pill");
    scroll_apply.set_sensitive(false);
    let hs = hires_switch.clone();
    let inv = inverted_switch.clone();
    let to = toast.clone();
    scroll_apply.connect_clicked(move |_| {
        let request = ipc::Request::SetHires {
            enabled:  hs.is_active(),
            inverted: inv.is_active()
        };
        report(ipc::try_request(&request), &to, "Scroll settings applied");
    });

    scroll_group.add(&hires_row);
    scroll_group.add(&inverted_row);
    let scroll_apply_row = ActionRow::new();
    scroll_apply_row.add_suffix(&scroll_apply);
    scroll_group.add(&scroll_apply_row);

    box_.append(&shift_group);
    box_.append(&scroll_group);
    scroll.set_child(Some(&box_));

    let mut w = widgets.borrow_mut();
    w.smartshift_switch = Some(shift_switch);
    w.smartshift_scale = Some(threshold_scale);
    w.smartshift_row = Some(threshold_row);
    w.hires_switch = Some(hires_switch);
    w.inverted_switch = Some(inverted_switch);
    w.smartshift_apply = Some(shift_apply);
    w.scroll_apply = Some(scroll_apply);
    drop(w);
    scroll
}
