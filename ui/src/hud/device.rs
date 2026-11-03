// SPDX-FileCopyrightText: 2025-2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Device and pointer pages: model and battery info, the DPI slider, desktop
//! integration and the version row.

use std::{cell::RefCell, rc::Rc};

use gtk4::{Box as GtkBox, Button, Orientation, Scale, ScrolledWindow};
use libadwaita::{
    ActionRow, ApplicationWindow, PreferencesGroup, Toast, ToastOverlay, prelude::*
};
use logi_mx_daemon::ipc::{self, Response};

use super::{
    HudState, PanelWidgets, apply_battery, background_section, page_body, page_scroll, report
};

/// DPI slider bounds accepted by the device.
const DPI_MIN: f64 = 400.0;
const DPI_MAX: f64 = 8000.0;
const DPI_STEP: f64 = 100.0;
/// Neutral DPI shown before the snapshot arrives.
const DEFAULT_DPI: u16 = 1000;

/// Narrows a GTK scale reading to a DPI setting value.
#[must_use]
const fn scale_dpi(value: f64) -> u16 {
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "scale bounds are owned by this UI; the cast saturates"
    )]
    let narrowed = value.round() as u16;
    narrowed
}

/// Creates the pointer page with the DPI slider.
///
/// The slider starts at the neutral default and stays insensitive together
/// with the Apply button; the snapshot loader fills in the real value and
/// enables both once the daemon answers, so the placeholder is never sent.
pub(super) fn pointer_page(
    toast: &ToastOverlay,
    widgets: &Rc<RefCell<PanelWidgets>>
) -> ScrolledWindow {
    let scroll = page_scroll();
    let group = PreferencesGroup::new();
    group.set_title("Pointer Sensitivity");
    group.set_description(Some("Cursor speed from 400 to 8000 DPI"));

    let dpi_row = ActionRow::new();
    dpi_row.set_title("Current DPI");
    dpi_row.set_subtitle("Detecting…");

    let scale_row = ActionRow::new();
    scale_row.set_title("Sensitivity");
    let scale = Scale::with_range(Orientation::Horizontal, DPI_MIN, DPI_MAX, DPI_STEP);
    scale.set_value(f64::from(DEFAULT_DPI));
    scale.set_draw_value(true);
    scale.set_hexpand(true);
    scale.set_sensitive(false);
    let dr = dpi_row.clone();
    scale.connect_value_changed(move |s| {
        dr.set_subtitle(&format!("{} DPI", scale_dpi(s.value())));
    });

    let apply = Button::with_label("Apply");
    apply.add_css_class("suggested-action");
    apply.add_css_class("pill");
    apply.set_sensitive(false);
    let sc = scale.clone();
    let to = toast.clone();
    apply.connect_clicked(move |_| {
        let dpi = scale_dpi(sc.value());
        report(
            ipc::try_request(&ipc::Request::SetDpi {
                value: dpi
            }),
            &to,
            &format!("DPI set to {dpi}")
        );
    });

    group.add(&dpi_row);
    let scale_box = GtkBox::new(Orientation::Horizontal, 12);
    scale_box.append(&scale);
    scale_box.append(&apply);
    scale_row.set_child(Some(&scale_box));
    group.add(&scale_row);

    scroll.set_child(Some(&group));

    let mut w = widgets.borrow_mut();
    w.dpi_row = Some(dpi_row);
    w.dpi_scale = Some(scale);
    w.dpi_apply = Some(apply);
    drop(w);
    scroll
}

/// Creates the device page: battery, backdrop, desktop integration and
/// version.
///
/// Model and battery start as "Detecting…"; the snapshot loader fills them
/// in once the daemon answers.
pub(super) fn device_page(
    state: &Rc<RefCell<HudState>>,
    window: &ApplicationWindow,
    toast: &ToastOverlay,
    widgets: &Rc<RefCell<PanelWidgets>>
) -> ScrolledWindow {
    let scroll = page_scroll();
    let box_ = page_body();

    let info_group = PreferencesGroup::new();
    info_group.set_title("Device");
    let name_row = ActionRow::new();
    name_row.set_title("Model");
    name_row.set_subtitle("Detecting…");
    let battery_row = ActionRow::new();
    battery_row.set_title("Battery");
    battery_row.set_subtitle("Detecting…");
    let refresh = Button::with_label("Refresh");
    refresh.add_css_class("pill");
    let widgets_refresh = Rc::clone(widgets);
    let to = toast.clone();
    refresh.connect_clicked(move |_| match ipc::try_request(&ipc::Request::Battery) {
        Some(Response::Battery {
            level,
            status
        }) => {
            apply_battery(&widgets_refresh.borrow(), &format!("{level}% · {status}"));
        }
        _ => {
            to.add_toast(Toast::new("Daemon unavailable"));
        }
    });
    battery_row.add_suffix(&refresh);
    info_group.add(&name_row);
    info_group.add(&battery_row);

    let integration_group = PreferencesGroup::new();
    integration_group.set_title("Desktop Integration");
    integration_group.set_description(Some("Overlay keybind for Hyprland"));
    let integration_row = ActionRow::new();
    integration_row.set_title("Hyprland");
    let configure = Button::with_label("Configure");
    configure.add_css_class("pill");
    let to = toast.clone();
    configure.connect_clicked(move |_| {
        let message = match crate::hyprland::ensure_integration() {
            Ok(crate::hyprland::Outcome::Configured {
                path
            }) => {
                format!("Hyprland configured: {path}")
            }
            Ok(crate::hyprland::Outcome::AlreadyConfigured {
                path
            }) => {
                format!("Already configured: {path}")
            }
            Ok(crate::hyprland::Outcome::NotHyprland) => "Not running under Hyprland".to_string(),
            Ok(crate::hyprland::Outcome::NoConfig) => {
                "Hyprland config not found in ~/.config/hypr".to_string()
            }
            Err(e) => format!("Hyprland setup failed: {e}")
        };
        to.add_toast(Toast::new(&message));
    });
    integration_row.add_suffix(&configure);
    integration_group.add(&integration_row);

    let version_row = ActionRow::new();
    version_row.set_title("Version");
    version_row.set_subtitle(env!("CARGO_PKG_VERSION"));
    let version_group = PreferencesGroup::new();
    version_group.add(&version_row);

    box_.append(&info_group);
    box_.append(&background_section(state, window));
    box_.append(&integration_group);
    box_.append(&version_group);
    scroll.set_child(Some(&box_));

    let mut w = widgets.borrow_mut();
    w.model_row = Some(name_row);
    w.battery_row = Some(battery_row);
    drop(w);
    scroll
}
