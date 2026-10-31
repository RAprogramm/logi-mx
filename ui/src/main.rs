// SPDX-FileCopyrightText: 2025 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

use std::{cell::RefCell, collections::HashMap, rc::Rc};

use gtk4::{
    glib, prelude::*, Box, Button, CssProvider, GestureClick, Label, Orientation, Scale, Switch,
};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use libadwaita::{
    prelude::*, ActionRow, Application, ApplicationWindow, Banner, HeaderBar, PreferencesGroup,
    PreferencesPage, StatusPage, Toast, ToastOverlay, ToolbarView, ViewStack, ViewSwitcher,
    ViewSwitcherPolicy, WindowTitle,
};
use logi_mx_daemon::ipc::{try_request, Request, Response};
use logi_mx_driver::prelude::*;

const APP_ID: &str = "com.logitech.mx.configurator";

const DPI_MIN: f64 = 400.0;
const DPI_MAX: f64 = 8000.0;
const DPI_STEP: f64 = 100.0;
const SMARTSHIFT_MIN: f64 = 1.0;
const SMARTSHIFT_MAX: f64 = 255.0;
const SMARTSHIFT_STEP: f64 = 1.0;
const WIDGET_WIDTH: i32 = 400;
const DEFAULT_DEVICE_NAME: &str = "MX Master 3S";
const DEFAULT_DPI: u16 = 1000;

/// Creates an icon image with consistent vertical alignment to avoid GTK
/// baseline warnings and keep rows visually balanced.
fn icon(name: &str) -> gtk4::Image {
    let image = gtk4::Image::from_icon_name(name);
    image.set_valign(gtk4::Align::Center);
    image
}

/// One-shot device state collected at startup.
///
/// The UI opens the device exactly once per launch and seeds every settings
/// group from this snapshot instead of re-opening the device four times.
#[derive(Debug, Clone)]
struct DeviceSnapshot {
    /// Marketing name reported by the device.
    name: String,
    /// Battery state, when the battery feature answered.
    battery: Option<BatteryInfo>,
    /// Currently applied DPI.
    dpi: u16,
    /// Current `SmartShift` configuration.
    smartshift: SmartShiftConfig,
    /// Current hi-res wheel configuration.
    hiresscroll: HiResScrollConfig,
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
    if let Some(Response::Info {
        name,
        dpi,
        smartshift,
        hires,
    }) = try_request(&Request::Info)
    {
        let battery = try_request(&Request::Battery).and_then(|r| match r {
            Response::Battery { level, status } => Some(BatteryInfo {
                level,
                status: parse_battery_status(&status),
            }),
            _ => None,
        });

        return Some(DeviceSnapshot {
            name,
            battery,
            dpi,
            smartshift,
            hiresscroll: hires,
        });
    }

    collect_device_snapshot_direct()
}

fn parse_battery_status(status: &str) -> BatteryStatus {
    match status {
        "Charging" => BatteryStatus::Charging,
        "Full" => BatteryStatus::Full,
        "Discharging" => BatteryStatus::Discharging,
        _ => BatteryStatus::Unknown,
    }
}

fn collect_device_snapshot_direct() -> Option<DeviceSnapshot> {
    let mut device = MxMaster3s::open_bolt_receiver_discovered().ok()?;

    let name = device
        .device_name()
        .unwrap_or_else(|_| DEFAULT_DEVICE_NAME.to_string());
    let battery = device.battery_info().ok();
    let dpi = device.dpi().unwrap_or(DEFAULT_DPI);
    let smartshift = device.smartshift().unwrap_or_default();
    let hiresscroll = device.hires_scroll().unwrap_or_default();

    Some(DeviceSnapshot {
        name,
        battery,
        dpi,
        smartshift,
        hiresscroll,
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
    if std::env::var("WAYLAND_DISPLAY").is_err() && std::env::var("DISPLAY").is_err() {
        eprintln!("No graphical session detected; logi-mx-ui requires Wayland or X11");
        return glib::ExitCode::FAILURE;
    }

    let app = Application::builder().application_id(APP_ID).build();
    app.connect_startup(|_| load_css());
    if std::env::var("LOGI_MX_OVERLAY").is_ok() {
        app.connect_activate(build_overlay);
    } else {
        app.connect_activate(build_ui);
    }
    app.run()
}

fn load_css() {
    let provider = CssProvider::new();
    provider.load_from_string(
        "
        .status-banner {
            padding: 6px 12px;
        }
        .settings-page {
            margin-top: 12px;
            margin-bottom: 24px;
        }
        .level-bar {
            min-height: 8px;
        }
        ",
    );
    gtk4::style_context_add_provider_for_display(
        &gtk4::gdk::Display::default().expect("could not connect to display"),
        &provider,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}

fn build_ui(app: &Application) {
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

    match collect_device_snapshot() {
        Some(snapshot) => {
            let banner = create_status_banner(&snapshot);
            toolbar.add_top_bar(&banner);

            let stack = create_connected_ui(&snapshot, &toast_overlay);
            let switcher = ViewSwitcher::builder()
                .stack(&stack)
                .policy(ViewSwitcherPolicy::Wide)
                .build();
            header.set_title_widget(Some(&switcher));

            toast_overlay.set_child(Some(&stack));
        }
        None => {
            let title = WindowTitle::builder()
                .title("Logitech MX Master 3S")
                .build();
            header.set_title_widget(Some(&title));
            toast_overlay.set_child(Some(&create_disconnected_ui()));
        }
    }

    toolbar.set_content(Some(&toast_overlay));
    window.set_content(Some(&toolbar));

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

fn create_status_banner(snapshot: &DeviceSnapshot) -> Banner {
    let battery_text = snapshot
        .battery
        .map(|b| format!("{}% · {:?}", b.level, b.status))
        .unwrap_or_else(|| "unknown".to_string());
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
        "Please connect your Logitech MX Master 3S via Bolt receiver",
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
    toast_overlay: &ToastOverlay,
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
        None => battery_row.set_subtitle("Unable to read"),
    }

    let refresh_btn = Button::with_label("Refresh");
    refresh_btn.add_css_class("pill");
    refresh_btn.set_tooltip_text(Some("Re-read battery status from the device"));
    let br = battery_row.clone();
    let bi = battery_icon;
    let bb = battery_bar.clone();
    let to = toast_overlay.clone();
    refresh_btn.connect_clicked(move |_| match try_request(&Request::Battery) {
        Some(Response::Battery { level, status }) => {
            let status_enum = parse_battery_status(&status);
            bi.set_icon_name(Some(battery_icon_name(level)));
            br.set_subtitle(&format!("{level}% · {status_enum:?}"));
            bb.set_value(f64::from(level));

            let toast = Toast::new("Battery status updated");
            to.add_toast(toast);
        }
        Some(Response::Error { message }) => {
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

/// Maps a battery percentage to the matching symbolic icon name.
#[must_use]
const fn battery_icon_name(level: u8) -> &'static str {
    match level {
        0..=20 => "battery-level-0-symbolic",
        21..=40 => "battery-level-20-symbolic",
        41..=60 => "battery-level-40-symbolic",
        61..=80 => "battery-level-60-symbolic",
        81..=90 => "battery-level-80-symbolic",
        _ => "battery-level-100-symbolic",
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
        match try_request(&Request::SetDpi { value: dpi }) {
            Some(Response::Ok) => {
                let toast = Toast::new(&format!("DPI set to {dpi}"));
                to.add_toast(toast);
            }
            Some(Response::Error { message }) => {
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
    toast_overlay: &ToastOverlay,
) -> PreferencesGroup {
    let group = PreferencesGroup::new();
    group.set_title("SmartShift");
    group.set_description(Some(
        "Automatic switching between ratchet and freespin modes",
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
        SMARTSHIFT_STEP,
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
            enabled: sw.is_active(),
            threshold: scale_threshold(ts.value()),
        };

        match try_request(&Request::SetSmartShift {
            enabled: config.enabled,
            threshold: config.threshold,
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
            Some(Response::Error { message }) => {
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
    toast_overlay: &ToastOverlay,
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
            enabled: hs.is_active(),
            inverted: is.is_active(),
        };

        match try_request(&Request::SetHires {
            enabled: config.enabled,
            inverted: config.inverted,
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
            Some(Response::Error { message }) => {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum MouseButton {
    Left,
    Right,
    Middle,
    Back,
    Forward,
    Gesture,
    ModeShift,
}

impl MouseButton {
    const ALL: [MouseButton; 7] = [
        MouseButton::Left,
        MouseButton::Right,
        MouseButton::Middle,
        MouseButton::Back,
        MouseButton::Forward,
        MouseButton::Gesture,
        MouseButton::ModeShift,
    ];

    const fn label(self) -> &'static str {
        match self {
            MouseButton::Left => "Left",
            MouseButton::Right => "Right",
            MouseButton::Middle => "Middle",
            MouseButton::Back => "Back",
            MouseButton::Forward => "Forward",
            MouseButton::Gesture => "Gesture",
            MouseButton::ModeShift => "Mode Shift",
        }
    }

    const fn position(self) -> (f64, f64) {
        match self {
            MouseButton::Left => (200.0, 150.0),
            MouseButton::Right => (400.0, 150.0),
            MouseButton::Middle => (300.0, 250.0),
            MouseButton::Back => (180.0, 350.0),
            MouseButton::Forward => (220.0, 350.0),
            MouseButton::Gesture => (180.0, 450.0),
            MouseButton::ModeShift => (300.0, 100.0),
        }
    }
}

#[derive(Default)]
struct OverlayState {
    selected: Option<MouseButton>,
    assignments: HashMap<MouseButton, String>,
}

fn build_overlay(app: &Application) {
    let window = ApplicationWindow::builder()
        .application(app)
        .title("Logitech MX Master 3S — Button Mapper")
        .default_width(1100)
        .default_height(700)
        .build();

    window.init_layer_shell();
    window.set_layer(Layer::Overlay);
    window.set_keyboard_mode(KeyboardMode::OnDemand);
    window.set_anchor(Edge::Top, true);
    window.set_anchor(Edge::Bottom, true);
    window.set_anchor(Edge::Left, true);
    window.set_anchor(Edge::Right, true);
    window.set_margin_top(80);
    window.set_margin_bottom(80);
    window.set_margin_start(80);
    window.set_margin_end(80);

    let state = Rc::new(RefCell::new(OverlayState::default()));

    let root = Box::new(Orientation::Horizontal, 0);
    root.add_css_class("background");

    let drawing = gtk4::DrawingArea::builder()
        .hexpand(true)
        .vexpand(true)
        .content_width(600)
        .content_height(600)
        .build();
    drawing.add_css_class("card");

    let panel = Box::new(Orientation::Vertical, 12);
    panel.set_margin_top(24);
    panel.set_margin_bottom(24);
    panel.set_margin_start(24);
    panel.set_margin_end(24);
    panel.set_size_request(360, -1);
    panel.add_css_class("card");

    let title = Label::new(Some("Button Mapper"));
    title.add_css_class("title-2");
    panel.append(&title);

    let selected_label = Label::new(Some("Select a button on the mouse diagram"));
    selected_label.set_wrap(true);
    selected_label.set_xalign(0.0);
    panel.append(&selected_label);

    let actions = [
        ("Toggle SmartShift", "Toggle SmartShift"),
        ("Keypress: Ctrl+C", "Keypress: Ctrl+C"),
        ("Keypress: Ctrl+V", "Keypress: Ctrl+V"),
        ("None", "None"),
    ];
    for (label, action) in actions {
        let btn = Button::with_label(label);
        btn.add_css_class("pill");
        let state_clone = Rc::clone(&state);
        let drawing_clone = drawing.clone();
        let selected_clone = selected_label.clone();
        let action = action.to_string();
        btn.connect_clicked(move |_| {
            let mut st = state_clone.borrow_mut();
            if let Some(button) = st.selected {
                st.assignments.insert(button, action.clone());
                selected_clone.set_text(&format!("{} assigned to {}", button.label(), action));
                drawing_clone.queue_draw();
            }
        });
        panel.append(&btn);
    }

    let close_btn = Button::with_label("Close");
    close_btn.add_css_class("flat");
    let win_close = window.clone();
    close_btn.connect_clicked(move |_| win_close.close());
    panel.append(&close_btn);

    let gesture = GestureClick::new();
    let state_gesture = Rc::clone(&state);
    let drawing_gesture = drawing.clone();
    let selected_gesture = selected_label.clone();
    gesture.connect_released(move |_gesture, _n_press, x, y| {
        let mut st = state_gesture.borrow_mut();
        for button in MouseButton::ALL {
            let (bx, by) = button.position();
            let dx = x - bx;
            let dy = y - by;
            if dx * dx + dy * dy < 40.0 * 40.0 {
                st.selected = Some(button);
                selected_gesture.set_text(&format!("Selected: {}", button.label()));
                drawing_gesture.queue_draw();
                return;
            }
        }
    });
    drawing.add_controller(gesture);

    let state_draw = Rc::clone(&state);
    drawing.set_draw_func(move |_, cr, width, height| {
        let st = state_draw.borrow();
        let scale_x = width as f64 / 600.0;
        let scale_y = height as f64 / 600.0;
        let scale = scale_x.min(scale_y);
        let offset_x = (width as f64 - 600.0 * scale) / 2.0;
        let offset_y = (height as f64 - 600.0 * scale) / 2.0;

        let _ = cr.save();
        cr.translate(offset_x, offset_y);
        cr.scale(scale, scale);

        // Mouse body
        cr.set_source_rgb(0.18, 0.18, 0.22);
        rounded_rect(cr, 120.0, 60.0, 360.0, 480.0, 120.0);
        let _ = cr.fill_preserve();
        cr.set_source_rgb(0.35, 0.35, 0.42);
        cr.set_line_width(3.0);
        let _ = cr.stroke();

        // Buttons
        for button in MouseButton::ALL {
            let (x, y) = button.position();
            let selected = st.selected == Some(button);
            let assigned = st.assignments.contains_key(&button);
            if selected {
                cr.set_source_rgb(0.95, 0.75, 0.65);
            } else if assigned {
                cr.set_source_rgb(0.55, 0.75, 0.95);
            } else {
                cr.set_source_rgb(0.30, 0.30, 0.36);
            }
            cr.arc(x, y, 36.0, 0.0, 2.0 * std::f64::consts::PI);
            let _ = cr.fill_preserve();
            cr.set_source_rgb(0.75, 0.75, 0.82);
            cr.set_line_width(2.0);
            let _ = cr.stroke();

            cr.set_source_rgb(0.95, 0.95, 0.98);
            cr.set_font_size(14.0);
            let text = button.label();
            if let Ok(extents) = cr.text_extents(text) {
                cr.move_to(x - extents.width() / 2.0, y + 5.0);
                let _ = cr.show_text(text);
            }
        }

        // Connection lines from selected button to panel
        if let Some(button) = st.selected {
            let (x, y) = button.position();
            cr.set_source_rgb(0.95, 0.75, 0.65);
            cr.set_line_width(2.0);
            cr.move_to(x + 36.0, y);
            cr.line_to(600.0, y);
            let _ = cr.stroke();
        }

        let _ = cr.restore();
    });

    root.append(&drawing);
    root.append(&panel);
    window.set_content(Some(&root));
    window.present();
}

fn rounded_rect(cr: &gtk4::cairo::Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    cr.new_sub_path();
    cr.arc(x + w - r, y + r, r, -std::f64::consts::FRAC_PI_2, 0.0);
    cr.arc(x + w - r, y + h - r, r, 0.0, std::f64::consts::FRAC_PI_2);
    cr.arc(
        x + r,
        y + h - r,
        r,
        std::f64::consts::FRAC_PI_2,
        std::f64::consts::PI,
    );
    cr.arc(
        x + r,
        y + r,
        r,
        std::f64::consts::PI,
        3.0 * std::f64::consts::FRAC_PI_2,
    );
    cr.close_path();
}
