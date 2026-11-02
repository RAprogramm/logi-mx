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
    cell::{Cell, OnceCell, RefCell},
    collections::HashMap,
    fs,
    io::BufReader,
    path::PathBuf,
    rc::Rc,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, AtomicU32, Ordering}
    },
    thread,
    time::Duration
};

use gtk4::{
    Box as GtkBox, Button, CheckButton, DropDown, EventControllerKey, EventControllerMotion,
    FlowBox, FlowBoxChild, GestureClick, Label, Orientation, Scale, ScrolledWindow, SelectionMode,
    Switch, glib
};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use libadwaita::{
    ActionRow, Application, ApplicationWindow, PreferencesGroup, Toast, ToastOverlay, ViewStack,
    ViewSwitcher, ViewSwitcherPolicy, prelude::*
};
use logi_mx_daemon::ipc::{
    self, BTN_BIT_BACK, BTN_BIT_FORWARD, BTN_BIT_GESTURE, BTN_BIT_LEFT, BTN_BIT_MIDDLE,
    BTN_BIT_RIGHT, LiveFrame, Response
};
use logi_mx_driver::devices::control_id;

use crate::snapshot::{DeviceSnapshot, collect_device_snapshot};

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
/// DPI slider bounds accepted by the device.
const DPI_MIN: f64 = 400.0;
const DPI_MAX: f64 = 8000.0;
const DPI_STEP: f64 = 100.0;
/// Neutral DPI shown before the snapshot arrives.
const DEFAULT_DPI: u16 = 1000;
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

/// Backdrop treatment behind the HUD content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum BgMode {
    /// Fully transparent backdrop.
    None,
    /// Semi-transparent dark fill.
    #[default]
    Dim,
    /// Compositor blur behind a barely tinted fill.
    Blur,
    /// Compositor blur behind a dark fill.
    BlurDim
}

impl BgMode {
    /// Every mode, in token order.
    const ALL: [Self; 4] = [Self::None, Self::Dim, Self::Blur, Self::BlurDim];

    /// The CSS class painting the window backdrop. The translucent fill is
    /// also the medium the compositor blur shows through, so blur-only mode
    /// tints the window faintly.
    const fn window_class(self) -> Option<&'static str> {
        match self {
            Self::None => None,
            Self::Blur => Some("hud-tint"),
            Self::Dim | Self::BlurDim => Some("hud-dim")
        }
    }

    const fn wants_blur(self) -> bool {
        matches!(self, Self::Blur | Self::BlurDim)
    }

    const fn dim_on(self) -> bool {
        matches!(self, Self::Dim | Self::BlurDim)
    }

    /// Combines the two independent backdrop checkboxes into a mode.
    const fn from_bools(dim: bool, blur: bool) -> Self {
        match (dim, blur) {
            (false, false) => Self::None,
            (true, false) => Self::Dim,
            (false, true) => Self::Blur,
            (true, true) => Self::BlurDim
        }
    }

    const fn token(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Dim => "dim",
            Self::Blur => "blur",
            Self::BlurDim => "blur-dim"
        }
    }

    fn from_token(token: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|mode| mode.token() == token)
            .unwrap_or_default()
    }
}

/// Path of the persisted backdrop mode, when a config home is available.
fn background_path() -> Option<PathBuf> {
    let base = std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|_| std::env::var("HOME").map(|home| PathBuf::from(home).join(".config")))
        .ok()?;
    Some(base.join("logi-mx").join("hud-background"))
}

/// Reads the persisted backdrop mode, defaulting to [`BgMode::Dim`].
fn load_background() -> BgMode {
    background_path()
        .and_then(|path| fs::read_to_string(path).ok())
        .map_or_else(BgMode::default, |token| BgMode::from_token(token.trim()))
}

/// Persists the backdrop mode; failures are logged, never fatal.
fn save_background(mode: BgMode) {
    let Some(path) = background_path() else {
        return;
    };
    if let Some(dir) = path.parent()
        && let Err(e) = fs::create_dir_all(dir)
    {
        tracing::warn!("Failed to create the config directory: {e}");
        return;
    }
    if let Err(e) = fs::write(&path, mode.token()) {
        tracing::warn!("Failed to save the background mode: {e}");
    }
}

/// Paints the window backdrop for the mode through CSS classes.
///
/// The blur class carries `backdrop-filter`, which GTK 4.24 translates into an
/// `ext-background-effect-v1` blur region for the compositor; no compositor
/// rules are involved.
fn apply_bg_classes(window: &ApplicationWindow, mode: BgMode) {
    window.remove_css_class("hud-dim");
    window.remove_css_class("hud-tint");
    window.remove_css_class("hud-blur");
    if let Some(class) = mode.window_class() {
        window.add_css_class(class);
    }
    if mode.wants_blur() {
        window.add_css_class("hud-blur");
    }
}

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
    Gesture
}

impl MouseButton {
    const ALL: [Self; 7] = [
        Self::Left,
        Self::Right,
        Self::Wheel,
        Self::DpiShift,
        Self::Back,
        Self::Forward,
        Self::Gesture
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
            Self::Gesture => control_id::GESTURE_BUTTON
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
        Self::Right
    ];

    const fn label(self) -> &'static str {
        match self {
            Self::Left => "Left",
            Self::Right => "Right",
            Self::Wheel => "Wheel Click",
            Self::DpiShift => "DPI Shift",
            Self::Back => "Back",
            Self::Forward => "Forward",
            Self::Gesture => "Gesture"
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
            Self::Gesture => (8.42, 61.69, 1.08, 6.3)
        }
    }

    /// Hit rectangle: the drawn region padded when too thin to click.
    const fn hit_region(self) -> (f64, f64, f64, f64) {
        let (x, y, w, h) = self.region();
        let pad = if w < THIN_HOTSPOT { HIT_PADDING } else { 0.0 };
        (x - pad, y - pad, w + 2.0 * pad, h + 2.0 * pad)
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
            Self::DpiShift => "button6-anchor"
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
            Self::DpiShift => "button6"
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
            Self::DpiShift => None
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
    buttons:     u32,
    /// Buttons pressed between two HUD frames; keeps brief taps visible.
    flash:       u32,
    wheel_speed: f64,
    wheel_dir:   i32,
    hwheel_dir:  i32,
    extra_keys:  Vec<u16>,
    connected:   bool
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

    /// Fades the velocity gauges toward zero and clears the tap flashes while
    /// the device is idle.
    ///
    /// Returns whether any gauge value actually moved.
    fn decay(&mut self) -> bool {
        let mut changed = self.flash != 0;
        self.flash = 0;
        if self.wheel_speed != 0.0 {
            self.wheel_speed *= 0.6;
            if self.wheel_speed < 0.5 {
                self.wheel_speed = 0.0;
            }
            changed = true;
        }
        changed
    }
}

/// Exponential smoothing of a new sample into a running value.
#[must_use]
fn blend(previous: f64, sample: f64) -> f64 {
    sample.mul_add(LIVE_EMA_ALPHA, previous * (1.0 - LIVE_EMA_ALPHA))
}

#[derive(Default)]
struct HudState {
    selected:    Option<MouseButton>,
    hover:       Option<MouseButton>,
    assignments: HashMap<MouseButton, String>,
    live:        LiveVisual,
    bg:          BgMode
}

/// One button tile: the card plus the dropdown assigning its behaviour.
#[derive(Clone)]
struct Tile {
    child:    FlowBoxChild,
    dropdown: DropDown
}

/// Shared handle to the button tiles of the open window, keyed by button.
type RowMap = Rc<RefCell<HashMap<MouseButton, Tile>>>;

thread_local! {
    /// The HUD window while it is open, so a second activation closes it
    /// instead of stacking another instance.
    static ACTIVE_WINDOW: RefCell<Option<ApplicationWindow>> = const { RefCell::new(None) };

    /// Main-thread callback that folds the newest telemetry frame into the
    /// open window; cleared when the HUD closes.
    static LIVE_HOOK: RefCell<Option<Rc<dyn Fn()>>> = const { RefCell::new(None) };

    /// Main-thread callback that applies the device snapshot delivered by
    /// the background loader; cleared when the HUD closes.
    static SNAPSHOT_HOOK: RefCell<Option<Rc<dyn Fn()>>> = const { RefCell::new(None) };

    /// Stop flag of the live subscription thread of the open window.
    static LIVE_STOP: RefCell<Option<Arc<AtomicBool>>> = const { RefCell::new(None) };

    /// Button rows of the open window, so diagram hover and clicks can
    /// highlight the matching list row; cleared when the HUD closes.
    static ROWS_REGISTRY: RefCell<Option<RowMap>> = const { RefCell::new(None) };

    /// Full-window drawing surface carrying the leader lines from the diagram
    /// to the settings rows; cleared when the HUD closes.
    static LEADERS_AREA: RefCell<Option<gtk4::DrawingArea>> = const { RefCell::new(None) };
}

/// Schedules a repaint of the leader-line overlay.
fn queue_leaders() {
    LEADERS_AREA.with(|area| {
        if let Some(area) = area.borrow().as_ref() {
            area.queue_draw();
        }
    });
}

/// Mirrors the focused button onto the list rows: hover wins over the
/// persistent selection, so both halves of the HUD always agree.
fn sync_row_focus(state: &HudState) {
    let focus = state.hover.or(state.selected);
    ROWS_REGISTRY.with(|registry| {
        let Some(rows) = registry.borrow().as_ref().map(Rc::clone) else {
            return;
        };
        for (button, tile) in rows.borrow().iter() {
            if Some(*button) == focus {
                tile.child.add_css_class("hud-focus");
            } else {
                tile.child.remove_css_class("hud-focus");
            }
        }
    });
    queue_leaders();
}

/// Opens the HUD, or closes it when it is already open.
///
/// This is the activation entry point: repeated launches (tray item or a
/// Hyprland keybind) toggle the HUD instead of stacking windows.
pub fn toggle_hud(app: &Application) {
    let existing = ACTIVE_WINDOW.with(|cell| cell.borrow().clone());
    let open_new = !existing.is_some_and(|window| {
        window.close();
        true
    });

    if open_new {
        build_hud(app);
    }
}

/// Builds and presents the full-screen HUD.
fn build_hud(app: &Application) {
    let window = create_window(app);
    let state = Rc::new(RefCell::new(HudState {
        bg: load_background(),
        ..HudState::default()
    }));
    let image = Rc::new(OnceCell::new());
    let masks = Rc::new(build_button_masks());
    let drawing = create_drawing_area();
    let leaders = create_leaders_area();
    LEADERS_AREA.with(|cell| *cell.borrow_mut() = Some(leaders.clone()));
    let widgets = Rc::new(RefCell::new(PanelWidgets::default()));
    let panel = create_panel(&state, &drawing, &window, &widgets);

    attach_hit_testing(&state, &drawing);
    attach_draw_callback(&state, &drawing, &image, &masks);
    attach_leaders_draw(&state, &leaders, &drawing, &window);
    attach_escape_to_close(&window);
    start_live(&state, &drawing);

    let root = GtkBox::new(Orientation::Horizontal, 0);
    root.set_margin_top(HUD_MARGIN);
    root.set_margin_bottom(HUD_MARGIN);
    root.set_margin_start(HUD_MARGIN);
    root.set_margin_end(HUD_MARGIN);
    root.append(&drawing);
    root.append(&panel);

    let overlay = gtk4::Overlay::new();
    overlay.set_child(Some(&root));
    overlay.add_overlay(&leaders);
    window.set_content(Some(&overlay));
    apply_bg_classes(&window, state.borrow().bg);

    ACTIVE_WINDOW.with(|cell| *cell.borrow_mut() = Some(window.clone()));
    window.connect_close_request(|_| {
        LIVE_STOP.with(|cell| {
            if let Some(stop) = cell.borrow_mut().take() {
                stop.store(true, Ordering::Relaxed);
            }
        });
        LIVE_HOOK.with(|hook| *hook.borrow_mut() = None);
        SNAPSHOT_HOOK.with(|hook| *hook.borrow_mut() = None);
        ROWS_REGISTRY.with(|rows| *rows.borrow_mut() = None);
        LEADERS_AREA.with(|area| *area.borrow_mut() = None);
        ACTIVE_WINDOW.with(|cell| *cell.borrow_mut() = None);
        glib::Propagation::Proceed
    });

    window.present();
    spawn_snapshot_loader(&widgets);
}

fn create_window(app: &Application) -> ApplicationWindow {
    let window = ApplicationWindow::builder()
        .application(app)
        .title("Logitech MX Master 3S — HUD")
        .default_width(1100)
        .default_height(700)
        .build();

    window.init_layer_shell();
    window.set_namespace(Some("logi-mx-hud"));
    window.set_exclusive_zone(-1);
    window.add_css_class("transparent-overlay");
    window.remove_css_class("background");
    window.set_layer(Layer::Overlay);
    window.set_keyboard_mode(KeyboardMode::OnDemand);
    window.set_anchor(Edge::Top, true);
    window.set_anchor(Edge::Bottom, true);
    window.set_anchor(Edge::Left, true);
    window.set_anchor(Edge::Right, true);

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

/// Builds the transparent full-window surface the leader lines are drawn on.
fn create_leaders_area() -> gtk4::DrawingArea {
    let area = gtk4::DrawingArea::builder()
        .hexpand(true)
        .vexpand(true)
        .build();
    area.set_can_target(false);
    area
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

/// Panel widgets whose values come from the device snapshot.
///
/// The panel is built with placeholders before the window is presented; the
/// background loader fills these widgets in as soon as the daemon answers,
/// so the first frame never waits on device I/O.
#[derive(Default)]
struct PanelWidgets {
    /// Model row on the device page.
    model_row:         Option<ActionRow>,
    /// Battery row on the device page.
    battery_row:       Option<ActionRow>,
    /// Current-DPI row on the pointer page.
    dpi_row:           Option<ActionRow>,
    /// DPI slider on the pointer page.
    dpi_scale:         Option<Scale>,
    /// `SmartShift` enable switch.
    smartshift_switch: Option<Switch>,
    /// `SmartShift` threshold slider.
    smartshift_scale:  Option<Scale>,
    /// `SmartShift` threshold subtitle row.
    smartshift_row:    Option<ActionRow>,
    /// Hi-res scroll switch.
    hires_switch:      Option<Switch>,
    /// Natural scrolling switch.
    inverted_switch:   Option<Switch>,
    /// Set once a snapshot (or its failure) has been applied; later
    /// deliveries never overwrite values the user may have edited.
    applied:           bool
}

/// Fills the registered panel widgets from a device snapshot.
///
/// `None` marks the device unreachable and leaves the controls at their
/// defaults with explicit "unavailable" subtitles.
fn apply_snapshot(widgets: &Rc<RefCell<PanelWidgets>>, snapshot: Option<&DeviceSnapshot>) {
    {
        let mut w = widgets.borrow_mut();
        if w.applied {
            return;
        }
        w.applied = true;
    }
    let w = widgets.borrow();
    let Some(snapshot) = snapshot else {
        if let Some(row) = &w.model_row {
            row.set_subtitle("daemon unavailable");
        }
        if let Some(row) = &w.battery_row {
            row.set_subtitle("unknown");
        }
        if let Some(row) = &w.dpi_row {
            row.set_subtitle("unknown");
        }
        return;
    };
    if let Some(row) = &w.model_row {
        row.set_subtitle(snapshot.name.as_str());
    }
    if let Some(row) = &w.battery_row {
        row.set_subtitle(&snapshot.battery.map_or_else(
            || "unknown".to_string(),
            |b| format!("{}% · {:?}", b.level, b.status)
        ));
    }
    if let Some(row) = &w.dpi_row {
        row.set_subtitle(&format!("{} DPI", snapshot.dpi));
    }
    if let Some(scale) = &w.dpi_scale {
        scale.set_value(f64::from(snapshot.dpi));
    }
    if let Some(switch) = &w.smartshift_switch {
        switch.set_active(snapshot.smartshift.enabled);
    }
    if let Some(scale) = &w.smartshift_scale {
        scale.set_value(f64::from(snapshot.smartshift.threshold));
    }
    if let Some(row) = &w.smartshift_row {
        row.set_subtitle(&format!("Current: {}", snapshot.smartshift.threshold));
    }
    if let Some(switch) = &w.hires_switch {
        switch.set_active(snapshot.hiresscroll.enabled);
    }
    if let Some(switch) = &w.inverted_switch {
        switch.set_active(snapshot.hiresscroll.inverted);
    }
}

/// Loads the device snapshot off the main thread and applies it to the panel.
///
/// Device I/O can take seconds (daemon round-trips, direct fallback scans),
/// so it must never block the first frame. The worker parks the result in a
/// shared slot and wakes the main context, where the registered hook applies
/// it; a failed load still applies and marks the device unavailable.
fn spawn_snapshot_loader(widgets: &Rc<RefCell<PanelWidgets>>) {
    let slot = Arc::new(Mutex::new(None::<Option<DeviceSnapshot>>));
    let slot_thread = Arc::clone(&slot);
    let slot_hook = Arc::clone(&slot);
    let widgets_hook = Rc::clone(widgets);
    let hook: Rc<dyn Fn()> = Rc::new(move || {
        let snapshot = slot_hook.lock().ok().and_then(|mut guard| guard.take());
        if let Some(snapshot) = snapshot {
            apply_snapshot(&widgets_hook, snapshot.as_ref());
        }
    });
    SNAPSHOT_HOOK.with(|cell| *cell.borrow_mut() = Some(hook));
    let context = glib::MainContext::default();
    if let Err(e) = thread::Builder::new()
        .name("snapshot-loader".to_string())
        .spawn(move || {
            let snapshot = collect_device_snapshot();
            if let Ok(mut guard) = slot_thread.lock() {
                *guard = Some(snapshot);
            }
            context.invoke(|| {
                SNAPSHOT_HOOK.with(|cell| {
                    if let Some(hook) = cell.borrow().as_ref() {
                        hook();
                    }
                });
            });
        })
    {
        tracing::warn!("Failed to spawn the snapshot loader thread: {e}");
        apply_snapshot(widgets, None);
    }
}

/// Builds the settings panel with the Device, Pointer, Wheel and Buttons pages.
fn create_panel(
    state: &Rc<RefCell<HudState>>,
    drawing: &gtk4::DrawingArea,
    window: &ApplicationWindow,
    widgets: &Rc<RefCell<PanelWidgets>>
) -> ToastOverlay {
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

    let stack = ViewStack::new();
    let pages = [
        (
            buttons_page(state, drawing, &toast),
            "buttons",
            "Buttons",
            "input-mouse-symbolic"
        ),
        (
            wheel_page(&toast, widgets),
            "wheel",
            "Wheel",
            "view-continuous-symbolic"
        ),
        (
            pointer_page(&toast, widgets),
            "pointer",
            "Pointer",
            "preferences-desktop-input-symbolic"
        ),
        (
            device_page(state, window, &toast, widgets),
            "device",
            "Device",
            "battery-symbolic"
        )
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
    stack.connect_visible_child_notify(move |_| queue_leaders());

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
    toast
}

/// Native remap targets offered on the assignment dropdowns: label and CID.
const REMAP_TARGETS: [(&str, u16); 7] = [
    ("Left", control_id::LEFT_BUTTON),
    ("Right", control_id::RIGHT_BUTTON),
    ("Middle click", control_id::MIDDLE_BUTTON),
    ("Back", control_id::BACK_BUTTON),
    ("Forward", control_id::FORWARD_BUTTON),
    ("Gesture", control_id::GESTURE_BUTTON),
    ("DPI Shift", control_id::SMARTSHIFT_TOGGLE)
];

/// Dropdown choices for a tile: index 0 restores the device-native behaviour,
/// every later entry maps onto the matching `REMAP_TARGETS` element.
const ASSIGNMENT_CHOICES: [&str; 8] = [
    "Native",
    "Left",
    "Right",
    "Middle click",
    "Back",
    "Forward",
    "Gesture",
    "DPI Shift"
];

/// Smallest height (in pixels) one tile row needs to stay comfortable; the
/// flow drops to two columns only when the panel cannot fit every row at this
/// height in a single column.
const TILE_ROW_MIN_HEIGHT: i32 = 64;

/// Builds one button tile: a card with the button name and its assignment
/// dropdown; picking an entry writes the remap into the mouse firmware
/// instantly and reverts the choice when the daemon refuses.
fn button_tile(
    state: &Rc<RefCell<HudState>>,
    drawing: &gtk4::DrawingArea,
    toast: &ToastOverlay,
    rows: &RowMap,
    button: MouseButton
) -> Tile {
    let child = FlowBoxChild::new();
    child.add_css_class("hud-tile");
    child.set_hexpand(true);
    child.set_vexpand(true);
    let inner = GtkBox::new(Orientation::Horizontal, 12);
    let name = Label::new(Some(button.label()));
    name.add_css_class("title");
    name.set_xalign(0.0);
    name.set_hexpand(true);
    name.set_valign(gtk4::Align::Center);
    let dropdown = DropDown::from_strings(&ASSIGNMENT_CHOICES);
    dropdown.add_css_class("hud-assign");
    dropdown.set_valign(gtk4::Align::Center);
    inner.append(&name);
    inner.append(&dropdown);
    child.set_child(Some(&inner));

    let state_row = Rc::clone(state);
    let drawing_row = drawing.clone();
    child.connect_activate(move |_| {
        let mut st = state_row.borrow_mut();
        st.selected = Some(button);
        drawing_row.queue_draw();
        sync_row_focus(&st);
    });

    let state_enter = Rc::clone(state);
    let drawing_enter = drawing.clone();
    let motion = EventControllerMotion::new();
    motion.connect_motion(move |_, _, _| {
        let mut st = state_enter.borrow_mut();
        if st.hover != Some(button) {
            st.hover = Some(button);
            drawing_enter.queue_draw();
            sync_row_focus(&st);
        }
    });
    let state_leave = Rc::clone(state);
    let drawing_leave = drawing.clone();
    motion.connect_leave(move |_| {
        let mut st = state_leave.borrow_mut();
        if st.hover == Some(button) {
            st.hover = None;
            drawing_leave.queue_draw();
            sync_row_focus(&st);
        }
    });
    child.add_controller(motion);
    child.connect_map(|_| queue_leaders());

    let state_assign = Rc::clone(state);
    let rows_assign = Rc::clone(rows);
    let drawing_assign = drawing.clone();
    let toast_assign = toast.clone();
    let applied = Rc::new(Cell::new(0_u32));
    dropdown.connect_selected_notify(move |picker| {
        let index = picker.selected();
        if index == applied.get() {
            return;
        }
        let target_cid = if index == 0 {
            0
        } else {
            REMAP_TARGETS[(index - 1) as usize].1
        };
        let response = ipc::try_request(&ipc::Request::SetButtonRemap {
            control_id: button.cid(),
            target_cid
        });
        if matches!(response, Some(Response::Ok)) {
            applied.set(index);
            let label =
                (target_cid != 0).then(|| REMAP_TARGETS[(index - 1) as usize].0.to_string());
            set_assignment(
                &state_assign,
                &rows_assign,
                &drawing_assign,
                button,
                label.clone()
            );
            let message = label.map_or_else(
                || format!("{} restored to native", button.label()),
                |target| format!("{} now acts as {target}", button.label())
            );
            toast_assign.add_toast(Toast::new(&message));
        } else {
            picker.set_selected(applied.get());
            toast_assign.add_toast(Toast::new("Daemon unavailable — change not applied"));
        }
    });

    Tile {
        child,
        dropdown
    }
}

/// Builds the button tiles: an adaptive flow of cards, each pairing the
/// button with a dropdown that assigns its behaviour straight into the mouse
/// firmware. The flow fits one or two columns to the available width.
/// Hovering a tile highlights the button on the diagram.
fn buttons_list_section(
    state: &Rc<RefCell<HudState>>,
    drawing: &gtk4::DrawingArea,
    toast: &ToastOverlay
) -> (GtkBox, RowMap) {
    let section = GtkBox::new(Orientation::Vertical, 8);
    section.set_vexpand(true);
    let title = Label::new(Some("Buttons"));
    title.add_css_class("title-4");
    title.set_xalign(0.0);
    let description = Label::new(Some(
        "Pick what each button should do; the mouse remembers it on its own"
    ));
    description.add_css_class("dim-label");
    description.add_css_class("caption");
    description.set_xalign(0.0);

    let flow = FlowBox::new();
    flow.add_css_class("hud-tiles");
    flow.set_selection_mode(SelectionMode::None);
    flow.set_homogeneous(false);
    flow.set_min_children_per_line(1);
    flow.set_max_children_per_line(1);
    flow.set_hexpand(true);
    flow.set_vexpand(true);
    flow.set_column_spacing(12);
    flow.set_row_spacing(12);

    flow.connect_notify_local(Some("height"), move |flow, _| {
        let rows = i32::try_from(MouseButton::ALL.len()).unwrap_or(7);
        let fits_one_column = flow.height() >= TILE_ROW_MIN_HEIGHT * rows;
        let wanted = if fits_one_column { 1 } else { 2 };
        if flow.max_children_per_line() != wanted {
            flow.set_max_children_per_line(wanted);
        }
    });

    let rows: RowMap = Rc::new(RefCell::new(HashMap::new()));
    for button in MouseButton::ALL {
        let card = button_tile(state, drawing, toast, &rows, button);
        rows.borrow_mut().insert(button, card.clone());
        flow.append(&card.child);
    }
    ROWS_REGISTRY.with(|cell| *cell.borrow_mut() = Some(Rc::clone(&rows)));

    section.append(&title);
    section.append(&description);
    section.append(&flow);
    (section, rows)
}

/// Stores an assignment for a button and reflects it in the tile dropdown and
/// the diagram leader label; `None` restores the native behaviour.
fn set_assignment(
    state: &Rc<RefCell<HudState>>,
    rows: &RowMap,
    drawing: &gtk4::DrawingArea,
    button: MouseButton,
    action: Option<String>
) {
    let index = action.as_ref().map_or(0, |label| {
        REMAP_TARGETS
            .iter()
            .position(|(target, _)| target == label)
            .and_then(|pos| u32::try_from(pos + 1).ok())
            .unwrap_or(0)
    });
    match action {
        None => {
            state.borrow_mut().assignments.remove(&button);
        }
        Some(action) => {
            state.borrow_mut().assignments.insert(button, action);
        }
    }
    if let Some(tile) = rows.borrow().get(&button) {
        tile.dropdown.set_selected(index);
    }
    drawing.queue_draw();
    queue_leaders();
}

/// Creates the assignment page: adaptive button tiles that highlight the
/// diagram, each carrying its own assignment dropdown.
fn buttons_page(
    state: &Rc<RefCell<HudState>>,
    drawing: &gtk4::DrawingArea,
    toast: &ToastOverlay
) -> ScrolledWindow {
    let scroll = page_scroll();
    scroll
        .vadjustment()
        .connect_value_changed(move |_| queue_leaders());
    let box_ = GtkBox::new(Orientation::Vertical, 0);
    let (buttons_section, _rows) = buttons_list_section(state, drawing, toast);
    box_.append(&buttons_section);
    scroll.set_child(Some(&box_));
    scroll
}

/// Creates the wheel page: `SmartShift`, hi-res and direction settings.
///
/// Controls start at neutral placeholders; the snapshot loader fills them in
/// once the daemon answers.
fn wheel_page(toast: &ToastOverlay, widgets: &Rc<RefCell<PanelWidgets>>) -> ScrolledWindow {
    let scroll = page_scroll();
    let box_ = GtkBox::new(Orientation::Vertical, 0);

    let shift_group = PreferencesGroup::new();
    shift_group.set_title("SmartShift");
    shift_group.set_description(Some("Automatic switching between ratchet and freespin"));

    let switch_row = ActionRow::new();
    switch_row.set_title("Enable SmartShift");
    let shift_switch = Switch::new();
    shift_switch.set_valign(gtk4::Align::Center);
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
    let tr = threshold_row.clone();
    threshold_scale.connect_value_changed(move |s| {
        tr.set_subtitle(&format!("Current: {}", scale_threshold(s.value())));
    });

    let shift_apply = Button::with_label("Apply");
    shift_apply.add_css_class("suggested-action");
    shift_apply.add_css_class("pill");
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
    hires_row.add_suffix(&hires_switch);
    hires_row.set_activatable_widget(Some(&hires_switch));

    let inverted_row = ActionRow::new();
    inverted_row.set_title("Natural Scrolling");
    let inverted_switch = Switch::new();
    inverted_switch.set_valign(gtk4::Align::Center);
    inverted_row.add_suffix(&inverted_switch);
    inverted_row.set_activatable_widget(Some(&inverted_switch));

    let scroll_apply = Button::with_label("Apply");
    scroll_apply.add_css_class("suggested-action");
    scroll_apply.add_css_class("pill");
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
    drop(w);
    scroll
}

/// Creates the pointer page with the DPI slider.
///
/// The slider starts at the neutral default; the snapshot loader fills in
/// the real value once the daemon answers.
fn pointer_page(toast: &ToastOverlay, widgets: &Rc<RefCell<PanelWidgets>>) -> ScrolledWindow {
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
    drop(w);
    scroll
}

/// Builds the backdrop section: two independent checkboxes for dimming and
/// compositor blur, so any of the four combinations is reachable. Changing
/// either persists the mode and repaints the window backdrop.
fn background_section(
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

/// Creates the device page: battery, backdrop, desktop integration and
/// version.
///
/// Model and battery start as "Detecting…"; the snapshot loader fills them
/// in once the daemon answers.
fn device_page(
    state: &Rc<RefCell<HudState>>,
    window: &ApplicationWindow,
    toast: &ToastOverlay,
    widgets: &Rc<RefCell<PanelWidgets>>
) -> ScrolledWindow {
    let scroll = page_scroll();
    let box_ = GtkBox::new(Orientation::Vertical, 0);

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
    let br = battery_row.clone();
    let to = toast.clone();
    refresh.connect_clicked(move |_| match ipc::try_request(&ipc::Request::Battery) {
        Some(Response::Battery {
            level,
            status
        }) => {
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
        Some(Response::Error {
            message
        }) => message,
        _ => "Daemon unavailable".to_string()
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
fn start_live(state: &Rc<RefCell<HudState>>, drawing: &gtk4::DrawingArea) {
    let stop = Arc::new(AtomicBool::new(false));
    let slot = Arc::new(Mutex::new(None::<LiveFrame>));
    let burst = Arc::new(AtomicU32::new(0));

    let connected = Arc::new(AtomicBool::new(false));
    let slot_hook = Arc::clone(&slot);
    let burst_hook = Arc::clone(&burst);
    let connected_hook = Arc::clone(&connected);
    let state_hook = Rc::clone(state);
    let drawing_hook = drawing.clone();
    let hook: Rc<dyn Fn()> = Rc::new(move || {
        let frame = slot_hook.lock().ok().and_then(|mut guard| guard.take());
        let flashed = burst_hook.swap(0, Ordering::Relaxed);
        {
            let mut st = state_hook.borrow_mut();
            st.live.connected = connected_hook.load(Ordering::Relaxed);
            if let Some(frame) = frame {
                st.live.apply(&frame);
            }
            st.live.flash |= flashed;
        }
        drawing_hook.queue_draw();
    });
    LIVE_HOOK.with(|cell| *cell.borrow_mut() = Some(Rc::clone(&hook)));
    LIVE_STOP.with(|cell| *cell.borrow_mut() = Some(Arc::clone(&stop)));

    let context = glib::MainContext::default();
    let stop_thread = Arc::clone(&stop);
    let slot_thread = Arc::clone(&slot);
    let burst_thread = Arc::clone(&burst);
    let connected_thread = Arc::clone(&connected);
    let pending = Arc::new(AtomicBool::new(false));
    let wake = move || {
        let context = context.clone();
        let pending = Arc::clone(&pending);
        if pending.swap(true, Ordering::Relaxed) {
            return;
        }
        context.invoke(move || {
            pending.store(false, Ordering::Relaxed);
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
                pump_live_socket(
                    socket,
                    &slot_thread,
                    &burst_thread,
                    &connected_thread,
                    &stop_thread,
                    &wake
                );
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
///
/// Every frame's button bits are also folded into `burst`, so a press that
/// starts and ends between two main-context wakes still reaches the flash
/// mask instead of being overwritten by the newest frame.
fn pump_live_socket(
    socket: std::os::unix::net::UnixStream,
    slot: &Arc<Mutex<Option<LiveFrame>>>,
    burst: &Arc<AtomicU32>,
    connected: &Arc<AtomicBool>,
    stop: &AtomicBool,
    wake: &dyn Fn()
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
                burst.fetch_or(frame.buttons, Ordering::Relaxed);
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
            Err(_) => break
        }
    }
    connected.store(false, Ordering::Relaxed);
    wake();
}

/// Wires diagram interaction: a release selects the button under the cursor
/// and motion hovers it, both mirrored onto the button list rows.
fn attach_hit_testing(state: &Rc<RefCell<HudState>>, drawing: &gtk4::DrawingArea) {
    let gesture = GestureClick::new();
    let state_gesture = Rc::clone(state);
    let drawing_gesture = drawing.clone();
    gesture.connect_released(move |_gesture, _n_press, px, py| {
        let (x, y) = to_layout(px, py, drawing_gesture.width(), drawing_gesture.height());
        let mut st = state_gesture.borrow_mut();
        for button in MouseButton::HIT_ORDER {
            if button.contains(x, y) {
                st.selected = Some(button);
                drawing_gesture.queue_draw();
                sync_row_focus(&st);
                return;
            }
        }
    });
    drawing.add_controller(gesture);

    let motion = EventControllerMotion::new();
    let state_motion = Rc::clone(state);
    let drawing_motion = drawing.clone();
    motion.connect_motion(move |_, px, py| {
        let (x, y) = to_layout(px, py, drawing_motion.width(), drawing_motion.height());
        let hit = MouseButton::HIT_ORDER
            .iter()
            .copied()
            .find(|button| button.contains(x, y));
        let mut st = state_motion.borrow_mut();
        if st.hover != hit {
            st.hover = hit;
            drawing_motion.queue_draw();
            sync_row_focus(&st);
        }
    });
    let state_leave = Rc::clone(state);
    let drawing_leave = drawing.clone();
    motion.connect_leave(move |_| {
        let mut st = state_leave.borrow_mut();
        if st.hover.is_some() {
            st.hover = None;
            drawing_leave.queue_draw();
            sync_row_focus(&st);
        }
    });
    drawing.add_controller(motion);
}

fn attach_draw_callback(
    state: &Rc<RefCell<HudState>>,
    drawing: &gtk4::DrawingArea,
    image: &Rc<OnceCell<Option<gtk4::cairo::ImageSurface>>>,
    masks: &Rc<ButtonMasks>
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

        if let Some((sx, sy)) = surface_scale {
            draw_selection(&st, cr, sx, sy, &masks_draw);
            draw_pressed_shapes(&st, cr, sx, sy, &masks_draw.fill);
        }
        let _ = cr.restore();

        draw_wheel_indicator(&st, cr, scale, offset_x, offset_y);
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
        (width * 4).cast_signed()
    )
    .ok()
}

/// Rasterizes the embedded device SVG once into a cairo surface.
fn render_device_svg() -> Option<gtk4::cairo::ImageSurface> {
    render_svg_to_surface(DEVICE_SVG.as_bytes())
}

/// Builds a standalone SVG containing only the device element with the
/// given id, injecting `style` presentation attributes into its tag.
///
/// The layout SVG keeps button shapes attribute-free, so injected `fill`
/// and `stroke` attributes always apply without conflicts.
fn device_element_svg_styled(id: &str, style: &str) -> Option<String> {
    let marker = format!("id=\"{id}\"");
    let at = DEVICE_SVG.find(&marker)?;
    let open = DEVICE_SVG[..at].rfind('<')?;
    let close = DEVICE_SVG[at..].find("/>")? + at + 2;
    let element = &DEVICE_SVG[open..close];
    let element = if style.is_empty() {
        std::borrow::Cow::Borrowed(element)
    } else {
        std::borrow::Cow::Owned(element.replacen("/>", &format!(" {style}/>"), 1))
    };
    Some(format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {LAYOUT_W} {LAYOUT_H}\">\
         <g transform=\"translate(0,{DEVICE_GROUP_DY})\">{element}</g></svg>"
    ))
}

/// Builds a standalone SVG containing only the device element with the
/// given id, so it can be rendered as an alpha mask aligned with the base
/// image.
fn device_element_svg(id: &str) -> Option<String> {
    device_element_svg_styled(id, "")
}

/// Alpha masks of every button shape, rendered once from the layout SVG.
#[derive(Default)]
struct ButtonMasks {
    /// Exact shape of each button, tinted for highlights and presses.
    fill:    HashMap<MouseButton, gtk4::cairo::ImageSurface>,
    /// Contour of each button, painted as the selection outline.
    outline: HashMap<MouseButton, gtk4::cairo::ImageSurface>
}

/// Stroke width of the selection outline, in layout units.
const OUTLINE_STROKE: f64 = 0.5;

/// Renders the fill and outline alpha masks of every button shape.
fn build_button_masks() -> ButtonMasks {
    let mut masks = ButtonMasks::default();
    let outline_style =
        format!("fill=\"none\" stroke=\"#000000\" stroke-width=\"{OUTLINE_STROKE}\"");
    for button in MouseButton::ALL {
        if let Some(svg) = device_element_svg(button.element_id())
            && let Some(surface) = render_svg_to_surface(svg.as_bytes())
        {
            masks.fill.insert(button, surface);
        }
        if let Some(svg) = device_element_svg_styled(button.element_id(), &outline_style)
            && let Some(surface) = render_svg_to_surface(svg.as_bytes())
        {
            masks.outline.insert(button, surface);
        }
    }
    masks
}

/// Computes uniform scale and centering offsets for the layout space.
/// The mouse is fitted and centered exactly as in the original design.
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

/// Paints one button's alpha mask with a flat color, aligned to the base
/// image through the raster-to-layout scale.
fn paint_button_shape(
    cr: &gtk4::cairo::Context,
    sx: f64,
    sy: f64,
    mask: &gtk4::cairo::ImageSurface,
    color: (f64, f64, f64, f64)
) {
    let pattern = gtk4::cairo::SurfacePattern::create(mask);
    let _ = cr.save();
    cr.scale(sx, sy);
    cr.set_source_rgba(color.0, color.1, color.2, color.3);
    let _ = cr.mask(&pattern);
    let _ = cr.restore();
}

/// Highlight colors: hover and selection share the accent, assignments are
/// tinted blue.
const ACCENT: (f64, f64, f64) = (0.93, 0.66, 0.46);
const ASSIGNED: (f64, f64, f64) = (0.42, 0.60, 0.80);

/// Highlights hovered, selected and assigned buttons by tinting their exact
/// SVG shapes; the selection additionally carries a contour outline.
fn draw_selection(
    st: &HudState,
    cr: &gtk4::cairo::Context,
    sx: f64,
    sy: f64,
    masks: &ButtonMasks
) {
    for button in MouseButton::ALL {
        let selected = st.selected == Some(button);
        let hovered = st.hover == Some(button);
        let assigned = st.assignments.contains_key(&button);
        if !selected && !hovered && !assigned {
            continue;
        }

        let Some(fill) = masks.fill.get(&button) else {
            continue;
        };
        let color = if selected || hovered {
            (ACCENT.0, ACCENT.1, ACCENT.2, 0.55)
        } else {
            (ASSIGNED.0, ASSIGNED.1, ASSIGNED.2, 0.50)
        };
        paint_button_shape(cr, sx, sy, fill, color);

        if selected && let Some(outline) = masks.outline.get(&button) {
            paint_button_shape(cr, sx, sy, outline, (ACCENT.0, ACCENT.1, ACCENT.2, 1.0));
        }
    }
}

/// Tints the exact SVG shape of every physically pressed button.
fn draw_pressed_shapes(
    st: &HudState,
    cr: &gtk4::cairo::Context,
    sx: f64,
    sy: f64,
    masks: &HashMap<MouseButton, gtk4::cairo::ImageSurface>
) {
    for button in MouseButton::ALL {
        let Some(bit) = button.live_bit() else {
            continue;
        };
        if (st.live.buttons | st.live.flash) & bit == 0 {
            continue;
        }
        let Some(mask) = masks.get(&button) else {
            continue;
        };
        paint_button_shape(cr, sx, sy, mask, (ACCENT.0, ACCENT.1, ACCENT.2, 0.9));
    }
}

/// Draws chevrons near the wheel showing scroll direction and velocity.
#[allow(
    clippy::suboptimal_flops,
    reason = "plain arithmetic keeps gauge geometry readable"
)]
fn draw_wheel_indicator(
    st: &HudState,
    cr: &gtk4::cairo::Context,
    scale: f64,
    offset_x: f64,
    offset_y: f64
) {
    let intensity = (st.live.wheel_speed / WHEEL_FULL_SCALE).min(1.0);
    if intensity <= 0.01 {
        return;
    }

    let Some(&(ax, ay)) = button_anchors().get(&MouseButton::Wheel) else {
        return;
    };
    let (ax, ay) = (offset_x + ax * scale, offset_y + ay * scale);
    let dir = st.live.wheel_dir;
    let vertical = dir != 0;
    let (ux, uy) = if vertical {
        (0.0, f64::from(dir))
    } else {
        (f64::from(st.live.hwheel_dir), 0.0)
    };

    cr.set_source_rgba(0.93, 0.66, 0.46, 0.4 + 0.6 * intensity);
    cr.set_line_width(1.2);
    for step in 1..=3 {
        let dist = (3.0 + f64::from(step) * 2.2) * scale;
        let cx = ax + ux * dist + if vertical { 6.5 * scale } else { 0.0 };
        let cy = ay + uy * dist + if vertical { 0.0 } else { -5.0 * scale };
        let half = 1.6 * scale;
        let (px, py) = if vertical {
            (half, half * 0.6 * f64::from(dir))
        } else {
            (half * 0.6 * f64::from(st.live.hwheel_dir), half)
        };
        cr.move_to(
            cx - (if vertical { half } else { 0.0 }),
            cy - (if vertical { 0.0 } else { half })
        );
        cr.line_to(cx + px * 0.4, cy + py * 0.4);
        cr.line_to(
            cx + (if vertical { half } else { 0.0 }),
            cy + (if vertical { 0.0 } else { half })
        );
        let _ = cr.stroke();
    }
}

/// Draws one leader line per button from its diagram anchor to the matching
/// row of the Buttons page, so the diagram and the settings list read as one
/// surface.
///
/// The overlay spans the whole window: anchors are projected through the
/// diagram transform into window space, rows through their own allocations.
/// Lines land on the left edge of each row, clamped to the scroll viewport so
/// a row scrolled out of view still keeps its line pointing the right way.
/// The focused button (hover or selection) draws bright and thick.
fn attach_leaders_draw(
    state: &Rc<RefCell<HudState>>,
    leaders: &gtk4::DrawingArea,
    drawing: &gtk4::DrawingArea,
    window: &ApplicationWindow
) {
    let state_draw = Rc::clone(state);
    let diagram = drawing.clone();
    let window = window.clone();
    leaders.set_draw_func(move |_, cr, _, _| {
        let st = state_draw.borrow();
        let Some(bounds) = diagram.compute_bounds(&window) else {
            return;
        };
        let (scale, offset_x, offset_y) =
            fit_transform(f64::from(diagram.width()), f64::from(diagram.height()));
        let base_x = f64::from(bounds.x()) + offset_x;
        let base_y = f64::from(bounds.y()) + offset_y;
        let Some(rows) = ROWS_REGISTRY.with(|registry| registry.borrow().as_ref().map(Rc::clone))
        else {
            return;
        };
        let rows = rows.borrow();
        let anchors = button_anchors();

        for button in MouseButton::ALL {
            let Some(&(ax, ay)) = anchors.get(&button) else {
                continue;
            };
            let Some(tile) = rows.get(&button) else {
                continue;
            };
            let ax = ax.mul_add(scale, base_x);
            let ay = ay.mul_add(scale, base_y);
            let Some((tx, ty)) = row_target(&tile.child, &window) else {
                continue;
            };
            let focused = st.selected == Some(button) || st.hover == Some(button);
            if focused {
                cr.set_source_rgba(0.93, 0.66, 0.46, 0.95);
                cr.set_line_width(2.0);
            } else {
                cr.set_source_rgba(0.62, 0.65, 0.70, 0.40);
                cr.set_line_width(1.0);
            }
            let mid_x = f64::midpoint(ax, tx);
            cr.move_to(ax, ay);
            cr.curve_to(mid_x, ay, mid_x, ty, tx, ty);
            let _ = cr.stroke();

            cr.new_path();
            cr.arc(ax, ay, 2.2, 0.0, std::f64::consts::TAU);
            let _ = cr.fill();
            cr.new_path();
            let tip = if focused { 3.0 } else { 2.0 };
            cr.arc(tx, ty, tip, 0.0, std::f64::consts::TAU);
            let _ = cr.fill();
        }
    });
}

/// Window-space landing point of a button tile's leader line: just left of
/// the tile, vertically centred and clamped to its scrolled viewport. `None`
/// when the tile is not on screen, for example while another page is open.
fn row_target(tile: &impl IsA<gtk4::Widget>, window: &ApplicationWindow) -> Option<(f64, f64)> {
    if !tile.is_mapped() {
        return None;
    }
    let bounds = tile.compute_bounds(window)?;
    let x = f64::from(bounds.x()) - 4.0;
    let mut y = f64::from(bounds.y()) + f64::from(bounds.height()) / 2.0;
    if let Some(scroll) = tile.ancestor(gtk4::ScrolledWindow::static_type())
        && let Some(area) = scroll.compute_bounds(window)
    {
        let top = f64::from(area.y()) + 8.0;
        let bottom = (f64::from(area.y()) + f64::from(area.height()) - 8.0).max(top);
        y = y.clamp(top, bottom);
    }
    Some((x, y))
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
        parse_attr_number(tail, "cy")?
    ))
}

fn parse_attr_number(text: &str, attribute: &str) -> Option<f64> {
    let tail = &text[text.find(attribute)? + attribute.len()..];
    let tail = &tail[tail.find('"')? + 1..];
    tail[..tail.find('"')?].parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn styled_element_svg_injects_attributes_before_close() {
        let svg = device_element_svg_styled("button4", "fill=\"none\" stroke=\"#000000\"")
            .expect("button4 present");
        assert!(svg.contains("fill=\"none\" stroke=\"#000000\"/>"));
        assert!(svg.contains("id=\"button4\""));
    }

    #[test]
    fn styled_element_svg_rejects_unknown_ids() {
        assert!(device_element_svg_styled("button99", "fill=\"none\"").is_none());
    }

    #[test]
    fn button_masks_cover_every_button() {
        let masks = build_button_masks();
        assert_eq!(masks.fill.len(), MouseButton::ALL.len());
        assert_eq!(masks.outline.len(), MouseButton::ALL.len());
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
    fn flash_survives_release_and_decays_away() {
        let mut live = LiveVisual::default();
        let pressed = LiveFrame {
            buttons: BTN_BIT_LEFT,
            ..LiveFrame::default()
        };
        live.apply(&pressed);
        live.flash |= BTN_BIT_LEFT;

        let released = LiveFrame {
            buttons: 0,
            ..LiveFrame::default()
        };
        live.apply(&released);
        assert_eq!(live.buttons, 0);
        assert_ne!(live.flash & BTN_BIT_LEFT, 0);

        assert!(live.decay());
        assert_eq!(live.flash, 0);
        assert!(!live.decay());
    }

    #[test]
    fn background_mode_round_trips_through_bools_and_tokens() {
        for mode in BgMode::ALL {
            assert_eq!(BgMode::from_bools(mode.dim_on(), mode.wants_blur()), mode);
            assert_eq!(BgMode::from_token(mode.token()), mode);
        }
        assert_eq!(BgMode::from_bools(false, false), BgMode::None);
        assert_eq!(BgMode::from_bools(true, true), BgMode::BlurDim);
        assert_eq!(BgMode::from_token("garbage"), BgMode::default());
    }
}
