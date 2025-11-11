// SPDX-FileCopyrightText: 2025 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

use gtk4::{Application, Box, Button, Label, Orientation, Scale, Switch, glib, prelude::*};
use libadwaita::{
    self as adw, ActionRow, ApplicationWindow, HeaderBar, PreferencesGroup, PreferencesPage,
    prelude::*
};
use logi_mx_driver::prelude::*;

const APP_ID: &str = "com.logitech.mx.configurator";

fn main() -> glib::ExitCode {
    let app = Application::builder().application_id(APP_ID).build();

    app.connect_activate(build_ui);

    app.run()
}

fn build_ui(app: &Application) {
    adw::init().expect("Failed to initialize libadwaita");

    let window = ApplicationWindow::builder()
        .application(app)
        .title("Logitech MX Master 3S")
        .resizable(false)
        .default_width(450)
        .default_height(550)
        .build();

    let header = HeaderBar::new();

    let main_box = Box::new(Orientation::Vertical, 0);
    main_box.append(&header);

    let prefs_page = PreferencesPage::new();

    let device_info = create_device_info_group();
    prefs_page.add(&device_info);

    let dpi_group = create_dpi_group();
    prefs_page.add(&dpi_group);

    let smartshift_group = create_smartshift_group();
    prefs_page.add(&smartshift_group);

    let scroll_group = create_scroll_group();
    prefs_page.add(&scroll_group);

    let battery_group = create_battery_group();
    prefs_page.add(&battery_group);

    main_box.append(&prefs_page);

    window.set_content(Some(&main_box));
    window.present();
}

fn create_device_info_group() -> PreferencesGroup {
    let group = PreferencesGroup::new();
    group.set_title("Device Information");
    group.set_description(Some("Connected device details"));

    let name_row = ActionRow::new();
    name_row.set_title("Device Name");

    match MxMaster3s::open_bolt_receiver(2).and_then(|mut d| d.get_device_name()) {
        Ok(name) => name_row.set_subtitle(&name),
        Err(_) => name_row.set_subtitle("Not connected")
    }

    group.add(&name_row);

    group
}

fn create_battery_group() -> PreferencesGroup {
    let group = PreferencesGroup::new();
    group.set_title("Battery");
    group.set_description(Some("Current battery status"));

    let battery_row = ActionRow::new();
    battery_row.set_title("Battery Level");

    match MxMaster3s::open_bolt_receiver(2).and_then(|mut d| d.get_battery_info()) {
        Ok(battery) => {
            battery_row.set_subtitle(&format!("{}% - {:?}", battery.level, battery.status))
        }
        Err(_) => battery_row.set_subtitle("Unknown")
    }

    group.add(&battery_row);

    let refresh_btn = Button::with_label("Refresh");
    let br = battery_row.clone();
    refresh_btn.connect_clicked(move |_| {
        if let Ok(mut device) = MxMaster3s::open_bolt_receiver(2)
            && let Ok(battery) = device.get_battery_info() {
                br.set_subtitle(&format!("{}% - {:?}", battery.level, battery.status));
            }
    });

    let button_row = ActionRow::new();
    button_row.add_suffix(&refresh_btn);
    group.add(&button_row);

    group
}

fn create_dpi_group() -> PreferencesGroup {
    let group = PreferencesGroup::new();
    group.set_title("DPI Settings");
    group.set_description(Some("Pointer sensitivity (400-8000)"));

    let dpi_row = ActionRow::new();
    dpi_row.set_title("Current DPI");

    let current_dpi = MxMaster3s::open_bolt_receiver(2)
        .and_then(|mut d| d.get_dpi())
        .unwrap_or(1000);

    let dpi_label = Label::new(Some(&current_dpi.to_string()));
    dpi_row.add_suffix(&dpi_label);

    let scale = Scale::with_range(Orientation::Horizontal, 400.0, 8000.0, 100.0);
    scale.set_value(current_dpi as f64);
    scale.set_draw_value(false);
    scale.set_hexpand(true);

    let lbl = dpi_label.clone();
    scale.connect_value_changed(move |scale| {
        let value = scale.value() as u16;
        lbl.set_text(&value.to_string());
    });

    let scale_row = ActionRow::new();
    scale_row.set_child(Some(&scale));

    let apply_btn = Button::with_label("Apply DPI");
    let sc = scale.clone();
    apply_btn.connect_clicked(move |_| {
        let dpi = sc.value() as u16;
        if let Ok(mut device) = MxMaster3s::open_bolt_receiver(2)
            && device.set_dpi(dpi).is_ok() {
                println!("DPI set to {}", dpi);
            }
    });

    let button_row = ActionRow::new();
    button_row.add_suffix(&apply_btn);

    group.add(&dpi_row);
    group.add(&scale_row);
    group.add(&button_row);

    group
}

fn create_smartshift_group() -> PreferencesGroup {
    let group = PreferencesGroup::new();
    group.set_title("SmartShift");
    group.set_description(Some("Automatic wheel mode switching"));

    let current_config = MxMaster3s::open_bolt_receiver(2)
        .and_then(|mut d| d.get_smartshift())
        .unwrap_or_default();

    let switch_row = ActionRow::new();
    switch_row.set_title("Enable SmartShift");
    switch_row.set_subtitle("Auto-switch between ratchet and freespin");

    let switch = Switch::new();
    switch.set_active(current_config.enabled);
    switch_row.add_suffix(&switch);
    switch_row.set_activatable_widget(Some(&switch));

    let threshold_row = ActionRow::new();
    threshold_row.set_title("Threshold");
    threshold_row.set_subtitle("Speed to trigger auto-switch (1-50)");

    let threshold_label = Label::new(Some(&current_config.threshold.to_string()));
    threshold_row.add_suffix(&threshold_label);

    let threshold_scale = Scale::with_range(Orientation::Horizontal, 1.0, 50.0, 1.0);
    threshold_scale.set_value(current_config.threshold as f64);
    threshold_scale.set_draw_value(false);
    threshold_scale.set_hexpand(true);

    let tl = threshold_label.clone();
    threshold_scale.connect_value_changed(move |scale| {
        tl.set_text(&(scale.value() as u8).to_string());
    });

    let threshold_scale_row = ActionRow::new();
    threshold_scale_row.set_child(Some(&threshold_scale));

    let apply_btn = Button::with_label("Apply SmartShift");
    let sw = switch.clone();
    let ts = threshold_scale.clone();
    apply_btn.connect_clicked(move |_| {
        let enabled = sw.is_active();
        let threshold = ts.value() as u8;

        if let Ok(mut device) = MxMaster3s::open_bolt_receiver(2) {
            let config = SmartShiftConfig {
                enabled,
                threshold
            };
            if device.set_smartshift(config).is_ok() {
                println!(
                    "SmartShift configured: enabled={}, threshold={}",
                    enabled, threshold
                );
            }
        }
    });

    let button_row = ActionRow::new();
    button_row.add_suffix(&apply_btn);

    group.add(&switch_row);
    group.add(&threshold_row);
    group.add(&threshold_scale_row);
    group.add(&button_row);

    group
}

fn create_scroll_group() -> PreferencesGroup {
    let group = PreferencesGroup::new();
    group.set_title("Scroll Settings");
    group.set_description(Some("High-resolution scrolling"));

    let current_config = MxMaster3s::open_bolt_receiver(2)
        .and_then(|mut d| d.get_hires_scroll())
        .unwrap_or_default();

    let hires_row = ActionRow::new();
    hires_row.set_title("Hi-Res Scrolling");
    hires_row.set_subtitle("Smooth pixel-precise scrolling");

    let hires_switch = Switch::new();
    hires_switch.set_active(current_config.enabled);
    hires_row.add_suffix(&hires_switch);
    hires_row.set_activatable_widget(Some(&hires_switch));

    let inverted_row = ActionRow::new();
    inverted_row.set_title("Inverted Scrolling");
    inverted_row.set_subtitle("Natural scrolling direction");

    let inverted_switch = Switch::new();
    inverted_switch.set_active(current_config.inverted);
    inverted_row.add_suffix(&inverted_switch);
    inverted_row.set_activatable_widget(Some(&inverted_switch));

    let apply_btn = Button::with_label("Apply Scroll Settings");
    let hs = hires_switch.clone();
    let is = inverted_switch.clone();
    apply_btn.connect_clicked(move |_| {
        let enabled = hs.is_active();
        let inverted = is.is_active();

        if let Ok(mut device) = MxMaster3s::open_bolt_receiver(2) {
            let config = HiResScrollConfig {
                enabled,
                inverted
            };
            if device.set_hires_scroll(config).is_ok() {
                println!(
                    "Scroll configured: hi-res={}, inverted={}",
                    enabled, inverted
                );
            }
        }
    });

    let button_row = ActionRow::new();
    button_row.add_suffix(&apply_btn);

    group.add(&hires_row);
    group.add(&inverted_row);
    group.add(&button_row);

    group
}
