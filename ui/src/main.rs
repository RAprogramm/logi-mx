// SPDX-FileCopyrightText: 2025 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

use gtk4::{Box, Button, Image, Orientation, Scale, Switch, glib, prelude::*};
use libadwaita::{
    ActionRow, Application, ApplicationWindow, Clamp, HeaderBar, PreferencesGroup,
    PreferencesPage, StatusPage, Toast, ToastOverlay, prelude::*
};
use logi_mx_driver::prelude::*;

const APP_ID: &str = "com.logitech.mx.configurator";

fn main() -> glib::ExitCode {
    let app = Application::builder().application_id(APP_ID).build();
    app.connect_activate(build_ui);
    app.run()
}

fn build_ui(app: &Application) {
    let window = ApplicationWindow::builder()
        .application(app)
        .title("Logitech MX Master 3S")
        .default_width(800)
        .default_height(700)
        .build();

    let header = HeaderBar::new();
    header.set_show_end_title_buttons(true);

    let toast_overlay = ToastOverlay::new();

    // Check device connection
    let content = if let Ok(mut device) = MxMaster3s::open_bolt_receiver(2) {
        let name = device
            .get_device_name()
            .unwrap_or_else(|_| "MX Master 3S".to_string());
        create_connected_ui(&name, toast_overlay.clone())
    } else {
        create_disconnected_ui()
    };

    toast_overlay.set_child(Some(&content));

    let main_box = Box::new(Orientation::Vertical, 0);
    main_box.append(&header);
    main_box.append(&toast_overlay);

    window.set_content(Some(&main_box));

    // Ensure application exits when window is closed
    let app_weak = app.downgrade();
    window.connect_close_request(move |_| {
        if let Some(app) = app_weak.upgrade() {
            app.quit();
        }
        glib::Propagation::Proceed
    });

    window.present();
}

fn create_disconnected_ui() -> Box {
    let status_page = StatusPage::new();
    status_page.set_icon_name(Some("input-mouse-symbolic"));
    status_page.set_title("Device Not Connected");
    status_page.set_description(Some(
        "Please connect your Logitech MX Master 3S via Bolt receiver"
    ));

    let main_box = Box::new(Orientation::Vertical, 0);
    main_box.append(&status_page);
    main_box.set_vexpand(true);
    main_box
}

fn create_connected_ui(device_name: &str, toast_overlay: ToastOverlay) -> Box {
    let scrolled = gtk4::ScrolledWindow::new();
    scrolled.set_vexpand(true);
    scrolled.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);

    let clamp = Clamp::new();
    clamp.set_maximum_size(800);
    clamp.set_tightening_threshold(600);

    let prefs_page = PreferencesPage::new();
    prefs_page.set_margin_top(24);
    prefs_page.set_margin_bottom(24);
    prefs_page.set_margin_start(12);
    prefs_page.set_margin_end(12);

    // Device Info
    let device_info = create_device_info_group(device_name);
    prefs_page.add(&device_info);

    // Battery
    let battery_group = create_battery_group(toast_overlay.clone());
    prefs_page.add(&battery_group);

    // DPI
    let dpi_group = create_dpi_group(toast_overlay.clone());
    prefs_page.add(&dpi_group);

    // SmartShift
    let smartshift_group = create_smartshift_group(toast_overlay.clone());
    prefs_page.add(&smartshift_group);

    // Scroll
    let scroll_group = create_scroll_group(toast_overlay);
    prefs_page.add(&scroll_group);

    clamp.set_child(Some(&prefs_page));
    scrolled.set_child(Some(&clamp));

    let main_box = Box::new(Orientation::Vertical, 0);
    main_box.append(&scrolled);
    main_box
}

fn create_device_info_group(name: &str) -> PreferencesGroup {
    let group = PreferencesGroup::new();
    group.set_title("Device Information");

    let name_row = ActionRow::new();
    name_row.add_prefix(&Image::from_icon_name("input-mouse-symbolic"));
    name_row.set_title("Device");
    name_row.set_subtitle(name);

    let connection_row = ActionRow::new();
    connection_row.add_prefix(&Image::from_icon_name("network-wireless-symbolic"));
    connection_row.set_title("Connection");
    connection_row.set_subtitle("Bolt Receiver");

    group.add(&name_row);
    group.add(&connection_row);

    group
}

fn create_battery_group(toast_overlay: ToastOverlay) -> PreferencesGroup {
    let group = PreferencesGroup::new();
    group.set_title("Battery");
    group.set_description(Some("Monitor battery status and charging"));

    let battery_icon = Image::from_icon_name("battery-symbolic");
    let battery_row = ActionRow::new();
    battery_row.add_prefix(&battery_icon);
    battery_row.set_title("Battery Level");

    match MxMaster3s::open_bolt_receiver(2).and_then(|mut d| d.get_battery_info()) {
        Ok(battery) => {
            let icon = match battery.level {
                0..=20 => "battery-level-0-symbolic",
                21..=40 => "battery-level-20-symbolic",
                41..=60 => "battery-level-40-symbolic",
                61..=80 => "battery-level-60-symbolic",
                81..=90 => "battery-level-80-symbolic",
                _ => "battery-level-100-symbolic"
            };
            battery_icon.set_icon_name(Some(icon));
            battery_row.set_subtitle(&format!("{}% · {:?}", battery.level, battery.status));
        }
        Err(_) => battery_row.set_subtitle("Unable to read")
    }

    let refresh_btn = Button::with_label("Refresh");
    refresh_btn.add_css_class("pill");
    let br = battery_row.clone();
    let bi = battery_icon.clone();
    let to = toast_overlay.clone();
    refresh_btn.connect_clicked(move |_| {
        if let Ok(mut device) = MxMaster3s::open_bolt_receiver(2)
            && let Ok(battery) = device.get_battery_info()
        {
            let icon = match battery.level {
                0..=20 => "battery-level-0-symbolic",
                21..=40 => "battery-level-20-symbolic",
                41..=60 => "battery-level-40-symbolic",
                61..=80 => "battery-level-60-symbolic",
                81..=90 => "battery-level-80-symbolic",
                _ => "battery-level-100-symbolic"
            };
            bi.set_icon_name(Some(icon));
            br.set_subtitle(&format!("{}% · {:?}", battery.level, battery.status));

            let toast = Toast::new("Battery status updated");
            to.add_toast(toast);
        }
    });

    battery_row.add_suffix(&refresh_btn);
    battery_row.set_activatable_widget(Some(&refresh_btn));

    group.add(&battery_row);
    group
}

fn create_dpi_group(toast_overlay: ToastOverlay) -> PreferencesGroup {
    let group = PreferencesGroup::new();
    group.set_title("Pointer Sensitivity");
    group.set_description(Some("Adjust cursor speed from 400 to 8000 DPI"));

    let current_dpi = MxMaster3s::open_bolt_receiver(2)
        .and_then(|mut d| d.get_dpi())
        .unwrap_or(1000);

    let dpi_row = ActionRow::new();
    dpi_row.add_prefix(&Image::from_icon_name(
        "preferences-desktop-pointing-symbolic"
    ));
    dpi_row.set_title("Current DPI");
    dpi_row.set_subtitle(&format!("{} DPI", current_dpi));

    group.add(&dpi_row);

    let scale_row = ActionRow::new();
    scale_row.set_title("Sensitivity");

    let scale = Scale::with_range(Orientation::Horizontal, 400.0, 8000.0, 100.0);
    scale.set_value(current_dpi as f64);
    scale.set_draw_value(true);
    scale.set_value_pos(gtk4::PositionType::Right);
    scale.set_hexpand(true);
    scale.set_width_request(400);

    let dr = dpi_row.clone();
    scale.connect_value_changed(move |s| {
        let value = s.value() as u16;
        dr.set_subtitle(&format!("{} DPI", value));
    });

    let scale_box = Box::new(Orientation::Horizontal, 12);
    scale_box.append(&scale);

    let apply_btn = Button::with_label("Apply");
    apply_btn.add_css_class("suggested-action");
    apply_btn.add_css_class("pill");

    let sc = scale.clone();
    let to = toast_overlay.clone();
    apply_btn.connect_clicked(move |_| {
        let dpi = sc.value() as u16;
        if let Ok(mut device) = MxMaster3s::open_bolt_receiver(2)
            && device.set_dpi(dpi).is_ok()
        {
            let toast = Toast::new(&format!("DPI set to {}", dpi));
            to.add_toast(toast);
        }
    });

    scale_box.append(&apply_btn);
    scale_row.set_child(Some(&scale_box));

    group.add(&scale_row);
    group
}

fn create_smartshift_group(toast_overlay: ToastOverlay) -> PreferencesGroup {
    let group = PreferencesGroup::new();
    group.set_title("SmartShift");
    group.set_description(Some(
        "Automatic switching between ratchet and freespin modes"
    ));

    let current_config = MxMaster3s::open_bolt_receiver(2)
        .and_then(|mut d| d.get_smartshift())
        .unwrap_or_default();

    let switch_row = ActionRow::new();
    switch_row.add_prefix(&Image::from_icon_name("weather-windy-symbolic"));
    switch_row.set_title("Enable SmartShift");
    switch_row.set_subtitle("Auto-switch based on scroll speed");

    let switch = Switch::new();
    switch.set_valign(gtk4::Align::Center);
    switch.set_active(current_config.enabled);
    switch_row.add_suffix(&switch);
    switch_row.set_activatable_widget(Some(&switch));

    group.add(&switch_row);

    let threshold_row = ActionRow::new();
    threshold_row.set_title("Sensitivity Threshold");
    threshold_row.set_subtitle(&format!("Current: {}", current_config.threshold));

    let threshold_scale = Scale::with_range(Orientation::Horizontal, 1.0, 50.0, 1.0);
    threshold_scale.set_value(current_config.threshold as f64);
    threshold_scale.set_draw_value(true);
    threshold_scale.set_value_pos(gtk4::PositionType::Right);
    threshold_scale.set_hexpand(true);
    threshold_scale.set_width_request(400);

    let tr = threshold_row.clone();
    threshold_scale.connect_value_changed(move |s| {
        let value = s.value() as u8;
        tr.set_subtitle(&format!("Current: {}", value));
    });

    let threshold_box = Box::new(Orientation::Horizontal, 12);
    threshold_box.append(&threshold_scale);

    let apply_btn = Button::with_label("Apply");
    apply_btn.add_css_class("suggested-action");
    apply_btn.add_css_class("pill");

    let sw = switch.clone();
    let ts = threshold_scale.clone();
    let to = toast_overlay.clone();
    apply_btn.connect_clicked(move |_| {
        let config = SmartShiftConfig {
            enabled:   sw.is_active(),
            threshold: ts.value() as u8
        };

        if let Ok(mut device) = MxMaster3s::open_bolt_receiver(2)
            && device.set_smartshift(config).is_ok()
        {
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
    });

    threshold_box.append(&apply_btn);
    threshold_row.set_child(Some(&threshold_box));

    group.add(&threshold_row);
    group
}

fn create_scroll_group(toast_overlay: ToastOverlay) -> PreferencesGroup {
    let group = PreferencesGroup::new();
    group.set_title("Scroll Settings");
    group.set_description(Some("Configure high-resolution and natural scrolling"));

    let current_config = MxMaster3s::open_bolt_receiver(2)
        .and_then(|mut d| d.get_hires_scroll())
        .unwrap_or_default();

    let hires_row = ActionRow::new();
    hires_row.add_prefix(&Image::from_icon_name("view-continuous-symbolic"));
    hires_row.set_title("Hi-Res Scrolling");
    hires_row.set_subtitle("Smooth pixel-precise scrolling");

    let hires_switch = Switch::new();
    hires_switch.set_valign(gtk4::Align::Center);
    hires_switch.set_active(current_config.enabled);
    hires_row.add_suffix(&hires_switch);
    hires_row.set_activatable_widget(Some(&hires_switch));

    let inverted_row = ActionRow::new();
    inverted_row.add_prefix(&Image::from_icon_name("object-flip-vertical-symbolic"));
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

    let hs = hires_switch.clone();
    let is = inverted_switch.clone();
    let to = toast_overlay;
    apply_btn.connect_clicked(move |_| {
        let config = HiResScrollConfig {
            enabled:  hs.is_active(),
            inverted: is.is_active()
        };

        if let Ok(mut device) = MxMaster3s::open_bolt_receiver(2)
            && device.set_hires_scroll(config).is_ok()
        {
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
    });

    apply_row.add_suffix(&apply_btn);
    apply_row.set_activatable_widget(Some(&apply_btn));

    group.add(&hires_row);
    group.add(&inverted_row);
    group.add(&apply_row);

    group
}
