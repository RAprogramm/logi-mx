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

mod background;
mod buttons;
mod device;
mod diagnostics;
mod diagram;
mod gestures;
mod keybinds;
mod live;
mod panel;
mod pie;
mod toasts;
mod wheel;
mod window;
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
    sync::{Arc, atomic::AtomicBool}
};

use gtk4::{
    Box as GtkBox, Button, DropDown, FlowBoxChild, Label, Orientation, Scale, ScrolledWindow,
    Switch
};
use libadwaita::{ActionRow, ApplicationWindow, prelude::*};
use logi_mx_daemon::ipc::{
    BTN_BIT_BACK, BTN_BIT_DPI, BTN_BIT_FORWARD, BTN_BIT_GESTURE, BTN_BIT_LEFT, BTN_BIT_MIDDLE,
    BTN_BIT_RIGHT, Response
};
use logi_mx_driver::devices::control_id;

pub use self::window::toggle_hud;
use self::{
    background::BgMode, live::LiveVisual, panel::background_section, toasts::Toasts,
    window::repaint_chip
};

/// Width of the settings panel in pixels.
const PANEL_WIDTH: i32 = 380;

/// Vertical gap between the sections of a settings page.
const SECTION_SPACING: i32 = 18;

/// Right gutter of a page column: the overlay scrollbar plus a visible gap
/// between it and the tiles.
const SCROLLBAR_GUTTER: i32 = 20;
/// Padding added to thin hotspots so they remain easy to click.
const HIT_PADDING: f64 = 2.5;
/// Minimum hotspot width before padding is applied, in layout units.
const THIN_HOTSPOT: f64 = 6.0;

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
    /// Battery text last shown on the header chip, kept so a DPI change can
    /// repaint the chip without losing the battery part.
    chip_battery:      Option<String>,
    /// Set once a snapshot (or its failure) has been applied; later
    /// deliveries never overwrite values the user may have edited.
    applied:           bool
}

/// Paints a fresh battery reading onto the battery row and the header status
/// chip so the panel never shows two different levels at once.
pub(super) fn apply_battery(widgets: &mut PanelWidgets, battery: &str) {
    if let Some(row) = &widgets.battery_row {
        row.set_subtitle(battery);
    }
    widgets.chip_battery = Some(battery.to_string());
    repaint_chip(widgets);
}

/// Reflects a successful DPI change on the current-DPI row and the header
/// chip, so the chip never keeps showing the opening snapshot's value after
/// the user applies a new one.
pub(super) fn apply_dpi(widgets: &mut PanelWidgets, dpi: u16) {
    widgets.chip_dpi = Some(dpi);
    if let Some(row) = &widgets.dpi_row {
        row.set_subtitle(&format!("{dpi} DPI"));
    }
    repaint_chip(widgets);
}

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
