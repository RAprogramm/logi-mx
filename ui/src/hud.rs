// SPDX-FileCopyrightText: 2025-2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Full-screen HUD: the only window of `logi-mx-ui`.
//!
//! The device diagram is the reference MX Master 3S layout SVG rendered with
//! `resvg` on a transparent surface. Every button is called out from its SVG
//! anchor with a leader line labelled with what it does. Live kernel input
//! events streamed by the daemon highlight the exact pressed button shapes
//! and show wheel direction and velocity, so the device can be
//! verified visually. A settings panel on the right exposes DPI, scroll
//! behaviour, battery and desktop integration. Toggled by the tray or the
//! configured keybind; a second activation closes it.

use std::{
    cell::{OnceCell, RefCell},
    collections::HashMap,
    io::BufReader,
    rc::Rc,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, OnceLock,
    },
    thread,
    time::Duration,
};

use gtk4::{
    glib, Box as GtkBox, Button, EventControllerKey, GestureClick, Label, Orientation, Scale,
    ScrolledWindow, Switch,
};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use libadwaita::{
    prelude::*, ActionRow, Application, ApplicationWindow, PreferencesGroup, Toast, ToastOverlay,
    ViewStack, ViewSwitcher, ViewSwitcherPolicy,
};
use logi_mx_daemon::ipc::{
    self, LiveFrame, Response, BTN_BIT_BACK, BTN_BIT_FORWARD, BTN_BIT_GESTURE, BTN_BIT_LEFT,
    BTN_BIT_MIDDLE, BTN_BIT_RIGHT,
};
use logi_mx_driver::{
    devices::control_id,
    prelude::{HiResScrollConfig, SmartShiftConfig},
};

use crate::snapshot::{collect_device_snapshot, DeviceSnapshot};

/// Device layout embedded from `assets/logitech-mx-master-3s.svg`.
const DEVICE_SVG: &str = include_str!("../assets/logitech-mx-master-3s.svg");
/// Width of the device layout coordinate space.
const LAYOUT_W: f64 = 128.32296;
/// Height of the device layout coordinate space.
const LAYOUT_H: f64 = 117.73977;
/// Vertical offset of the device group inside the layout SVG.
const DEVICE_GROUP_DY: f64 = -179.26022;
/// Width the SVG is rasterized to once and cached, in pixels.
const SVG_RENDER_WIDTH: u32 = 1024;
/// Width of the settings panel in pixels.
const PANEL_WIDTH: i32 = 380;
/// Margin between the HUD window and the screen edges in pixels.
const HUD_MARGIN: i32 = 80;
/// Padding added to thin hotspots so they remain easy to click.
const HIT_PADDING: f64 = 2.5;
/// Minimum hotspot width before padding is applied, in layout units.
const THIN_HOTSPOT: f64 = 6.0;
/// Minimum distance between a leader line end and its label block, in px.
const LABEL_EDGE_PX: f64 = 8.0;
/// Minimum vertical distance between neighbouring leader labels.
const LABEL_MIN_GAP: f64 = 6.0;
/// DPI slider bounds accepted by the device.
const DPI_MIN: f64 = 400.0;
const DPI_MAX: f64 = 8000.0;
const DPI_STEP: f64 = 100.0;
/// `SmartShift` threshold slider bounds.
const SMARTSHIFT_MIN: f64 = 1.0;
const SMARTSHIFT_MAX: f64 = 255.0;
const SMARTSHIFT_STEP: f64 = 1.0;
/// Scroll velocity that saturates the wheel chevrons, in notches/s.
const WHEEL_FULL_SCALE: f64 = 25.0;
/// Exponential smoothing factor applied to every new live sample.
const LIVE_EMA_ALPHA: f64 = 0.3;
/// Interval between idle decays of the live gauges.
const LIVE_DECAY_INTERVAL: Duration = Duration::from_millis(250);
/// Delay before the live subscription reconnects after a disconnect.
const LIVE_RETRY_DELAY: Duration = Duration::from_secs(1);
/// Read timeout of the live socket, bounding how long a stop request waits.
const LIVE_POLL_TIMEOUT: Duration = Duration::from_millis(500);

/// Reprogrammable buttons of the device diagram.
///
/// Regions are the exact button geometries of the reference layout SVG:
/// front of the mouse at the top, thumb platform on the left.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum MouseButton {
    Left,
    Right,
    Wheel,
    DpiShift,
    Back,
    Forward,
    Gesture,
}

impl MouseButton {
    const ALL: [Self; 7] = [
        Self::Left,
        Self::Right,
        Self::Wheel,
        Self::DpiShift,
        Self::Back,
        Self::Forward,
        Self::Gesture,
    ];

    /// HID++ reprogrammable control ID of this button.
    const fn cid(self) -> u16 {
        match self {
            Self::Left => control_id::LEFT_BUTTON,
            Self::Right => control_id::RIGHT_BUTTON,
            Self::Wheel => control_id::MIDDLE_BUTTON,
            Self::DpiShift => control_id::SMARTSHIFT_TOGGLE,
            Self::Back => control_id::BACK_BUTTON,
            Self::Forward => control_id::FORWARD_BUTTON,
            Self::Gesture => control_id::GESTURE_BUTTON,
        }
    }

    /// Hit-test order: small hotspots first so padding never lets a large
    /// region steal their clicks.
    const HIT_ORDER: [Self; 7] = [
        Self::Wheel,
        Self::DpiShift,
        Self::Back,
        Self::Forward,
        Self::Gesture,
        Self::Left,
        Self::Right,
    ];

    const fn label(self) -> &'static str {
        match self {
            Self::Left => "Left",
            Self::Right => "Right",
            Self::Wheel => "Wheel Click",
            Self::DpiShift => "DPI Shift",
            Self::Back => "Back",
            Self::Forward => "Forward",
            Self::Gesture => "Gesture",
        }
    }

    /// Button rectangle `(x, y, width, height)` in layout units, matching the
    /// reference SVG geometry.
    const fn region(self) -> (f64, f64, f64, f64) {
        match self {
            Self::Left => (25.5, 12.3, 22.1, 36.1),
            Self::Right => (47.6, 8.1, 20.5, 37.7),
            Self::Wheel => (44.3, 19.5, 7.2, 15.3),
            Self::DpiShift => (45.9, 44.1, 4.0, 7.2),
            Self::Back => (20.1, 47.5, 1.4, 9.0),
            Self::Forward => (20.1, 58.0, 1.4, 9.0),
            Self::Gesture => (8.42, 61.69, 1.08, 6.3),
        }
    }

    /// Hit rectangle: the drawn region padded when too thin to click.
    const fn hit_region(self) -> (f64, f64, f64, f64) {
        let (x, y, w, h) = self.region();
        let pad = if w < THIN_HOTSPOT { HIT_PADDING } else { 0.0 };
        (x - pad, y - pad, w + 2.0 * pad, h + 2.0 * pad)
    }

    /// Reports whether the leader line runs to the left edge of the layout.
    ///
    /// Thumb buttons sit on the left flank of the device, so their callouts
    /// go left; everything else is called out to the right.
    const fn leader_to_left(self) -> bool {
        matches!(self, Self::Back | Self::Forward | Self::Gesture)
    }

    /// Id of the anchor circle for this button inside the layout SVG.
    const fn anchor_id(self) -> &'static str {
        match self {
            Self::Left => "button0-anchor",
            Self::Right => "button1-anchor",
            Self::Wheel => "button2-anchor",
            Self::Forward => "button3-anchor",
            Self::Back => "button4-anchor",
            Self::Gesture => "button5-anchor",
            Self::DpiShift => "button6-anchor",
        }
    }

    /// Id of the drawn shape element for this button inside the layout SVG.
    const fn element_id(self) -> &'static str {
        match self {
            Self::Left => "button0",
            Self::Right => "button1",
            Self::Wheel => "button2",
            Self::Forward => "button3",
            Self::Back => "button4",
            Self::Gesture => "button5",
            Self::DpiShift => "button6",
        }
    }

    /// Live telemetry bit for this button, when the kernel reports it.
    const fn live_bit(self) -> Option<u32> {
        match self {
            Self::Left => Some(BTN_BIT_LEFT),
            Self::Right => Some(BTN_BIT_RIGHT),
            Self::Wheel => Some(BTN_BIT_MIDDLE),
            Self::Back => Some(BTN_BIT_BACK),
            Self::Forward => Some(BTN_BIT_FORWARD),
            Self::Gesture => Some(BTN_BIT_GESTURE),
            Self::DpiShift => None,
        }
    }

    /// Reports whether a layout-space point falls inside the hit rectangle.
    const fn contains(self, x: f64, y: f64) -> bool {
        let (rx, ry, rw, rh) = self.hit_region();
        x >= rx && x <= rx + rw && y >= ry && y <= ry + rh
    }
}

/// Smoothed live input picture shown on the diagram.
#[derive(Default, Debug)]
struct LiveVisual {
    buttons: u32,
    wheel_speed: f64,
    wheel_dir: i32,
    hwheel_dir: i32,
    extra_keys: Vec<u16>,
    connected: bool,
}

impl LiveVisual {
    /// Folds one telemetry frame into the smoothed gauges.
    fn apply(&mut self, frame: &LiveFrame) {
        self.buttons = frame.buttons;
        self.extra_keys.clone_from(&frame.extra_keys);
        self.connected = true;

        let dt = f64::from(frame.dt_ms.max(1)) / 1000.0;
        let wheel_units = (frame.wheel.abs() * 120).max(frame.wheel_hi.abs());
        let instant_wheel = f64::from(wheel_units) / dt / 120.0;
        self.wheel_speed = if self.wheel_speed == 0.0 {
            instant_wheel
        } else {
            blend(self.wheel_speed, instant_wheel)
        };
        if frame.wheel_hi != 0 {
            self.wheel_dir = frame.wheel_hi.signum();
        }
        if frame.hwheel_hi != 0 {
            self.hwheel_dir = frame.hwheel_hi.signum();
        }
    }

    /// Fades the velocity gauges toward zero while the device is idle.
    ///
    /// Returns whether any gauge value actually moved.
    fn decay(&mut self) -> bool {
        if self.wheel_speed == 0.0 {
            return false;
        }
        self.wheel_speed *= 0.6;
        if self.wheel_speed < 0.5 {
            self.wheel_speed = 0.0;
        }
        true
    }
}

/// Exponential smoothing of a new sample into a running value.
#[must_use]
fn blend(previous: f64, sample: f64) -> f64 {
    sample.mul_add(LIVE_EMA_ALPHA, previous * (1.0 - LIVE_EMA_ALPHA))
}

#[derive(Default)]
struct HudState {
    selected: Option<MouseButton>,
    assignments: HashMap<MouseButton, String>,
    live: LiveVisual,
}

thread_local! {
    /// The HUD window while it is open, so a second activation closes it
    /// instead of stacking another instance.
    static ACTIVE_WINDOW: RefCell<Option<ApplicationWindow>> = const { RefCell::new(None) };

    /// Main-thread callback that folds the newest telemetry frame into the
    /// open window; cleared when the HUD closes.
    static LIVE_HOOK: RefCell<Option<Rc<dyn Fn()>>> = const { RefCell::new(None) };

    /// Stop flag of the live subscription thread of the open window.
    static LIVE_STOP: RefCell<Option<Arc<AtomicBool>>> = const { RefCell::new(None) };
}

/// Opens the HUD, or closes it when it is already open.
///
/// This is the activation entry point: repeated launches (tray item or a
/// Hyprland keybind) toggle the HUD instead of stacking windows.
pub fn toggle_hud(app: &Application) {
    let open_new = ACTIVE_WINDOW.with(|cell| {
        cell.borrow().as_ref().is_none_or(|window| {
            window.close();
            false
        })
    });

    if open_new {
        build_hud(app);
    }
}

/// Builds and presents the full-screen HUD.
fn build_hud(app: &Application) {
    let window = create_window(app);
    let state = Rc::new(RefCell::new(HudState::default()));
    let image = Rc::new(OnceCell::new());
    let masks = Rc::new(build_button_masks());
    let drawing = create_drawing_area();
    let snapshot = collect_device_snapshot();
    let (panel, live_label) = create_panel(&state, &drawing, &window, snapshot.as_ref());

    attach_hit_testing(&state, &drawing, &live_label);
    attach_draw_callback(&state, &drawing, &image, &masks);
    attach_escape_to_close(&window);
    start_live(&state, &drawing, &live_label);

    let root = GtkBox::new(Orientation::Horizontal, 0);
    root.append(&drawing);
    root.append(&panel);

    window.set_content(Some(&root));

    ACTIVE_WINDOW.with(|cell| *cell.borrow_mut() = Some(window.clone()));
    window.connect_close_request(|_| {
        LIVE_STOP.with(|cell| {
            if let Some(stop) = cell.borrow_mut().take() {
                stop.store(true, Ordering::Relaxed);
            }
        });
        LIVE_HOOK.with(|hook| *hook.borrow_mut() = None);
        ACTIVE_WINDOW.with(|cell| *cell.borrow_mut() = None);
        glib::Propagation::Proceed
    });

    window.present();
}

fn create_window(app: &Application) -> ApplicationWindow {
    let window = ApplicationWindow::builder()
        .application(app)
        .title("Logitech MX Master 3S — HUD")
        .default_width(1100)
        .default_height(700)
        .build();

    window.init_layer_shell();
    window.add_css_class("transparent-overlay");
    window.remove_css_class("background");
    window.set_layer(Layer::Overlay);
    window.set_keyboard_mode(KeyboardMode::OnDemand);
    window.set_anchor(Edge::Top, true);
    window.set_anchor(Edge::Bottom, true);
    window.set_anchor(Edge::Left, true);
    window.set_anchor(Edge::Right, true);
    window.set_margin(Edge::Top, HUD_MARGIN);
    window.set_margin(Edge::Bottom, HUD_MARGIN);
    window.set_margin(Edge::Left, HUD_MARGIN);
    window.set_margin(Edge::Right, HUD_MARGIN);

    window
}

fn create_drawing_area() -> gtk4::DrawingArea {
    gtk4::DrawingArea::builder()
        .hexpand(true)
        .vexpand(true)
        .content_width(600)
        .content_height(600)
        .build()
}

fn attach_escape_to_close(window: &ApplicationWindow) {
    let keys = EventControllerKey::new();
    let win_key = window.clone();
    keys.connect_key_pressed(move |_controller, key, _code, _state| {
        if key == gtk4::gdk::Key::Escape {
            win_key.close();
            glib::Propagation::Stop
        } else {
            glib::Propagation::Proceed
        }
    });
    window.add_controller(keys);
}

/// Builds the settings panel: live status plus the Device, Pointer, Wheel
/// and Buttons pages.
fn create_panel(
    state: &Rc<RefCell<HudState>>,
    drawing: &gtk4::DrawingArea,
    window: &ApplicationWindow,
    snapshot: Option<&DeviceSnapshot>,
) -> (ToastOverlay, Label) {
    let toast = ToastOverlay::new();
    toast.set_size_request(PANEL_WIDTH, -1);
    toast.add_css_class("card");

    let root = GtkBox::new(Orientation::Vertical, 8);
    root.set_margin_top(16);
    root.set_margin_bottom(16);
    root.set_margin_start(16);
    root.set_margin_end(16);

    let title = Label::new(Some("MX Master 3S"));
    title.add_css_class("title-2");
    root.append(&title);

    let live_label = Label::new(Some("Live telemetry: connecting..."));
    live_label.add_css_class("dim-label");
    live_label.set_wrap(true);
    live_label.set_xalign(0.0);
    root.append(&live_label);

    let stack = ViewStack::new();
    let pages = [
        (
            buttons_page(state, drawing, &toast),
            "buttons",
            "Buttons",
            "input-mouse-symbolic",
        ),
        (
            wheel_page(snapshot, &toast),
            "wheel",
            "Wheel",
            "view-continuous-symbolic",
        ),
        (
            pointer_page(snapshot, &toast),
            "pointer",
            "Pointer",
            "preferences-desktop-input-symbolic",
        ),
        (
            device_page(snapshot, &toast),
            "device",
            "Device",
            "battery-symbolic",
        ),
    ];
    for (page, name, title, icon) in pages {
        stack.add_titled(&page, Some(name), title);
        stack.page(&page).set_icon_name(Some(icon));
    }

    let switcher = ViewSwitcher::builder()
        .stack(&stack)
        .policy(ViewSwitcherPolicy::Narrow)
        .build();
    root.append(&switcher);

    let stack_box = GtkBox::new(Orientation::Vertical, 0);
    stack_box.set_vexpand(true);
    stack_box.append(&stack);
    root.append(&stack_box);

    let close_btn = Button::with_label("Close");
    close_btn.add_css_class("flat");
    let win_close = window.clone();
    close_btn.connect_clicked(move |_| win_close.close());
    root.append(&close_btn);

    toast.set_child(Some(&root));
    (toast, live_label)
}

/// Native remap targets offered on the assignments page: label and CID.
/// `0x0000` restores the device-native behaviour of the selected control.
const REMAP_TARGETS: [(&str, u16); 8] = [
    ("Act as Left", control_id::LEFT_BUTTON),
    ("Act as Right", control_id::RIGHT_BUTTON),
    ("Act as Middle", control_id::MIDDLE_BUTTON),
    ("Act as Back", control_id::BACK_BUTTON),
    ("Act as Forward", control_id::FORWARD_BUTTON),
    ("Act as Gesture", control_id::GESTURE_BUTTON),
    ("Act as DPI Shift", control_id::SMARTSHIFT_TOGGLE),
    ("Restore native", 0x0000),
];

/// Creates the button assignment page.
fn buttons_page(
    state: &Rc<RefCell<HudState>>,
    drawing: &gtk4::DrawingArea,
    toast: &ToastOverlay,
) -> ScrolledWindow {
    let scroll = page_scroll();
    let group = PreferencesGroup::new();
    group.set_title("Assignments");
    group.set_description(Some(
        "Select a button on the diagram, then pick what it should act as",
    ));

    for (label, target_cid) in REMAP_TARGETS {
        let button = Button::with_label(label);
        button.add_css_class("pill");
        let state_clone = Rc::clone(state);
        let drawing_clone = drawing.clone();
        let toast_clone = toast.clone();
        button.connect_clicked(move |_| {
            let Some(selected) = state_clone.borrow().selected else {
                toast_clone.add_toast(Toast::new("Select a button on the diagram first"));
                return;
            };
            let control_id = selected.cid();
            let response = ipc::try_request(&ipc::Request::SetButtonRemap {
                control_id,
                target_cid,
            });
            let remapped = matches!(response, Some(Response::Ok));
            report(
                response,
                &toast_clone,
                &format!("{} now acts as {label}", selected.label()),
            );
            if remapped {
                let mut st = state_clone.borrow_mut();
                if target_cid == 0 {
                    st.assignments.remove(&selected);
                } else {
                    st.assignments.insert(selected, label.to_string());
                }
                drawing_clone.queue_draw();
            }
        });
        group.add(&button);
    }

    scroll.set_child(Some(&group));
    scroll
}

/// Creates the wheel page: `SmartShift`, hi-res and direction settings.
fn wheel_page(snapshot: Option<&DeviceSnapshot>, toast: &ToastOverlay) -> ScrolledWindow {
    let scroll = page_scroll();
    let box_ = GtkBox::new(Orientation::Vertical, 0);
    let smartshift = snapshot.map_or_else(SmartShiftConfig::default, |s| s.smartshift);
    let hires = snapshot.map_or_else(HiResScrollConfig::default, |s| s.hiresscroll);

    let shift_group = PreferencesGroup::new();
    shift_group.set_title("SmartShift");
    shift_group.set_description(Some("Automatic switching between ratchet and freespin"));

    let switch_row = ActionRow::new();
    switch_row.set_title("Enable SmartShift");
    let shift_switch = Switch::new();
    shift_switch.set_valign(gtk4::Align::Center);
    shift_switch.set_active(smartshift.enabled);
    switch_row.add_suffix(&shift_switch);
    switch_row.set_activatable_widget(Some(&shift_switch));

    let threshold_row = ActionRow::new();
    threshold_row.set_title("Disengage Sensitivity");
    threshold_row.set_subtitle(&format!("Current: {}", smartshift.threshold));
    let threshold_scale = Scale::with_range(
        Orientation::Horizontal,
        SMARTSHIFT_MIN,
        SMARTSHIFT_MAX,
        SMARTSHIFT_STEP,
    );
    threshold_scale.set_value(f64::from(smartshift.threshold));
    threshold_scale.set_draw_value(true);
    threshold_scale.set_hexpand(true);
    let tr = threshold_row.clone();
    threshold_scale.connect_value_changed(move |s| {
        tr.set_subtitle(&format!("Current: {}", scale_threshold(s.value())));
    });

    let shift_apply = Button::with_label("Apply");
    shift_apply.add_css_class("suggested-action");
    shift_apply.add_css_class("pill");
    let sw = shift_switch;
    let ts = threshold_scale.clone();
    let to = toast.clone();
    shift_apply.connect_clicked(move |_| {
        let request = ipc::Request::SetSmartShift {
            enabled: sw.is_active(),
            threshold: scale_threshold(ts.value()),
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
    hires_switch.set_active(hires.enabled);
    hires_row.add_suffix(&hires_switch);
    hires_row.set_activatable_widget(Some(&hires_switch));

    let inverted_row = ActionRow::new();
    inverted_row.set_title("Natural Scrolling");
    let inverted_switch = Switch::new();
    inverted_switch.set_valign(gtk4::Align::Center);
    inverted_switch.set_active(hires.inverted);
    inverted_row.add_suffix(&inverted_switch);
    inverted_row.set_activatable_widget(Some(&inverted_switch));

    let scroll_apply = Button::with_label("Apply");
    scroll_apply.add_css_class("suggested-action");
    scroll_apply.add_css_class("pill");
    let hs = hires_switch;
    let inv = inverted_switch;
    let to = toast.clone();
    scroll_apply.connect_clicked(move |_| {
        let request = ipc::Request::SetHires {
            enabled: hs.is_active(),
            inverted: inv.is_active(),
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
    scroll
}

/// Creates the pointer page with the DPI slider.
fn pointer_page(snapshot: Option<&DeviceSnapshot>, toast: &ToastOverlay) -> ScrolledWindow {
    let scroll = page_scroll();
    let group = PreferencesGroup::new();
    group.set_title("Pointer Sensitivity");
    group.set_description(Some("Cursor speed from 400 to 8000 DPI"));

    let current_dpi = snapshot.map_or(1000, |s| s.dpi);
    let dpi_row = ActionRow::new();
    dpi_row.set_title("Current DPI");
    dpi_row.set_subtitle(&format!("{current_dpi} DPI"));

    let scale_row = ActionRow::new();
    scale_row.set_title("Sensitivity");
    let scale = Scale::with_range(Orientation::Horizontal, DPI_MIN, DPI_MAX, DPI_STEP);
    scale.set_value(f64::from(current_dpi));
    scale.set_draw_value(true);
    scale.set_hexpand(true);
    let dr = dpi_row.clone();
    scale.connect_value_changed(move |s| {
        dr.set_subtitle(&format!("{} DPI", scale_dpi(s.value())));
    });

    let apply = Button::with_label("Apply");
    apply.add_css_class("suggested-action");
    apply.add_css_class("pill");
    let sc = scale.clone();
    let to = toast.clone();
    apply.connect_clicked(move |_| {
        let dpi = scale_dpi(sc.value());
        report(
            ipc::try_request(&ipc::Request::SetDpi { value: dpi }),
            &to,
            &format!("DPI set to {dpi}"),
        );
    });

    group.add(&dpi_row);
    let scale_box = GtkBox::new(Orientation::Horizontal, 12);
    scale_box.append(&scale);
    scale_box.append(&apply);
    scale_row.set_child(Some(&scale_box));
    group.add(&scale_row);

    scroll.set_child(Some(&group));
    scroll
}

/// Creates the device page: battery, desktop integration and version.
fn device_page(snapshot: Option<&DeviceSnapshot>, toast: &ToastOverlay) -> ScrolledWindow {
    let scroll = page_scroll();
    let box_ = GtkBox::new(Orientation::Vertical, 0);

    let info_group = PreferencesGroup::new();
    info_group.set_title("Device");
    let name_row = ActionRow::new();
    name_row.set_title("Model");
    name_row.set_subtitle(snapshot.map_or("daemon unavailable", |s| s.name.as_str()));
    let battery_row = ActionRow::new();
    battery_row.set_title("Battery");
    battery_row.set_subtitle(&snapshot.map_or_else(
        || "unknown".to_string(),
        |s| {
            s.battery.map_or_else(
                || "unknown".to_string(),
                |b| format!("{}% · {:?}", b.level, b.status),
            )
        },
    ));
    let refresh = Button::with_label("Refresh");
    refresh.add_css_class("pill");
    let br = battery_row.clone();
    let to = toast.clone();
    refresh.connect_clicked(move |_| match ipc::try_request(&ipc::Request::Battery) {
        Some(Response::Battery { level, status }) => {
            br.set_subtitle(&format!("{level}% · {status}"));
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
            Ok(crate::hyprland::Outcome::Configured { path }) => {
                format!("Hyprland configured: {path}")
            }
            Ok(crate::hyprland::Outcome::AlreadyConfigured { path }) => {
                format!("Already configured: {path}")
            }
            Ok(crate::hyprland::Outcome::NotHyprland) => "Not running under Hyprland".to_string(),
            Ok(crate::hyprland::Outcome::NoConfig) => {
                "Hyprland config not found in ~/.config/hypr".to_string()
            }
            Err(e) => format!("Hyprland setup failed: {e}"),
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
    box_.append(&integration_group);
    box_.append(&version_group);
    scroll.set_child(Some(&box_));
    scroll
}

/// A page-sized vertical scroller for panel content.
fn page_scroll() -> ScrolledWindow {
    let scroll = ScrolledWindow::new();
    scroll.set_vexpand(true);
    scroll.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
    scroll
}

/// Turns a daemon reply into a toast message.
fn report(response: Option<Response>, toast: &ToastOverlay, success: &str) {
    let message = match response {
        Some(Response::Ok) => success.to_string(),
        Some(Response::Error { message }) => message,
        _ => "Daemon unavailable".to_string(),
    };
    toast.add_toast(Toast::new(&message));
}

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

/// Subscribes to daemon telemetry and folds frames into the HUD state.
///
/// A background thread owns the socket and survives daemon restarts; each
/// frame wakes the main context through an invoke, where the window hook
/// applies it. Closing the HUD clears the hook and stops the thread.
fn start_live(state: &Rc<RefCell<HudState>>, drawing: &gtk4::DrawingArea, label: &Label) {
    let stop = Arc::new(AtomicBool::new(false));
    let slot = Arc::new(Mutex::new(None::<LiveFrame>));

    let connected = Arc::new(AtomicBool::new(false));
    let slot_hook = Arc::clone(&slot);
    let connected_hook = Arc::clone(&connected);
    let state_hook = Rc::clone(state);
    let drawing_hook = drawing.clone();
    let label_hook = label.clone();
    let hook: Rc<dyn Fn()> = Rc::new(move || {
        let frame = slot_hook.lock().ok().and_then(|mut guard| guard.take());
        {
            let mut st = state_hook.borrow_mut();
            st.live.connected = connected_hook.load(Ordering::Relaxed);
            if let Some(frame) = frame {
                st.live.apply(&frame);
            }
        }
        let summary = live_summary(&state_hook.borrow().live);
        label_hook.set_text(&summary);
        drawing_hook.queue_draw();
    });
    LIVE_HOOK.with(|cell| *cell.borrow_mut() = Some(Rc::clone(&hook)));
    LIVE_STOP.with(|cell| *cell.borrow_mut() = Some(Arc::clone(&stop)));

    let context = glib::MainContext::default();
    let stop_thread = Arc::clone(&stop);
    let slot_thread = Arc::clone(&slot);
    let connected_thread = Arc::clone(&connected);
    let wake = move || {
        let context = context.clone();
        context.invoke(move || {
            LIVE_HOOK.with(|cell| {
                if let Some(hook) = cell.borrow().as_ref() {
                    hook();
                }
            });
        });
    };
    if let Err(e) = thread::Builder::new()
        .name("hud-live".to_string())
        .spawn(move || {
            while !stop_thread.load(Ordering::Relaxed) {
                let Ok(socket) = ipc::open_live_stream() else {
                    thread::sleep(LIVE_RETRY_DELAY);
                    continue;
                };
                connected_thread.store(true, Ordering::Relaxed);
                wake();
                pump_live_socket(socket, &slot_thread, &connected_thread, &stop_thread, &wake);
            }
        })
    {
        tracing::warn!("Failed to spawn live subscription thread: {e}");
    }

    let state_decay = Rc::clone(state);
    let drawing_decay = drawing.clone();
    glib::timeout_add_local(LIVE_DECAY_INTERVAL, move || {
        if ACTIVE_WINDOW.with(|window| window.borrow().is_none()) {
            return glib::ControlFlow::Break;
        }
        let changed = state_decay.borrow_mut().live.decay();
        if changed {
            drawing_decay.queue_draw();
        }
        glib::ControlFlow::Continue
    });
}

/// Reads frames from one live socket into the shared slot until it drops.
fn pump_live_socket(
    socket: std::os::unix::net::UnixStream,
    slot: &Arc<Mutex<Option<LiveFrame>>>,
    connected: &Arc<AtomicBool>,
    stop: &AtomicBool,
    wake: &dyn Fn(),
) {
    let _ = socket.set_read_timeout(Some(LIVE_POLL_TIMEOUT));
    let mut reader = BufReader::new(socket);
    loop {
        let mut line = String::new();
        match std::io::BufRead::read_line(&mut reader, &mut line) {
            Ok(0) => break,
            Ok(_) => {
                let parsed = serde_json::from_str::<Response>(&line);
                let Ok(Response::Live(frame)) = parsed else {
                    continue;
                };
                if let Ok(mut guard) = slot.lock() {
                    *guard = Some(frame);
                }
                wake();
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if stop.load(Ordering::Relaxed) {
                    break;
                }
            }
            Err(_) => break,
        }
    }
    connected.store(false, Ordering::Relaxed);
    wake();
}

/// Formats the live status line shown in the panel.
fn live_summary(live: &LiveVisual) -> String {
    if !live.connected {
        return "Live telemetry: waiting for the daemon".to_string();
    }
    if live.buttons == 0 && live.wheel_speed < 0.5 && live.extra_keys.is_empty() {
        return "Live: connected · waiting for input".to_string();
    }
    let mut text = format!("Live: wheel {:.0}/s", live.wheel_speed);
    if !live.extra_keys.is_empty() {
        let codes: Vec<String> = live.extra_keys.iter().map(ToString::to_string).collect();
        text.push_str(" · raw keys ");
        text.push_str(&codes.join(","));
    }
    text
}

fn attach_hit_testing(
    state: &Rc<RefCell<HudState>>,
    drawing: &gtk4::DrawingArea,
    status_label: &Label,
) {
    let gesture = GestureClick::new();
    let state_gesture = Rc::clone(state);
    let drawing_gesture = drawing.clone();
    let status_gesture = status_label.clone();
    gesture.connect_released(move |_gesture, _n_press, px, py| {
        let (x, y) = to_layout(px, py, drawing_gesture.width(), drawing_gesture.height());
        let mut st = state_gesture.borrow_mut();
        for button in MouseButton::HIT_ORDER {
            if button.contains(x, y) {
                st.selected = Some(button);
                status_gesture.set_text(&format!("Selected: {}", button.label()));
                drawing_gesture.queue_draw();
                return;
            }
        }
    });
    drawing.add_controller(gesture);
}

fn attach_draw_callback(
    state: &Rc<RefCell<HudState>>,
    drawing: &gtk4::DrawingArea,
    image: &Rc<OnceCell<Option<gtk4::cairo::ImageSurface>>>,
    masks: &Rc<HashMap<MouseButton, gtk4::cairo::ImageSurface>>,
) {
    let state_draw = Rc::clone(state);
    let image_draw = Rc::clone(image);
    let masks_draw = Rc::clone(masks);
    drawing.set_draw_func(move |_, cr, width, height| {
        let st = state_draw.borrow();
        let (scale, offset_x, offset_y) = fit_transform(f64::from(width), f64::from(height));

        let _ = cr.save();
        cr.translate(offset_x, offset_y);
        cr.scale(scale, scale);

        let surface_scale = image_draw
            .get_or_init(render_device_svg)
            .as_ref()
            .map(|surface| {
                let sx = LAYOUT_W / f64::from(surface.width());
                let sy = LAYOUT_H / f64::from(surface.height());
                let _ = cr.save();
                cr.scale(sx, sy);
                let _ = cr.set_source_surface(surface, 0.0, 0.0);
                let _ = cr.paint();
                let _ = cr.restore();
                (sx, sy)
            });

        draw_leaders(&st, cr, width, height);
        draw_selection(&st, cr);
        if let Some((sx, sy)) = surface_scale {
            draw_pressed_shapes(&st, cr, sx, sy, &masks_draw);
        }
        let _ = cr.restore();

        draw_wheel_indicator(&st, cr, width);
    });
}

/// Rasterizes an SVG document once into a cairo surface sized to the layout.
fn render_svg_to_surface(svg: &[u8]) -> Option<gtk4::cairo::ImageSurface> {
    let tree = usvg::Tree::from_data(svg, &usvg::Options::default()).ok()?;
    let size = tree.size();
    #[allow(
        clippy::cast_precision_loss,
        reason = "the raster width constant is exactly representable as f32"
    )]
    let scale = SVG_RENDER_WIDTH as f32 / size.width();
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "the raster height is clamped to a sane pixel range"
    )]
    let height = (size.height() * scale).round().clamp(1.0, 8192.0) as u32;
    let width = SVG_RENDER_WIDTH;

    let mut pixmap = resvg::tiny_skia::Pixmap::new(width, height)?;
    let transform = resvg::tiny_skia::Transform::from_scale(scale, scale);
    resvg::render(&tree, transform, &mut pixmap.as_mut());

    let mut data = pixmap.take();
    for pixel in data.as_chunks_mut::<4>().0 {
        pixel.swap(0, 2);
    }

    gtk4::cairo::ImageSurface::create_for_data(
        data,
        gtk4::cairo::Format::ARgb32,
        width.cast_signed(),
        height.cast_signed(),
        (width * 4).cast_signed(),
    )
    .ok()
}

/// Rasterizes the embedded device SVG once into a cairo surface.
fn render_device_svg() -> Option<gtk4::cairo::ImageSurface> {
    render_svg_to_surface(DEVICE_SVG.as_bytes())
}

/// Builds a standalone SVG containing only the device element with the
/// given id, so it can be rendered as an alpha mask aligned with the base
/// image.
fn device_element_svg(id: &str) -> Option<String> {
    let marker = format!("id=\"{id}\"");
    let at = DEVICE_SVG.find(&marker)?;
    let open = DEVICE_SVG[..at].rfind('<')?;
    let close = DEVICE_SVG[at..].find("/>")? + at + 2;
    let element = &DEVICE_SVG[open..close];
    Some(format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {LAYOUT_W} {LAYOUT_H}\">\
         <g transform=\"translate(0,{DEVICE_GROUP_DY})\">{element}</g></svg>"
    ))
}

/// Alpha masks of every button shape, rendered once from the layout SVG.
fn build_button_masks() -> HashMap<MouseButton, gtk4::cairo::ImageSurface> {
    MouseButton::ALL
        .iter()
        .filter_map(|button| {
            let svg = device_element_svg(button.element_id())?;
            let surface = render_svg_to_surface(svg.as_bytes())?;
            Some((*button, surface))
        })
        .collect()
}

/// Computes uniform scale and centering offsets for the layout space.
fn fit_transform(width: f64, height: f64) -> (f64, f64, f64) {
    let scale = (width / LAYOUT_W).min(height / LAYOUT_H);
    let content_w = LAYOUT_W * scale;
    let content_h = LAYOUT_H * scale;
    (scale, (width - content_w) / 2.0, (height - content_h) / 2.0)
}

/// Maps widget pixel coordinates into the device layout space.
fn to_layout(px: f64, py: f64, width: i32, height: i32) -> (f64, f64) {
    let (scale, offset_x, offset_y) = fit_transform(f64::from(width), f64::from(height));
    ((px - offset_x) / scale, (py - offset_y) / scale)
}

fn draw_selection(st: &HudState, cr: &gtk4::cairo::Context) {
    for button in MouseButton::ALL {
        let selected = st.selected == Some(button);
        let assigned = st.assignments.contains_key(&button);
        if !selected && !assigned {
            continue;
        }

        let (x, y, w, h) = button.region();
        if selected {
            cr.set_source_rgba(0.93, 0.66, 0.46, 0.55);
        } else {
            cr.set_source_rgba(0.42, 0.60, 0.80, 0.50);
        }
        rounded_rect(cr, x, y, w, h, 0.8);
        let _ = cr.fill_preserve();

        if selected {
            cr.set_source_rgb(0.93, 0.66, 0.46);
            cr.set_line_width(0.5);
            let _ = cr.stroke();
        }
    }
}

/// Tints the exact SVG shape of every physically pressed button.
fn draw_pressed_shapes(
    st: &HudState,
    cr: &gtk4::cairo::Context,
    sx: f64,
    sy: f64,
    masks: &HashMap<MouseButton, gtk4::cairo::ImageSurface>,
) {
    for button in MouseButton::ALL {
        let Some(bit) = button.live_bit() else {
            continue;
        };
        if st.live.buttons & bit == 0 {
            continue;
        }
        let Some(mask) = masks.get(&button) else {
            continue;
        };
        let pattern = gtk4::cairo::SurfacePattern::create(mask);

        let _ = cr.save();
        cr.scale(sx, sy);
        cr.set_source_rgba(0.93, 0.66, 0.46, 0.9);
        let _ = cr.mask(&pattern);
        let _ = cr.restore();
    }
}

/// Draws chevrons near the wheel showing scroll direction and velocity.
#[allow(
    clippy::suboptimal_flops,
    reason = "plain arithmetic keeps gauge geometry readable"
)]
fn draw_wheel_indicator(st: &HudState, cr: &gtk4::cairo::Context, _width: i32) {
    let intensity = (st.live.wheel_speed / WHEEL_FULL_SCALE).min(1.0);
    if intensity <= 0.01 {
        return;
    }

    let Some(&(ax, ay)) = button_anchors().get(&MouseButton::Wheel) else {
        return;
    };
    let dir = st.live.wheel_dir;
    let vertical = dir != 0;
    let (ux, uy) = if vertical {
        (0.0, f64::from(dir))
    } else {
        (f64::from(st.live.hwheel_dir), 0.0)
    };

    cr.set_source_rgba(0.93, 0.66, 0.46, 0.4 + 0.6 * intensity);
    cr.set_line_width(0.6);
    for step in 1..=3 {
        let dist = 3.0 + f64::from(step) * 2.2;
        let cx = ax + ux * dist + if vertical { 6.5 } else { 0.0 };
        let cy = ay + uy * dist + if vertical { 0.0 } else { -5.0 };
        let half = 1.6;
        let (px, py) = if vertical {
            (half, half * 0.6 * uy)
        } else {
            (half * 0.6 * ux, half)
        };
        cr.move_to(
            cx - (if vertical { half } else { 0.0 }),
            cy - (if vertical { 0.0 } else { half }),
        );
        cr.line_to(cx + px * 0.4, cy + py * 0.4);
        cr.line_to(
            cx + (if vertical { half } else { 0.0 }),
            cy + (if vertical { 0.0 } else { half }),
        );
        let _ = cr.stroke();
    }
}

/// Draws one callout per button from its SVG anchor point to the layout
/// edge, labelled with what the button does and its current assignment.
///
/// Labels on each side are sorted by anchor height and pushed apart so they
/// never overlap; the leader bends at a knee near the edge to reach its
/// label row.
fn draw_leaders(st: &HudState, cr: &gtk4::cairo::Context, width: i32, height: i32) {
    let anchors = button_anchors();
    let (scale, offset_x, _) = fit_transform(f64::from(width), f64::from(height));

    for to_left in [true, false] {
        let mut side: Vec<(MouseButton, f64, f64)> = MouseButton::ALL
            .iter()
            .filter(|button| button.leader_to_left() == to_left)
            .filter_map(|button| anchors.get(button).map(|&(ax, ay)| (*button, ax, ay)))
            .collect();
        side.sort_by(|(_, _, a), (_, _, b)| a.total_cmp(b));
        let rows = resolve_rows(&side, LABEL_MIN_GAP);

        let knee_x = if to_left { -0.5 } else { LAYOUT_W + 0.5 };
        let label_x = if to_left {
            (LABEL_EDGE_PX - offset_x) / scale
        } else {
            (f64::from(width) - LABEL_EDGE_PX - offset_x) / scale
        };

        for (button, ax, ay, label_y) in rows {
            let selected = st.selected == Some(button);
            if selected {
                cr.set_source_rgba(0.93, 0.66, 0.46, 0.95);
                cr.set_line_width(0.4);
            } else {
                cr.set_source_rgba(0.62, 0.65, 0.70, 0.55);
                cr.set_line_width(0.25);
            }

            let title = button.label().to_uppercase();
            let assignment = st.assignments.get(&button);
            let block_w = measure_text(cr, &title, 2.7)
                .max(assignment.map_or(0.0, |action| measure_text(cr, action, 2.0)));
            let line_end = if to_left {
                label_x + block_w + 1.5
            } else {
                label_x - block_w - 1.5
            };

            cr.move_to(ax, ay);
            cr.line_to(knee_x, ay);
            cr.line_to(knee_x, label_y);
            cr.line_to(line_end, label_y);
            let _ = cr.stroke();

            cr.new_path();
            cr.arc(ax, ay, 0.75, 0.0, std::f64::consts::TAU);
            let _ = cr.fill();

            draw_label(cr, &title, 2.7, label_x, label_y - 0.5, to_left);
            if let Some(action) = assignment {
                draw_label(cr, action, 2.0, label_x, label_y + 2.6, to_left);
            }
        }
    }
}

/// Measures text width at the given size in the current transform units.
fn measure_text(cr: &gtk4::cairo::Context, text: &str, size: f64) -> f64 {
    select_label_font(cr, size);
    cr.text_extents(text).map_or(0.0, |extents| extents.width())
}

/// Assigns label rows on one side so neighbouring callouts never overlap.
///
/// Keeps each label at its anchor height when there is room and pushes it
/// down otherwise; input must be sorted by anchor height.
fn resolve_rows(
    side: &[(MouseButton, f64, f64)],
    min_gap: f64,
) -> Vec<(MouseButton, f64, f64, f64)> {
    let mut rows = Vec::with_capacity(side.len());
    let mut previous_y = f64::MIN;
    for &(button, ax, ay) in side {
        let label_y = ay.max(previous_y + min_gap);
        previous_y = label_y;
        rows.push((button, ax, ay, label_y));
    }
    rows
}

/// Renders one leader label: left-aligned at `anchor_x` for the left side,
/// right-aligned at `anchor_x` for the right side.
fn draw_label(
    cr: &gtk4::cairo::Context,
    text: &str,
    size: f64,
    anchor_x: f64,
    baseline_y: f64,
    to_left: bool,
) {
    select_label_font(cr, size);
    cr.set_source_rgba(0.88, 0.90, 0.94, 0.92);

    if let Ok(extents) = cr.text_extents(text) {
        let x = if to_left {
            anchor_x - extents.x_bearing()
        } else {
            anchor_x - extents.x_bearing() - extents.width()
        };
        cr.move_to(x, baseline_y - extents.y_bearing());
        let _ = cr.show_text(text);
    }
}

fn select_label_font(cr: &gtk4::cairo::Context, size: f64) {
    cr.select_font_face(
        "Sans",
        gtk4::cairo::FontSlant::Normal,
        gtk4::cairo::FontWeight::Normal,
    );
    cr.set_font_size(size);
}

/// Anchor points of every button, parsed once from the layout SVG.
fn button_anchors() -> &'static HashMap<MouseButton, (f64, f64)> {
    static ANCHORS: OnceLock<HashMap<MouseButton, (f64, f64)>> = OnceLock::new();
    ANCHORS.get_or_init(|| {
        MouseButton::ALL
            .iter()
            .filter_map(|button| {
                parse_anchor(DEVICE_SVG, button.anchor_id()).map(|point| (*button, point))
            })
            .collect()
    })
}

/// Extracts `(cx, cy)` of the anchor circle with the given id.
fn parse_anchor(svg: &str, anchor_id: &str) -> Option<(f64, f64)> {
    let tail = &svg[svg.find(anchor_id)?..];
    Some((
        parse_attr_number(tail, "cx")?,
        parse_attr_number(tail, "cy")?,
    ))
}

fn parse_attr_number(text: &str, attribute: &str) -> Option<f64> {
    let tail = &text[text.find(attribute)? + attribute.len()..];
    let tail = &tail[tail.find('"')? + 1..];
    tail[..tail.find('"')?].parse().ok()
}

fn rounded_rect(
    cr: &gtk4::cairo::Context,
    left: f64,
    top: f64,
    width: f64,
    height: f64,
    radius: f64,
) {
    cr.new_sub_path();
    cr.arc(
        left + width - radius,
        top + radius,
        radius,
        -std::f64::consts::FRAC_PI_2,
        0.0,
    );
    cr.arc(
        left + width - radius,
        top + height - radius,
        radius,
        0.0,
        std::f64::consts::FRAC_PI_2,
    );
    cr.arc(
        left + radius,
        top + height - radius,
        radius,
        std::f64::consts::PI,
        3.0 * std::f64::consts::FRAC_PI_2,
    );
    cr.arc(
        left + radius,
        top + radius,
        radius,
        std::f64::consts::PI,
        3.0 * std::f64::consts::FRAC_PI_2,
    );
    cr.close_path();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thumb_buttons_are_called_out_to_the_left() {
        assert!(MouseButton::Back.leader_to_left());
        assert!(MouseButton::Forward.leader_to_left());
        assert!(MouseButton::Gesture.leader_to_left());
    }

    #[test]
    fn top_buttons_are_called_out_to_the_right() {
        assert!(!MouseButton::Left.leader_to_left());
        assert!(!MouseButton::Right.leader_to_left());
        assert!(!MouseButton::Wheel.leader_to_left());
        assert!(!MouseButton::DpiShift.leader_to_left());
    }

    #[test]
    fn every_button_has_an_svg_anchor() {
        assert_eq!(button_anchors().len(), MouseButton::ALL.len());
    }

    #[test]
    fn gesture_anchor_sits_on_the_rotated_tab() {
        let (x, y) = *button_anchors()
            .get(&MouseButton::Gesture)
            .expect("gesture anchor present");
        assert!(x < 12.0 && (60.0..70.0).contains(&y));
    }

    #[test]
    fn resolve_rows_pushes_overlapping_labels_down() {
        let side = vec![
            (MouseButton::Back, 20.8, 52.0),
            (MouseButton::Forward, 20.8, 62.5),
            (MouseButton::Gesture, 9.0, 64.8),
        ];
        let rows = resolve_rows(&side, LABEL_MIN_GAP);

        assert!((rows[0].3 - 52.0).abs() < f64::EPSILON);
        assert!((rows[1].3 - 62.5).abs() < f64::EPSILON);
        assert!(rows[2].3 >= rows[1].3 + LABEL_MIN_GAP);
    }

    #[test]
    fn resolve_rows_keeps_spaced_labels_at_anchor_height() {
        let side = vec![
            (MouseButton::Right, 61.1, 17.1),
            (MouseButton::Wheel, 47.7, 27.7),
            (MouseButton::Left, 34.4, 38.2),
        ];
        let rows = resolve_rows(&side, LABEL_MIN_GAP);

        assert!((rows[0].3 - 17.1).abs() < f64::EPSILON);
        assert!((rows[1].3 - 27.7).abs() < f64::EPSILON);
        assert!((rows[2].3 - 38.2).abs() < f64::EPSILON);
    }

    #[test]
    fn parse_anchor_reads_circle_coordinates() {
        let svg = r#"<circle id="button0-anchor" cx="34.382" cy="38.154" r="0.8"/>"#;
        assert_eq!(parse_anchor(svg, "button0-anchor"), Some((34.382, 38.154)));
        assert_eq!(parse_anchor(svg, "missing-anchor"), None);
    }

    #[test]
    fn element_svg_extracts_self_closing_shapes() {
        let svg = device_element_svg("button4").expect("button4 present");
        assert!(svg.contains("id=\"button4\""));
        assert!(svg.starts_with("<svg"));
        assert!(svg.contains("translate(0,-179.26022)"));
    }

    #[test]
    fn element_svg_rejects_unknown_ids() {
        assert!(device_element_svg("button99").is_none());
    }

    #[test]
    fn motion_frame_marks_connection() {
        let mut live = LiveVisual::default();
        assert!(!live.connected);
        let frame = LiveFrame {
            dx: 30,
            dy: 40,
            dt_ms: 10,
            ..LiveFrame::default()
        };
        live.apply(&frame);

        assert!(live.connected);
        assert_eq!(live.wheel_speed, 0.0);
    }

    #[test]
    fn live_wheel_direction_and_decay() {
        let mut live = LiveVisual::default();
        let frame = LiveFrame {
            wheel_hi: -240,
            dt_ms: 20,
            ..LiveFrame::default()
        };
        live.apply(&frame);
        assert_eq!(live.wheel_dir, -1);
        assert!(live.wheel_speed > 0.0);

        for _ in 0..30 {
            live.decay();
        }
        assert_eq!(live.wheel_speed, 0.0);
    }

    #[test]
    fn live_summary_mentions_raw_keys() {
        let live = LiveVisual {
            connected: true,
            extra_keys: vec![279],
            ..LiveVisual::default()
        };
        assert!(live_summary(&live).contains("raw keys 279"));
    }
}
