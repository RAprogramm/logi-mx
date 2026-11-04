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
//!
//! This module owns window creation and assembly, the shared `HudState`,
//! the `PanelWidgets` registry and the small helpers the page submodules
//! (`buttons`, `gestures`, `wheel`, `device`) and the diagram renderer
//! (`diagram`) are built on.

mod buttons;
mod device;
mod diagnostics;
mod diagram;
mod gestures;
mod keybinds;
mod pie;
mod toasts;
mod wheel;

use std::{
    cell::{Cell, OnceCell, RefCell},
    collections::HashMap,
    fs,
    io::BufReader,
    path::PathBuf,
    rc::Rc,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU32, Ordering}
    },
    thread,
    time::Duration
};

use gtk4::{
    Box as GtkBox, Button, CheckButton, DropDown, EventControllerKey, EventControllerMotion,
    FlowBoxChild, GestureClick, Image, Label, Orientation, Scale, ScrolledWindow, Switch,
    ToggleButton, glib
};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use libadwaita::{
    ActionRow, Application, ApplicationWindow, PreferencesGroup, ViewStack, prelude::*
};
use logi_mx_daemon::ipc::{
    self, BTN_BIT_BACK, BTN_BIT_DPI, BTN_BIT_FORWARD, BTN_BIT_GESTURE, BTN_BIT_LEFT,
    BTN_BIT_MIDDLE, BTN_BIT_RIGHT, LiveFrame, Response
};
use logi_mx_driver::devices::control_id;

use self::{
    buttons::buttons_page,
    device::{device_page, pointer_page},
    diagnostics::diagnostics_page,
    diagram::{attach_draw_callback, attach_leaders_draw, build_button_masks, to_layout},
    gestures::gestures_page,
    pie::pie_page,
    toasts::Toasts,
    wheel::wheel_page
};
use crate::snapshot::{DeviceSnapshot, collect_device_snapshot};

/// Width of the settings panel in pixels.
const PANEL_WIDTH: i32 = 380;

/// Vertical gap between the sections of a settings page.
const SECTION_SPACING: i32 = 18;

/// Right gutter of a page column: the overlay scrollbar plus a visible gap
/// between it and the tiles.
const SCROLLBAR_GUTTER: i32 = 20;
/// Margin between the HUD window and the screen edges in pixels.
const HUD_MARGIN: i32 = 80;
/// Padding added to thin hotspots so they remain easy to click.
const HIT_PADDING: f64 = 2.5;
/// Minimum hotspot width before padding is applied, in layout units.
const THIN_HOTSPOT: f64 = 6.0;
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

    /// The same backdrop with compositor blur dropped, used when the runtime
    /// GTK cannot request compositor blur regions.
    const fn without_blur(self) -> Self {
        match self {
            Self::Blur => Self::None,
            Self::BlurDim => Self::Dim,
            other => other
        }
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

/// Whether the runtime GTK translates the HUD's `backdrop-filter` into a
/// real compositor blur region (the `ext-background-effect` protocol added
/// in GTK 4.24).
fn blur_supported() -> bool {
    gtk4::check_version(4, 24, 0).is_none()
}

/// Reads the persisted backdrop mode, defaulting to [`BgMode::Dim`].
///
/// Blur modes fall back to their unblurred equivalents when the runtime GTK
/// is older than 4.24, where `backdrop-filter` paints only a faint tint.
fn load_background() -> BgMode {
    let mode = background_path()
        .and_then(|path| fs::read_to_string(path).ok())
        .map_or_else(BgMode::default, |token| BgMode::from_token(token.trim()));
    if blur_supported() {
        mode
    } else {
        mode.without_blur()
    }
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
pub(super) enum MouseButton {
    Left,
    Right,
    Wheel,
    DpiShift,
    Back,
    Forward,
    Gesture
}

impl MouseButton {
    /// Every button, in diagram listing order.
    pub(super) const ALL: [Self; 7] = [
        Self::Left,
        Self::Right,
        Self::Wheel,
        Self::DpiShift,
        Self::Back,
        Self::Forward,
        Self::Gesture
    ];

    /// HID++ reprogrammable control ID of this button.
    pub(super) const fn cid(self) -> u16 {
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

    /// Human-readable button name shown on tiles and rows.
    pub(super) const fn label(self) -> &'static str {
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
    pub(super) const fn anchor_id(self) -> &'static str {
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
    pub(super) const fn element_id(self) -> &'static str {
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

    /// Live telemetry bit for this button.
    pub(super) const fn live_bit(self) -> u32 {
        match self {
            Self::Left => BTN_BIT_LEFT,
            Self::Right => BTN_BIT_RIGHT,
            Self::Wheel => BTN_BIT_MIDDLE,
            Self::Back => BTN_BIT_BACK,
            Self::Forward => BTN_BIT_FORWARD,
            Self::Gesture => BTN_BIT_GESTURE,
            Self::DpiShift => BTN_BIT_DPI
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

    /// Folds one main-context live update: the newest frame, the stream state
    /// and the tap flashes collected between two wakes.
    ///
    /// A dropped stream clears the pressed-button mask so a button cannot
    /// stay tinted on a stale mask while no telemetry arrives.
    fn fold(&mut self, frame: Option<LiveFrame>, connected: bool, flashed: u32) {
        if let Some(frame) = frame {
            self.apply(&frame);
        }
        self.connected = connected;
        if !connected {
            self.buttons = 0;
        }
        self.flash |= flashed;
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

/// Interaction state shared by the diagram, the pages and the live pump:
/// which button is focused and what assignments the user has made.
#[derive(Default)]
pub(super) struct HudState {
    selected:    Option<MouseButton>,
    hover:       Option<MouseButton>,
    assignments: HashMap<MouseButton, String>,
    live:        LiveVisual,
    bg:          BgMode
}

/// One button tile: the card plus the dropdown assigning its behaviour.
#[derive(Clone)]
pub(super) struct Tile {
    child:    FlowBoxChild,
    dropdown: DropDown,
    /// Last remap index confirmed for this tile: `0` until the background
    /// loader or the user sets one.
    applied:  Rc<Cell<u32>>
}

/// Shared handle to the button tiles of the open window, keyed by button.
pub(super) type RowMap = Rc<RefCell<HashMap<MouseButton, Tile>>>;

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
    pub(super) static ROWS_REGISTRY: RefCell<Option<RowMap>> = const { RefCell::new(None) };

    /// Full-window drawing surface carrying the leader lines from the diagram
    /// to the settings rows; cleared when the HUD closes.
    static LEADERS_AREA: RefCell<Option<gtk4::DrawingArea>> = const { RefCell::new(None) };
}

/// Schedules a repaint of the leader-line overlay.
pub(super) fn queue_leaders() {
    LEADERS_AREA.with(|area| {
        if let Some(area) = area.borrow().as_ref() {
            area.queue_draw();
        }
    });
}

/// Mirrors the focused button onto the list rows: hover wins over the
/// persistent selection, so both halves of the HUD always agree.
pub(super) fn sync_row_focus(state: &HudState) {
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
    let toasts = Toasts::new();
    let panel = create_panel(&state, &drawing, &window, &widgets, &toasts);

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
        buttons::clear_remap_hook();
        diagnostics::clear_diag_hook();
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
/// so the first frame never waits on device I/O. The snapshot-seeded
/// controls and their Apply buttons start insensitive and are enabled only
/// once real values arrive.
#[derive(Default)]
pub(super) struct PanelWidgets {
    /// Model row on the device page.
    model_row:         Option<ActionRow>,
    /// Battery row on the device page.
    battery_row:       Option<ActionRow>,
    /// Header chip carrying the live connection, battery and DPI summary.
    status_chip:       Option<Label>,
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
    /// DPI Apply button, kept insensitive until the snapshot provides a real
    /// DPI value.
    dpi_apply:         Option<Button>,
    /// `SmartShift` Apply button, kept insensitive until the snapshot seeds
    /// the `SmartShift` controls.
    smartshift_apply:  Option<Button>,
    /// Scrolling Apply button, kept insensitive until the snapshot seeds the
    /// scroll switches.
    scroll_apply:      Option<Button>,
    /// DPI carried for the header chip so a battery refresh can repaint the
    /// chip without losing the DPI part.
    chip_dpi:          Option<u16>,
    /// Set once a snapshot (or its failure) has been applied; later
    /// deliveries never overwrite values the user may have edited.
    applied:           bool
}

/// Paints a fresh battery reading onto the battery row and the header status
/// chip so the panel never shows two different levels at once.
///
/// The chip keeps the connection wording and appends the DPI part recorded
/// by the last snapshot.
pub(super) fn apply_battery(widgets: &PanelWidgets, battery: &str) {
    if let Some(row) = &widgets.battery_row {
        row.set_subtitle(battery);
    }
    if let Some(chip) = &widgets.status_chip {
        let mut text = format!("Connected · {battery}");
        if let Some(dpi) = widgets.chip_dpi {
            text = format!("{text} · {dpi} DPI");
        }
        chip.set_text(&text);
        chip.remove_css_class("hud-status-bad");
        chip.add_css_class("hud-status-ok");
    }
}

/// Fills the registered panel widgets from a device snapshot.
///
/// The snapshot-seeded controls start insensitive so their placeholder
/// values can never be edited or applied before the device answers; a
/// successful snapshot fills them in and enables them. `None` marks the
/// device unreachable and leaves the controls at their defaults with
/// explicit "unavailable" subtitles.
fn apply_snapshot(widgets: &Rc<RefCell<PanelWidgets>>, snapshot: Option<&DeviceSnapshot>) {
    {
        let mut w = widgets.borrow_mut();
        if w.applied {
            return;
        }
        w.applied = true;
        w.chip_dpi = snapshot.and_then(|snapshot| snapshot.dpi);
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
        if let Some(chip) = &w.status_chip {
            chip.set_text("Daemon unavailable");
            chip.remove_css_class("hud-status-ok");
            chip.add_css_class("hud-status-bad");
        }
        return;
    };
    if let Some(row) = &w.model_row {
        row.set_subtitle(snapshot.name.as_str());
    }
    if let Some(row) = &w.dpi_row {
        let dpi_text = snapshot
            .dpi
            .map_or_else(|| "unknown".to_string(), |dpi| format!("{dpi} DPI"));
        row.set_subtitle(&dpi_text);
    }
    if let Some(scale) = &w.dpi_scale
        && let Some(dpi) = snapshot.dpi
    {
        scale.set_value(f64::from(dpi));
        scale.set_sensitive(true);
    }
    if let Some(button) = &w.dpi_apply {
        button.set_sensitive(snapshot.dpi.is_some());
    }
    if let Some(switch) = &w.smartshift_switch {
        switch.set_active(snapshot.smartshift.enabled);
        switch.set_sensitive(true);
    }
    if let Some(scale) = &w.smartshift_scale {
        scale.set_value(f64::from(snapshot.smartshift.threshold));
        scale.set_sensitive(true);
    }
    if let Some(row) = &w.smartshift_row {
        row.set_subtitle(&format!("Current: {}", snapshot.smartshift.threshold));
    }
    if let Some(button) = &w.smartshift_apply {
        button.set_sensitive(true);
    }
    if let Some(switch) = &w.hires_switch {
        switch.set_active(snapshot.hiresscroll.enabled);
        switch.set_sensitive(true);
    }
    if let Some(switch) = &w.inverted_switch {
        switch.set_active(snapshot.hiresscroll.inverted);
        switch.set_sensitive(true);
    }
    if let Some(button) = &w.scroll_apply {
        button.set_sensitive(true);
    }
    apply_battery(
        &w,
        &snapshot.battery.map_or_else(
            || "unknown".to_string(),
            |b| format!("{}% · {:?}", b.level, b.status)
        )
    );
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
fn create_panel(
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
        content.append(&Image::from_icon_name(icon));
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

/// A page-sized vertical scroller for panel content.
pub(super) fn page_scroll() -> ScrolledWindow {
    let scroll = ScrolledWindow::new();
    scroll.add_css_class("hud-page");
    scroll.set_vexpand(true);
    scroll.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
    scroll
}

/// Vertical column of a settings page: section rhythm plus a right gutter
/// that keeps the overlay scrollbar clear of the tiles.
pub(super) fn page_body() -> GtkBox {
    let box_ = GtkBox::new(Orientation::Vertical, SECTION_SPACING);
    box_.set_margin_end(SCROLLBAR_GUTTER);
    box_
}

/// Turns a daemon reply into a toast message.
pub(super) fn report(response: Option<Response>, toast: &Toasts, success: &str) {
    let message = match response {
        Some(Response::Ok) => success.to_string(),
        Some(Response::Error {
            message
        }) => message,
        _ => "Daemon unavailable".to_string()
    };
    toast.show(&message);
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
        let connected = connected_hook.load(Ordering::Relaxed);
        state_hook.borrow_mut().live.fold(frame, connected, flashed);
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

#[cfg(test)]
mod tests {
    use super::*;

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
    fn disconnect_clears_pressed_buttons() {
        let mut live = LiveVisual::default();
        let pressed = LiveFrame {
            buttons: BTN_BIT_LEFT,
            ..LiveFrame::default()
        };
        live.fold(Some(pressed), true, 0);
        assert_ne!(live.buttons & BTN_BIT_LEFT, 0);

        live.fold(None, false, 0);
        assert!(!live.connected);
        assert_eq!(live.buttons, 0);
    }

    #[test]
    fn disconnected_frame_cannot_restore_pressed_buttons() {
        let mut live = LiveVisual::default();
        let pressed = LiveFrame {
            buttons: BTN_BIT_RIGHT,
            ..LiveFrame::default()
        };
        live.fold(Some(pressed), false, 0);

        assert!(!live.connected);
        assert_eq!(live.buttons, 0);
    }

    #[test]
    fn connected_fold_keeps_buttons_and_flashes() {
        let mut live = LiveVisual::default();
        live.fold(None, true, BTN_BIT_GESTURE);

        assert!(live.connected);
        assert_eq!(live.flash & BTN_BIT_GESTURE, BTN_BIT_GESTURE);
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

    #[test]
    fn without_blur_drops_compositor_modes() {
        assert_eq!(BgMode::Blur.without_blur(), BgMode::None);
        assert_eq!(BgMode::BlurDim.without_blur(), BgMode::Dim);
        assert_eq!(BgMode::None.without_blur(), BgMode::None);
        assert_eq!(BgMode::Dim.without_blur(), BgMode::Dim);
    }
}
