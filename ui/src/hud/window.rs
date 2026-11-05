// SPDX-FileCopyrightText: 2025-2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! HUD window lifecycle: construction, layer-shell setup, snapshot loading
//! and pointer hit testing over the diagram.

use std::{
    cell::{Cell, OnceCell, RefCell},
    rc::Rc,
    sync::{Arc, Mutex, atomic::Ordering},
    thread,
    time::Duration
};

use gtk4::{
    Box as GtkBox, EventControllerKey, EventControllerMotion, GestureClick, Orientation, glib
};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use libadwaita::{Application, ApplicationWindow, prelude::*};

use super::{
    ACTIVE_WINDOW, HudState, LEADERS_AREA, LIVE_HOOK, LIVE_STOP, MouseButton, PanelWidgets,
    ROWS_REGISTRY, SNAPSHOT_HOOK, apply_battery,
    background::{apply_bg_classes, load_background},
    buttons, diagnostics,
    diagram::{attach_draw_callback, attach_leaders_draw, build_button_masks, to_layout},
    live::start_live,
    panel::create_panel,
    sync_row_focus,
    toasts::Toasts
};
use crate::snapshot::{DeviceSnapshot, collect_device_snapshot};

const HUD_MARGIN: i32 = 80;

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
        next_snapshot_generation();
        buttons::clear_remap_hook();
        diagnostics::clear_diag_hook();
        ROWS_REGISTRY.with(|rows| *rows.borrow_mut() = None);
        LEADERS_AREA.with(|area| *area.borrow_mut() = None);
        ACTIVE_WINDOW.with(|cell| *cell.borrow_mut() = None);
        glib::Propagation::Proceed
    });

    window.present();
    let generation = next_snapshot_generation();
    spawn_snapshot_loader(&widgets, generation);
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

/// Repaints the header chip from the recorded parts: connection wording,
/// the last battery reading and the last known DPI, each present only when
/// the daemon has supplied it.
pub(super) fn repaint_chip(widgets: &PanelWidgets) {
    let Some(chip) = &widgets.status_chip else {
        return;
    };
    let mut parts: Vec<String> = vec!["Connected".to_string()];
    if let Some(battery) = &widgets.chip_battery {
        parts.push(battery.clone());
    }
    if let Some(dpi) = widgets.chip_dpi {
        parts.push(format!("{dpi} DPI"));
    }
    chip.set_text(&parts.join(" · "));
    chip.remove_css_class("hud-status-bad");
    chip.add_css_class("hud-status-ok");
}

/// Fills the registered panel widgets from a device snapshot.
///
/// The snapshot-seeded controls start insensitive so their placeholder
/// values can never be edited or applied before the device answers; a
/// successful snapshot fills them in and enables them. `None` marks the
/// device unreachable: the controls stay at their defaults with explicit
/// "unavailable" subtitles, and because the snapshot counts as unapplied
/// the loader keeps retrying until the daemon answers.
fn apply_snapshot(widgets: &Rc<RefCell<PanelWidgets>>, snapshot: Option<&DeviceSnapshot>) {
    let mut w = widgets.borrow_mut();
    if w.applied {
        return;
    }
    w.chip_dpi = snapshot.and_then(|snapshot| snapshot.dpi);
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
    w.applied = true;
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
    let battery = snapshot.battery.map_or_else(
        || "unknown".to_string(),
        |b| format!("{}% · {:?}", b.level, b.status)
    );
    apply_battery(&mut w, &battery);
}

/// Pause before re-reading a device snapshot that failed: the daemon may
/// still be starting or the mouse may be out of radio range.
const SNAPSHOT_RETRY: Duration = Duration::from_secs(5);

thread_local! {
    /// Generation token of the open HUD window, bumped whenever a window is
    /// built or closed so a stale snapshot loader can never re-arm against a
    /// window that is gone or has been replaced.
    static SNAPSHOT_GENERATION: Cell<u64> = const { Cell::new(0) };
}

/// Retires the previous window's snapshot loader and returns the new
/// generation token. Every loader — hook and retry alike — stops as soon as
/// the token it captured no longer matches the current one.
fn next_snapshot_generation() -> u64 {
    SNAPSHOT_GENERATION.with(|cell| {
        let next = cell.get() + 1;
        cell.set(next);
        next
    })
}

/// Loads the device snapshot off the main thread and applies it to the panel.
///
/// Device I/O can take seconds (daemon round-trips, direct fallback scans),
/// so it must never block the first frame. The worker parks the result in a
/// shared slot and wakes the main context, where the registered hook applies
/// it; a failed load marks the device unavailable and re-arms this loader
/// after [`SNAPSHOT_RETRY`], so the panel recovers on its own once the
/// daemon answers again. A loader carrying a stale `generation` — the window
/// closed or a newer HUD took over while its thread was reading — exits
/// without touching the hooks or re-arming.
fn spawn_snapshot_loader(widgets: &Rc<RefCell<PanelWidgets>>, generation: u64) {
    if SNAPSHOT_GENERATION.with(Cell::get) != generation {
        return;
    }
    let slot = Arc::new(Mutex::new(None::<Option<DeviceSnapshot>>));
    let slot_thread = Arc::clone(&slot);
    let slot_hook = Arc::clone(&slot);
    let widgets_hook = Rc::clone(widgets);
    let hook: Rc<dyn Fn()> = Rc::new(move || {
        if SNAPSHOT_GENERATION.with(Cell::get) != generation {
            return;
        }
        let snapshot = slot_hook.lock().ok().and_then(|mut guard| guard.take());
        if let Some(snapshot) = snapshot {
            apply_snapshot(&widgets_hook, snapshot.as_ref());
            if !widgets_hook.borrow().applied {
                let widgets_retry = Rc::clone(&widgets_hook);
                glib::timeout_add_local_once(SNAPSHOT_RETRY, move || {
                    spawn_snapshot_loader(&widgets_retry, generation);
                });
            }
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
        let widgets_retry = Rc::clone(widgets);
        glib::timeout_add_local_once(SNAPSHOT_RETRY, move || {
            spawn_snapshot_loader(&widgets_retry, generation);
        });
    }
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
