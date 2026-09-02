// SPDX-FileCopyrightText: 2025 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

use gtk4::{Box, Button, Image, Orientation, Scale, Switch, glib, prelude::*};
use libadwaita::{
    ActionRow, Application, ApplicationWindow, Clamp, HeaderBar, PreferencesGroup,
    PreferencesPage, StatusPage, Toast, ToastOverlay, prelude::*
};
use logi_mx_driver::prelude::*;

const APP_ID: &str = "com.logitech.mx.configurator";

/// One-shot device state collected at startup.
///
/// The UI opens the device exactly once per launch and seeds every settings
/// group from this snapshot instead of re-opening the device four times.
#[derive(Debug, Clone)]
struct DeviceSnapshot {
    /// Marketing name reported by the device.
    name:        String,
    /// Battery state, when the battery feature answered.
    battery:     Option<BatteryInfo>,
    /// Currently applied DPI.
    dpi:         u16,
    /// Current `SmartShift` configuration.
    smartshift:  SmartShiftConfig,
    /// Current hi-res wheel configuration.
    hiresscroll: HiResScrollConfig
}

/// Opens the device once and reads every supported property.
///
/// # Returns
///
/// `Some` snapshot when the device opened, `None` when it is unreachable.
///
/// # Examples
///
/// ```no_run
/// # use logi_mx_driver::prelude::*;
/// // Mirrors the UI startup path: a single open, a single set of reads.
/// let mut device = MxMaster3s::open_bolt_receiver_discovered()?;
/// let name = device.device_name()?;
/// # Ok::<(), masterror::AppError>(())
/// ```
fn collect_device_snapshot() -> Option<DeviceSnapshot> {
    let mut device = MxMaster3s::open_bolt_receiver_discovered().ok()?;

    let name = device
        .device_name()
        .unwrap_or_else(|_| "MX Master 3S".to_string());
    let battery = device.battery_info().ok();
    let dpi = device.dpi().unwrap_or(1000);
    let smartshift = device.smartshift().unwrap_or_default();
    let hiresscroll = device.hires_scroll().unwrap_or_default();

    Some(DeviceSnapshot {
        name,
        battery,
        dpi,
        smartshift,
        hiresscroll
    })
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

    // Collect a single device snapshot instead of opening the device in
    // every settings group
    let content = collect_device_snapshot().map_or_else(create_disconnected_ui, |snapshot| {
        create_connected_ui(&snapshot, &toast_overlay)
    });

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

fn create_connected_ui(snapshot: &DeviceSnapshot, toast_overlay: &ToastOverlay) -> Box {
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
    let device_info = create_device_info_group(&snapshot.name);
    prefs_page.add(&device_info);

    // Battery
    let battery_group = create_battery_group(snapshot, toast_overlay);
    prefs_page.add(&battery_group);

    // DPI
    let dpi_group = create_dpi_group(snapshot, toast_overlay);
    prefs_page.add(&dpi_group);

    // SmartShift
    let smartshift_group = create_smartshift_group(snapshot, toast_overlay);
    prefs_page.add(&smartshift_group);

    // Scroll
    let scroll_group = create_scroll_group(snapshot, toast_overlay);
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

fn create_battery_group(
    snapshot: &DeviceSnapshot,
    toast_overlay: &ToastOverlay
) -> PreferencesGroup {
    let group = PreferencesGroup::new();
    group.set_title("Battery");
    group.set_description(Some("Monitor battery status and charging"));

    let battery_icon = Image::from_icon_name("battery-symbolic");
    let battery_row = ActionRow::new();
    battery_row.add_prefix(&battery_icon);
    battery_row.set_title("Battery Level");

    match snapshot.battery {
        Some(battery) => {
            battery_icon.set_icon_name(Some(battery_icon_name(battery.level)));
            battery_row.set_subtitle(&format!("{}% · {:?}", battery.level, battery.status));
        }
        None => battery_row.set_subtitle("Unable to read")
    }

    let refresh_btn = Button::with_label("Refresh");
    refresh_btn.add_css_class("pill");
    let br = battery_row.clone();
    let bi = battery_icon;
    let to = toast_overlay.clone();
    refresh_btn.connect_clicked(move |_| {
        if let Ok(mut device) = MxMaster3s::open_bolt_receiver_discovered()
            && let Ok(battery) = device.battery_info()
        {
            bi.set_icon_name(Some(battery_icon_name(battery.level)));
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

fn create_dpi_group(snapshot: &DeviceSnapshot, toast_overlay: &ToastOverlay) -> PreferencesGroup {
    let group = PreferencesGroup::new();
    group.set_title("Pointer Sensitivity");
    group.set_description(Some("Adjust cursor speed from 400 to 8000 DPI"));

    let current_dpi = snapshot.dpi;

    let dpi_row = ActionRow::new();
    dpi_row.add_prefix(&Image::from_icon_name(
        "preferences-desktop-pointing-symbolic"
    ));
    dpi_row.set_title("Current DPI");
    dpi_row.set_subtitle(&format!("{current_dpi} DPI"));

    group.add(&dpi_row);

    let scale_row = ActionRow::new();
    scale_row.set_title("Sensitivity");

    let scale = Scale::with_range(Orientation::Horizontal, 400.0, 8000.0, 100.0);
    scale.set_value(f64::from(current_dpi));
    scale.set_draw_value(true);
    scale.set_value_pos(gtk4::PositionType::Right);
    scale.set_hexpand(true);
    scale.set_width_request(400);

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

    let sc = scale;
    let to = toast_overlay.clone();
    apply_btn.connect_clicked(move |_| {
        let dpi = scale_dpi(sc.value());
        if let Ok(mut device) = MxMaster3s::open_bolt_receiver_discovered()
            && device.set_dpi(dpi).is_ok()
        {
            let toast = Toast::new(&format!("DPI set to {dpi}"));
            to.add_toast(toast);
        }
    });

    scale_box.append(&apply_btn);
    scale_row.set_child(Some(&scale_box));

    group.add(&scale_row);
    group
}

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
    threshold_row.set_title("Disengage Sensitivity");
    threshold_row.set_subtitle(&format!("Current: {}", current_config.threshold));

    let threshold_scale = Scale::with_range(Orientation::Horizontal, 1.0, 255.0, 1.0);
    threshold_scale.set_value(f64::from(current_config.threshold));
    threshold_scale.set_draw_value(true);
    threshold_scale.set_value_pos(gtk4::PositionType::Right);
    threshold_scale.set_hexpand(true);
    threshold_scale.set_width_request(400);

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

    let sw = switch;
    let ts = threshold_scale;
    let to = toast_overlay.clone();
    apply_btn.connect_clicked(move |_| {
        let config = SmartShiftConfig {
            enabled:   sw.is_active(),
            threshold: scale_threshold(ts.value())
        };

        if let Ok(mut device) = MxMaster3s::open_bolt_receiver_discovered()
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

fn create_scroll_group(
    snapshot: &DeviceSnapshot,
    toast_overlay: &ToastOverlay
) -> PreferencesGroup {
    let group = PreferencesGroup::new();
    group.set_title("Scroll Settings");
    group.set_description(Some("Configure high-resolution and natural scrolling"));

    let current_config = snapshot.hiresscroll;

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

    let hs = hires_switch;
    let is = inverted_switch;
    let to = toast_overlay.clone();
    apply_btn.connect_clicked(move |_| {
        let config = HiResScrollConfig {
            enabled:  hs.is_active(),
            inverted: is.is_active()
        };

        if let Ok(mut device) = MxMaster3s::open_bolt_receiver_discovered()
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
