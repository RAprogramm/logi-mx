// SPDX-FileCopyrightText: 2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! `logi-mx-overlay` entry point: one transparent layer-shell window per
//! monitor, kept mapped so the pie appears with zero latency.
//!
//! The windows never take focus (`KeyboardMode::None`) and carry an empty
//! input region, so pointer events fall through to the desktop and the
//! overlay is invisible to the compositor until the daemon streams an
//! `Open`. Decoded events reach the main context through the [`stream`]
//! pump and a coalesced wake, where the scene is folded and every surface
//! is queued for repaint.

use std::{
    cell::RefCell,
    rc::Rc,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering}
    },
    thread_local
};

use gtk4::{
    Application, ApplicationWindow, CssProvider, DrawingArea, cairo::Region, gdk, glib,
    prelude::*, style_context_add_provider_for_display
};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use logi_mx_daemon::pie::PieEvent;
use logi_mx_overlay::{
    draw,
    scene::{MonitorGeom, PieScene},
    stream
};

/// `GApplication` id; the unique name keeps a second instance from stacking a
/// second set of overlay windows.
const APP_ID: &str = "com.logitech.mx.overlay";
/// Layer-shell namespace advertised to the compositor.
const SHELL_NAMESPACE: &str = "logi-mx-pie";
/// Sheet that makes the full-screen surfaces fully transparent.
const OVERLAY_CSS: &str =
    "window.logi-mx-pie, window.logi-mx-pie > * { background: none transparent; }";

thread_local! {
    /// Main-thread callback that drains the event queue and repaints the
    /// surfaces; installed once the windows exist.
    static PIE_HOOK: RefCell<Option<Rc<dyn Fn()>>> = const { RefCell::new(None) };
}

/// Renderer state shared by the draw callbacks and the wake hook.
#[derive(Default)]
struct Stage {
    scene:    PieScene,
    monitors: Vec<MonitorGeom>
}

/// Runs the overlay application, or exits with an error when no graphical
/// session is available.
///
/// # Returns
///
/// The process exit code produced by the GTK main loop.
fn main() -> glib::ExitCode {
    if std::env::var("WAYLAND_DISPLAY").is_err() && std::env::var("DISPLAY").is_err() {
        eprintln!("No graphical session detected; logi-mx-overlay requires Wayland or X11");
        return glib::ExitCode::FAILURE;
    }

    let app = Application::builder().application_id(APP_ID).build();
    app.connect_startup(load_css);
    app.connect_activate(build_overlay);
    app.run()
}

/// Installs the transparent-background stylesheet on the default display.
fn load_css(_app: &Application) {
    let provider = CssProvider::new();
    provider.load_from_string(OVERLAY_CSS);
    if let Some(display) = gdk::Display::default() {
        style_context_add_provider_for_display(
            &display,
            &provider,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION
        );
    }
}

/// Builds one click-through surface per monitor and starts the stream pump.
///
/// A second activation (a repeated `systemctl restart` racing a live
/// instance) is ignored: the windows are built exactly once per process.
fn build_overlay(app: &Application) {
    if !app.windows().is_empty() {
        return;
    }
    let monitors = monitor_geometries();
    if monitors.is_empty() {
        eprintln!("overlay: the display reported no monitors, nothing to draw on");
    }

    let stage = Rc::new(RefCell::new(Stage {
        scene:    PieScene::default(),
        monitors: monitors.iter().map(|(_, geom)| *geom).collect()
    }));
    let queue = Arc::new(Mutex::new(Vec::<PieEvent>::new()));
    let connected = Arc::new(AtomicBool::new(false));

    let mut areas = Vec::with_capacity(monitors.len());
    for (index, (monitor, geom)) in monitors.iter().enumerate() {
        areas.push(monitor_window(app, monitor, *geom, index, &stage));
    }

    install_hook(
        stage,
        Arc::clone(&queue),
        Arc::clone(&connected),
        Rc::new(areas)
    );
    stream::spawn(Arc::clone(&queue), Arc::clone(&connected), make_wake());
    std::mem::forget(app.hold());
}

/// Creates the layer-shell window and drawing surface for one monitor.
fn monitor_window(
    app: &Application,
    monitor: &gdk::Monitor,
    geom: MonitorGeom,
    index: usize,
    stage: &Rc<RefCell<Stage>>
) -> DrawingArea {
    let window = ApplicationWindow::builder().application(app).build();
    window.add_css_class("logi-mx-pie");
    window.remove_css_class("background");
    window.init_layer_shell();
    window.set_namespace(Some(SHELL_NAMESPACE));
    window.set_layer(Layer::Overlay);
    window.set_exclusive_zone(-1);
    window.set_keyboard_mode(KeyboardMode::None);
    for edge in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
        window.set_anchor(edge, true);
    }
    window.set_monitor(Some(monitor));
    window.connect_realize(|window| {
        if let Some(surface) = window.surface() {
            surface.set_input_region(Some(&Region::create()));
        }
    });
    let scale = geom.scale.max(1);
    window.set_default_size(geom.width / scale, geom.height / scale);

    let area = DrawingArea::builder().hexpand(true).vexpand(true).build();
    area.set_can_target(false);
    let draw_stage = Rc::clone(stage);
    area.set_draw_func(move |_area, cr, _width, _height| {
        let stage = draw_stage.borrow();
        draw::render(cr, index, &stage.scene);
    });
    window.set_child(Some(&area));
    window.present();
    area
}

/// Enumerates the monitors with their physical geometries.
///
/// GDK reports logical geometry and an integer scale; Hyprland anchors are
/// physical, so the overlay multiplies origin and size by the scale to get
/// the same coordinate space the daemon uses.
fn monitor_geometries() -> Vec<(gdk::Monitor, MonitorGeom)> {
    let Some(display) = gdk::Display::default() else {
        return Vec::new();
    };
    let monitors = display.monitors();
    let mut out = Vec::with_capacity(usize::try_from(monitors.n_items()).unwrap_or(0));
    for position in 0..monitors.n_items() {
        let Some(monitor) = monitors
            .item(position)
            .and_then(|item| item.downcast::<gdk::Monitor>().ok())
        else {
            continue;
        };
        let geometry = monitor.geometry();
        let scale = monitor.scale_factor().max(1);
        out.push((
            monitor,
            MonitorGeom {
                x: geometry.x().saturating_mul(scale),
                y: geometry.y().saturating_mul(scale),
                width: geometry.width().saturating_mul(scale),
                height: geometry.height().saturating_mul(scale),
                scale
            }
        ));
    }
    out
}

/// Installs the main-context hook that folds queued events into the scene.
///
/// A dropped stream while a pie is open (daemon restart mid-hold) clears the
/// scene, so a stale ring never lingers on screen.
fn install_hook(
    stage: Rc<RefCell<Stage>>,
    queue: Arc<Mutex<Vec<PieEvent>>>,
    connected: Arc<AtomicBool>,
    areas: Rc<Vec<DrawingArea>>
) {
    let hook: Rc<dyn Fn()> = Rc::new(move || {
        let events: Vec<PieEvent> = match queue.lock() {
            Ok(mut guard) => std::mem::take(&mut *guard),
            Err(poisoned) => std::mem::take(&mut *poisoned.into_inner())
        };
        let mut changed = !events.is_empty();
        {
            let stage_ref = &mut *stage.borrow_mut();
            let Stage {
                scene,
                monitors
            } = stage_ref;
            for event in &events {
                scene.apply(event, monitors);
            }
            if !connected.load(Ordering::Relaxed) && scene.is_active() {
                scene.reset();
                changed = true;
            }
        }
        if changed {
            for area in areas.iter() {
                area.queue_draw();
            }
        }
    });
    PIE_HOOK.with(|cell| *cell.borrow_mut() = Some(hook));
}

/// Builds the coalesced wake callback the pump thread calls.
///
/// Many events between two main-context turns collapse into a single hook
/// run, which drains the whole queue at once.
fn make_wake() -> stream::Wake {
    let context = glib::MainContext::default();
    let pending = Arc::new(AtomicBool::new(false));
    Arc::new(move || {
        let context = context.clone();
        let pending = Arc::clone(&pending);
        if pending.swap(true, Ordering::Relaxed) {
            return;
        }
        context.invoke(move || {
            pending.store(false, Ordering::Relaxed);
            PIE_HOOK.with(|cell| {
                if let Some(hook) = cell.borrow().as_ref() {
                    hook();
                }
            });
        });
    })
}
