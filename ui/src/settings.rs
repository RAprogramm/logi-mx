// SPDX-FileCopyrightText: 2025 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

use gtk4::{Box, Button, Orientation, Scale, Switch, glib, prelude::*};
use libadwaita::{
    ActionRow, Application, ApplicationWindow, Banner, HeaderBar, PreferencesGroup,
    PreferencesPage, StatusPage, Toast, ToastOverlay, ToolbarView, ViewStack, ViewSwitcher,
    ViewSwitcherPolicy, WindowTitle, prelude::*
};
use logi_mx_daemon::ipc::{Request, Response, try_request};
use logi_mx_driver::prelude::*;

use crate::{
    hyprland::Outcome,
    snapshot::{DeviceSnapshot, collect_device_snapshot, parse_battery_status}
};

const DPI_MIN: f64 = 400.0;
const DPI_MAX: f64 = 8000.0;
const DPI_STEP: f64 = 100.0;
const SMARTSHIFT_MIN: f64 = 1.0;
const SMARTSHIFT_MAX: f64 = 255.0;
const SMARTSHIFT_STEP: f64 = 1.0;
const WIDGET_WIDTH: i32 = 400;

/// Creates an icon image with consistent vertical alignment to avoid GTK
/// baseline warnings and keep rows visually balanced.
fn icon(name: &str) -> gtk4::Image {
    let image = gtk4::Image::from_icon_name(name);
    image.set_valign(gtk4::Align::Center);
    image
}

/// Narrows a GTK scale reading to a DPI setting value.
///
/// The scale range is configured by this UI, so out-of-range readings cannot
/// occur in normal use; Rust's saturating float-to-integer cast keeps the
/// conversion total for pathological inputs.
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

/// Narrows a GTK scale reading to a `SmartShift` threshold value.
///
/// Same invariants as [`scale_dpi`]: the 1-50 scale range is owned here and
/// the cast saturates.
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

pub fn build_ui(app: &Application) {
    let window = ApplicationWindow::builder()
        .application(app)
        .title("Logitech MX Master 3S")
        .default_width(900)
        .default_height(700)
        .build();

    let header = HeaderBar::new();
    header.set_show_end_title_buttons(true);

    let toast_overlay = ToastOverlay::new();
    let toolbar = ToolbarView::new();
    toolbar.add_top_bar(&header);

    if let Some(snapshot) = collect_device_snapshot() {
        let banner = create_status_banner(&snapshot);
        toolbar.add_top_bar(&banner);

        let stack = create_connected_ui(&snapshot, &toast_overlay);
        let switcher = ViewSwitcher::builder()
            .stack(&stack)
            .policy(ViewSwitcherPolicy::Wide)
            .build();
        header.set_title_widget(Some(&switcher));

        toast_overlay.set_child(Some(&stack));
    } else {
        let title = WindowTitle::builder()
            .title("Logitech MX Master 3S")
            .build();
        header.set_title_widget(Some(&title));
        toast_overlay.set_child(Some(&create_disconnected_ui()));
    }

    toolbar.set_content(Some(&toast_overlay));
    window.set_content(Some(&toolbar));

    let app_weak = app.downgrade();
    window.connect_close_request(move |_| {
        if let Some(app) = app_weak.upgrade() {
            app.quit();
        }
        glib::Propagation::Proceed
    });

    window.present();
}

fn create_status_banner(snapshot: &DeviceSnapshot) -> Banner {
    let battery_text = snapshot.battery.map_or_else(
        || "unknown".to_string(),
        |b| format!("{}% · {:?}", b.level, b.status)
    );
    let banner = Banner::new(&format!(
        "Connected: {} · Battery: {} · DPI: {}",
        snapshot.name, battery_text, snapshot.dpi
    ));
    banner.add_css_class("status-banner");
    banner.set_valign(gtk4::Align::Start);
    banner
}

fn create_disconnected_ui() -> StatusPage {
    let status_page = StatusPage::new();
    status_page.set_icon_name(Some("input-mouse-symbolic"));
    status_page.set_title("Device Not Connected");
    status_page.set_description(Some(
        "Please connect your Logitech MX Master 3S via Bolt receiver"
    ));
    status_page.set_vexpand(true);
    status_page
}

fn create_connected_ui(snapshot: &DeviceSnapshot, toast_overlay: &ToastOverlay) -> ViewStack {
    let stack = ViewStack::new();

    let device_page = PreferencesPage::new();
    device_page.add_css_class("settings-page");
    device_page.add(&create_device_info_group(&snapshot.name));
    device_page.add(&create_battery_group(snapshot, toast_overlay));
    device_page.add(&create_integration_group(toast_overlay));
    stack.add_titled(&device_page, Some("device"), "Device");

    let pointer_page = PreferencesPage::new();
    pointer_page.add_css_class("settings-page");
    pointer_page.add(&create_dpi_group(snapshot, toast_overlay));
    stack.add_titled(&pointer_page, Some("pointer"), "Pointer");

    let wheel_page = PreferencesPage::new();
    wheel_page.add_css_class("settings-page");
    wheel_page.add(&create_smartshift_group(snapshot, toast_overlay));
    wheel_page.add(&create_scroll_group(snapshot, toast_overlay));
    stack.add_titled(&wheel_page, Some("wheel"), "Wheel");

    stack
}

fn create_device_info_group(name: &str) -> PreferencesGroup {
    let group = PreferencesGroup::new();
    group.set_title("Device Information");

    let name_row = ActionRow::new();
    name_row.add_prefix(&icon("input-mouse-symbolic"));
    name_row.set_title("Device");
    name_row.set_subtitle(name);

    let connection_row = ActionRow::new();
    connection_row.add_prefix(&icon("network-wireless-symbolic"));
    connection_row.set_title("Connection");
    connection_row.set_subtitle("Bolt Receiver");

    group.add(&name_row);
    group.add(&connection_row);

    group
}

fn create_battery_group(
    snapshot: &DeviceSnapshot,
    toast_overlay: &ToastOverlay
) -> PreferencesGroup {
    let group = PreferencesGroup::new();
    group.set_title("Battery");
    group.set_description(Some("Monitor battery status and charging"));

    let battery_icon = icon("battery-symbolic");
    let battery_row = ActionRow::new();
    battery_row.add_prefix(&battery_icon);
    battery_row.set_title("Battery Level");
    battery_row.set_tooltip_text(Some("Current charge level and charging state"));

    let battery_bar = gtk4::LevelBar::new();
    battery_bar.set_min_value(0.0);
    battery_bar.set_max_value(100.0);
    battery_bar.set_value(0.0);
    battery_bar.set_width_request(120);
    battery_bar.set_valign(gtk4::Align::Center);

    match snapshot.battery {
        Some(battery) => {
            battery_icon.set_icon_name(Some(battery_icon_name(battery.level)));
            battery_row.set_subtitle(&format!("{}% · {:?}", battery.level, battery.status));
            battery_bar.set_value(f64::from(battery.level));
        }
        None => battery_row.set_subtitle("Unable to read")
    }

    let refresh_btn = Button::with_label("Refresh");
    refresh_btn.add_css_class("pill");
    refresh_btn.set_tooltip_text(Some("Re-read battery status from the device"));
    let br = battery_row.clone();
    let bi = battery_icon;
    let bb = battery_bar.clone();
    let to = toast_overlay.clone();
    refresh_btn.connect_clicked(move |_| match try_request(&Request::Battery) {
        Some(Response::Battery {
            level,
            status
        }) => {
            let status_enum = parse_battery_status(&status);
            bi.set_icon_name(Some(battery_icon_name(level)));
            br.set_subtitle(&format!("{level}% · {status_enum:?}"));
            bb.set_value(f64::from(level));

            let toast = Toast::new("Battery status updated");
            to.add_toast(toast);
        }
        Some(Response::Error {
            message
        }) => {
            let toast = Toast::new(&format!("Failed to refresh battery: {message}"));
            to.add_toast(toast);
        }
        _ => {
            let toast = Toast::new("Daemon unavailable");
            to.add_toast(toast);
        }
    });

    battery_row.add_suffix(&battery_bar);
    battery_row.add_suffix(&refresh_btn);
    battery_row.set_activatable_widget(Some(&refresh_btn));

    group.add(&battery_row);
    group
}

/// Builds the desktop-integration group with the Hyprland setup button.
fn create_integration_group(toast_overlay: &ToastOverlay) -> PreferencesGroup {
    let group = PreferencesGroup::new();
    group.set_title("Desktop Integration");
    group.set_description(Some("Window rules and overlay keybind for Hyprland"));

    let row = ActionRow::new();
    row.add_prefix(&icon("preferences-system-symbolic"));
    row.set_title("Hyprland");
    row.set_subtitle("Float and center the settings window, bind the overlay");

    let button = Button::with_label("Configure");
    button.add_css_class("pill");
    button.set_tooltip_text(Some("Check the Hyprland config and apply the integration"));
    let to = toast_overlay.clone();
    button.connect_clicked(move |_| {
        let message = match crate::hyprland::ensure_integration() {
            Ok(Outcome::Configured {
                path
            }) => format!("Hyprland configured: {path}"),
            Ok(Outcome::AlreadyConfigured {
                path
            }) => format!("Already configured: {path}"),
            Ok(Outcome::NotHyprland) => "Not running under Hyprland".to_string(),
            Ok(Outcome::NoConfig) => "Hyprland config not found in ~/.config/hypr".to_string(),
            Err(e) => format!("Hyprland setup failed: {e}")
        };
        to.add_toast(Toast::new(&message));
    });

    row.add_suffix(&button);
    row.set_activatable_widget(Some(&button));

    group.add(&row);
    group
}

/// Maps a battery percentage to the matching symbolic icon name.
#[must_use]
const fn battery_icon_name(level: u8) -> &'static str {
    match level {
        0..=20 => "battery-level-0-symbolic",
        21..=40 => "battery-level-20-symbolic",
        41..=60 => "battery-level-40-symbolic",
        61..=80 => "battery-level-60-symbolic",
        81..=90 => "battery-level-80-symbolic",
        _ => "battery-level-100-symbolic"
    }
}

/// Builds the DPI adjustment group seeded from the snapshot.
///
/// Shows the current DPI and a slider that writes the selected value to the
/// device on `Apply`.
fn create_dpi_group(snapshot: &DeviceSnapshot, toast_overlay: &ToastOverlay) -> PreferencesGroup {
    let group = PreferencesGroup::new();
    group.set_title("Pointer Sensitivity");
    group.set_description(Some("Adjust cursor speed from 400 to 8000 DPI"));

    let current_dpi = snapshot.dpi;

    let dpi_row = ActionRow::new();
    dpi_row.add_prefix(&icon("input-mouse-symbolic"));
    dpi_row.set_title("Current DPI");
    dpi_row.set_subtitle(&format!("{current_dpi} DPI"));

    group.add(&dpi_row);

    let scale_row = ActionRow::new();
    scale_row.set_title("Sensitivity");

    let scale = Scale::with_range(Orientation::Horizontal, DPI_MIN, DPI_MAX, DPI_STEP);
    scale.set_value(f64::from(current_dpi));
    scale.set_draw_value(true);
    scale.set_value_pos(gtk4::PositionType::Right);
    scale.set_tooltip_text(Some("Pointer sensitivity in DPI"));
    scale.set_hexpand(true);
    scale.set_width_request(WIDGET_WIDTH);

    let dr = dpi_row;
    scale.connect_value_changed(move |s| {
        let value = scale_dpi(s.value());
        dr.set_subtitle(&format!("{value} DPI"));
    });

    let scale_box = Box::new(Orientation::Horizontal, 12);
    scale_box.append(&scale);

    let apply_btn = Button::with_label("Apply");
    apply_btn.add_css_class("suggested-action");
    apply_btn.add_css_class("pill");
    apply_btn.set_tooltip_text(Some("Apply the selected DPI to the device"));

    let sc = scale;
    let to = toast_overlay.clone();
    apply_btn.connect_clicked(move |_| {
        let dpi = scale_dpi(sc.value());
        match try_request(&Request::SetDpi {
            value: dpi
        }) {
            Some(Response::Ok) => {
                let toast = Toast::new(&format!("DPI set to {dpi}"));
                to.add_toast(toast);
            }
            Some(Response::Error {
                message
            }) => {
                let toast = Toast::new(&format!("Failed to set DPI: {message}"));
                to.add_toast(toast);
            }
            None => {
                let toast = Toast::new("Daemon unavailable");
                to.add_toast(toast);
            }
            _ => {
                let toast = Toast::new("Unexpected response");
                to.add_toast(toast);
            }
        }
    });

    scale_box.append(&apply_btn);
    scale_row.set_child(Some(&scale_box));

    group.add(&scale_row);
    group
}

/// Builds the `SmartShift` toggle and threshold group.
fn create_smartshift_group(
    snapshot: &DeviceSnapshot,
    toast_overlay: &ToastOverlay
) -> PreferencesGroup {
    let group = PreferencesGroup::new();
    group.set_title("SmartShift");
    group.set_description(Some(
        "Automatic switching between ratchet and freespin modes"
    ));

    let current_config = snapshot.smartshift;

    let switch_row = ActionRow::new();
    switch_row.add_prefix(&icon("weather-windy-symbolic"));
    switch_row.set_title("Enable SmartShift");
    switch_row.set_subtitle("Auto-switch based on scroll speed");

    let switch = Switch::new();
    switch.set_valign(gtk4::Align::Center);
    switch.set_active(current_config.enabled);
    switch_row.add_suffix(&switch);
    switch_row.set_activatable_widget(Some(&switch));

    group.add(&switch_row);

    let threshold_row = ActionRow::new();
    threshold_row.set_title("Disengage Sensitivity");
    threshold_row.set_subtitle(&format!("Current: {}", current_config.threshold));

    let threshold_scale = Scale::with_range(
        Orientation::Horizontal,
        SMARTSHIFT_MIN,
        SMARTSHIFT_MAX,
        SMARTSHIFT_STEP
    );
    threshold_scale.set_value(f64::from(current_config.threshold));
    threshold_scale.set_draw_value(true);
    threshold_scale.set_value_pos(gtk4::PositionType::Right);
    threshold_scale.set_tooltip_text(Some("Scroll speed threshold for SmartShift"));
    threshold_scale.set_hexpand(true);
    threshold_scale.set_width_request(WIDGET_WIDTH);

    let tr = threshold_row.clone();
    threshold_scale.connect_value_changed(move |s| {
        let value = scale_threshold(s.value());
        tr.set_subtitle(&format!("Current: {value}"));
    });

    let threshold_box = Box::new(Orientation::Horizontal, 12);
    threshold_box.append(&threshold_scale);

    let apply_btn = Button::with_label("Apply");
    apply_btn.add_css_class("suggested-action");
    apply_btn.add_css_class("pill");
    apply_btn.set_tooltip_text(Some("Apply SmartShift settings to the device"));

    let sw = switch;
    let ts = threshold_scale;
    let to = toast_overlay.clone();
    apply_btn.connect_clicked(move |_| {
        let config = SmartShiftConfig {
            enabled:   sw.is_active(),
            threshold: scale_threshold(ts.value())
        };

        match try_request(&Request::SetSmartShift {
            enabled:   config.enabled,
            threshold: config.threshold
        }) {
            Some(Response::Ok) => {
                let toast = Toast::new(&format!(
                    "SmartShift {} at threshold {}",
                    if config.enabled {
                        "enabled"
                    } else {
                        "disabled"
                    },
                    config.threshold
                ));
                to.add_toast(toast);
            }
            Some(Response::Error {
                message
            }) => {
                let toast = Toast::new(&format!("Failed to configure SmartShift: {message}"));
                to.add_toast(toast);
            }
            None => {
                let toast = Toast::new("Daemon unavailable");
                to.add_toast(toast);
            }
            _ => {
                let toast = Toast::new("Unexpected response");
                to.add_toast(toast);
            }
        }
    });

    threshold_box.append(&apply_btn);
    threshold_row.set_child(Some(&threshold_box));

    group.add(&threshold_row);
    group
}

fn create_scroll_group(
    snapshot: &DeviceSnapshot,
    toast_overlay: &ToastOverlay
) -> PreferencesGroup {
    let group = PreferencesGroup::new();
    group.set_title("Scroll Settings");
    group.set_description(Some("Configure high-resolution and natural scrolling"));

    let current_config = snapshot.hiresscroll;

    let hires_row = ActionRow::new();
    hires_row.add_prefix(&icon("view-continuous-symbolic"));
    hires_row.set_title("Hi-Res Scrolling");
    hires_row.set_subtitle("Smooth pixel-precise scrolling");

    let hires_switch = Switch::new();
    hires_switch.set_valign(gtk4::Align::Center);
    hires_switch.set_active(current_config.enabled);
    hires_row.add_suffix(&hires_switch);
    hires_row.set_activatable_widget(Some(&hires_switch));

    let inverted_row = ActionRow::new();
    inverted_row.add_prefix(&icon("object-flip-vertical-symbolic"));
    inverted_row.set_title("Natural Scrolling");
    inverted_row.set_subtitle("Invert scroll direction");

    let inverted_switch = Switch::new();
    inverted_switch.set_valign(gtk4::Align::Center);
    inverted_switch.set_active(current_config.inverted);
    inverted_row.add_suffix(&inverted_switch);
    inverted_row.set_activatable_widget(Some(&inverted_switch));

    let apply_row = ActionRow::new();
    let apply_btn = Button::with_label("Apply Settings");
    apply_btn.add_css_class("suggested-action");
    apply_btn.add_css_class("pill");
    apply_btn.set_tooltip_text(Some("Apply scroll settings to the device"));

    let hs = hires_switch;
    let is = inverted_switch;
    let to = toast_overlay.clone();
    apply_btn.connect_clicked(move |_| {
        let config = HiResScrollConfig {
            enabled:  hs.is_active(),
            inverted: is.is_active()
        };

        match try_request(&Request::SetHires {
            enabled:  config.enabled,
            inverted: config.inverted
        }) {
            Some(Response::Ok) => {
                let toast = Toast::new(&format!(
                    "Scroll: {} · {}",
                    if config.enabled { "Hi-Res" } else { "Normal" },
                    if config.inverted {
                        "Natural"
                    } else {
                        "Traditional"
                    }
                ));
                to.add_toast(toast);
            }
            Some(Response::Error {
                message
            }) => {
                let toast = Toast::new(&format!("Failed to configure scroll: {message}"));
                to.add_toast(toast);
            }
            None => {
                let toast = Toast::new("Daemon unavailable");
                to.add_toast(toast);
            }
            _ => {
                let toast = Toast::new("Unexpected response");
                to.add_toast(toast);
            }
        }
    });

    apply_row.add_suffix(&apply_btn);
    apply_row.set_activatable_widget(Some(&apply_btn));

    group.add(&hires_row);
    group.add(&inverted_row);
    group.add(&apply_row);

    group
}
