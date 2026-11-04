// SPDX-FileCopyrightText: 2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! `logi-mx-overlay` entry point: one transparent layer-shell window per
//! monitor, kept mapped so the pie appears with zero latency.
//!
//! The windows never take focus (`KeyboardMode::None`) and carry an empty
//! input region, so pointer events fall through to the desktop and the
//! overlay is invisible to the compositor until the daemon streams a
//! `Charge`. Decoded events reach the main context through the [`stream`]
//! pump and a coalesced wake, where the scene is folded and every surface
//! is queued for repaint. While a charge fills or the menu blooms, a short
//! 60 fps timer keeps repainting until the animation window passes.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering}
    },
    thread_local,
    time::{Duration, Instant}
};

use gtk4::{
    Application, ApplicationWindow, CssProvider, DrawingArea, cairo::Region, gdk, glib,
    prelude::*, style_context_add_provider_for_display
};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use logi_mx_daemon::pie::{HOLD_DELAY, PieEvent, direction_angle};
use logi_mx_driver::devices::GestureDirection;
use logi_mx_overlay::{
    draw::{self, Motion},
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
/// Duration of the open bloom animation.
const OPEN_ANIM: Duration = Duration::from_millis(190);
/// Duration of the close fade-out, shorter than the entrance by design.
const EXIT_ANIM: Duration = Duration::from_millis(120);
/// Repaint cadence while an animation runs (~60 fps).
const ANIM_TICK: Duration = Duration::from_millis(16);

thread_local! {
    /// Main-thread callback that drains the event queue and repaints the
    /// surfaces; installed once the windows exist.
    static PIE_HOOK: RefCell<Option<Rc<dyn Fn()>>> = const { RefCell::new(None) };
}

/// Duration of the highlight slide between sectors.
const HL_ANIM: Duration = Duration::from_millis(120);
/// Exponential smoothing time constant of the aim needle, seconds.
const NEEDLE_TAU: f64 = 0.04;

/// Renderer state shared by the draw callbacks and the wake hook.
#[derive(Default)]
struct Stage {
    scene:          PieScene,
    monitors:       Vec<MonitorGeom>,
    charge_started: Option<Instant>,
    open_started:   Option<Instant>,
    fading:         Option<(PieScene, Instant)>,
    needle:         Option<(f64, Instant)>,
    highlight:      Option<HlAnim>,
    last_hl:        Option<GestureDirection>
}

/// One slide of the highlight wedge between sector centres.
#[derive(Debug, Clone, Copy)]
struct HlAnim {
    from:    Option<f64>,
    to:      Option<GestureDirection>,
    started: Instant
}

impl Stage {
    /// Smoothed needle and highlight positions for this frame: the needle
    /// glides exponentially towards the exact aim, the highlight wedge
    /// slides between sector centres over [`HL_ANIM`].
    fn motion(&mut self) -> Motion {
        let now = Instant::now();
        let target = self
            .scene
            .aim
            .or_else(|| self.scene.highlight.and_then(direction_angle));
        let needle = match (self.needle, target) {
            (Some((current, previous)), Some(target)) => {
                let dt = now.duration_since(previous).as_secs_f64().clamp(0.0, 0.1);
                let step = 1.0 - (-dt / NEEDLE_TAU).exp();
                let next = wrap_diff(target - current).mul_add(step, current);
                self.needle = Some((next, now));
                Some(next)
            }
            (None, Some(target)) => {
                self.needle = Some((target, now));
                Some(target)
            }
            (held, None) => held.map(|(angle, _)| angle)
        };
        let highlight = match self.highlight {
            Some(anim) => {
                let t = ease_out(fraction(anim.started.elapsed(), HL_ANIM));
                let to = anim.to.and_then(direction_angle);
                let wedge = match (anim.from, to) {
                    (Some(from), Some(to)) => Some((wrap_diff(to - from).mul_add(t, from), 1.0)),
                    (None, Some(to)) => Some((to, t)),
                    (Some(from), None) => Some((from, 1.0 - t)),
                    (None, None) => None
                };
                if anim.started.elapsed() >= HL_ANIM {
                    self.highlight = None;
                }
                wedge.or_else(|| to.map(|angle| (angle, 1.0)))
            }
            None => self
                .scene
                .highlight
                .and_then(direction_angle)
                .map(|angle| (angle, 1.0))
        };
        Motion {
            needle,
            highlight
        }
    }

    /// The angle the highlight wedge currently paints, mid-slide if one runs.
    fn current_hl_angle(&self) -> Option<f64> {
        match self.highlight {
            Some(anim) => {
                let t = ease_out(fraction(anim.started.elapsed(), HL_ANIM));
                let to = anim.to.and_then(direction_angle)?;
                Some(
                    anim.from
                        .map_or(to, |from| wrap_diff(to - from).mul_add(t, from))
                )
            }
            None => self.scene.highlight.and_then(direction_angle)
        }
    }
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
        scene:          PieScene::default(),
        monitors:       monitors.iter().map(|(_, geom)| *geom).collect(),
        charge_started: None,
        open_started:   None,
        fading:         None,
        needle:         None,
        highlight:      None,
        last_hl:        None
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
    window.set_default_size(geom.width, geom.height);

    let area = DrawingArea::builder().hexpand(true).vexpand(true).build();
    area.set_can_target(false);
    let draw_stage = Rc::clone(stage);
    area.set_draw_func(move |_area, cr, _width, _height| {
        let mut stage = draw_stage.borrow_mut();
        if stage.scene.is_active() {
            let progress = if stage.scene.charging {
                stage
                    .charge_started
                    .map_or(0.0, |start| fraction(start.elapsed(), HOLD_DELAY))
            } else {
                stage
                    .open_started
                    .map_or(1.0, |start| fraction(start.elapsed(), OPEN_ANIM))
            };
            let motion = stage.motion();
            draw::render(cr, index, &stage.scene, progress, &motion);
        } else if let Some((snapshot, started)) = stage.fading.clone() {
            let progress = fraction(started.elapsed(), EXIT_ANIM);
            if progress < 1.0 {
                let motion = stage.motion();
                draw::render_exit(cr, index, &snapshot, progress, &motion);
            }
        }
    });
    window.set_child(Some(&area));
    window.present();
    area
}

/// Enumerates the monitors with their logical geometries.
///
/// GDK reports logical geometry and an integer scale; Hyprland anchors live
/// in the same logical space, so the geometry is taken as reported and the
/// scale is only carried along for painting hints.
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
                x: geometry.x(),
                y: geometry.y(),
                width: geometry.width(),
                height: geometry.height(),
                scale
            }
        ));
    }
    out
}

/// Installs the main-context hook that folds queued events into the scene.
///
/// A dropped stream while a pie is open (daemon restart mid-hold) clears the
/// scene, so a stale ring never lingers on screen. While the charge fills or
/// the menu blooms, the hook also arms a short repaint timer.
fn install_hook(
    stage: Rc<RefCell<Stage>>,
    queue: Arc<Mutex<Vec<PieEvent>>>,
    connected: Arc<AtomicBool>,
    areas: Rc<Vec<DrawingArea>>
) {
    let anim_running = Rc::new(Cell::new(false));
    let hook: Rc<dyn Fn()> = Rc::new(move || {
        let events: Vec<PieEvent> = match queue.lock() {
            Ok(mut guard) => std::mem::take(&mut *guard),
            Err(poisoned) => std::mem::take(&mut *poisoned.into_inner())
        };
        let changed = !events.is_empty();
        {
            let now = Instant::now();
            let stage_ref = &mut *stage.borrow_mut();
            let previous_hl = stage_ref.current_hl_angle();
            let Stage {
                scene,
                monitors,
                charge_started,
                open_started,
                fading,
                needle,
                highlight,
                last_hl
            } = stage_ref;
            for event in &events {
                match event {
                    PieEvent::Charge {
                        ..
                    } => {
                        *charge_started = Some(now);
                        *fading = None;
                        *needle = None;
                        *highlight = None;
                        *last_hl = None;
                    }
                    PieEvent::Open {
                        ..
                    } => {
                        *charge_started = None;
                        *open_started = Some(now);
                        *fading = None;
                        *needle = None;
                        *highlight = None;
                        *last_hl = None;
                    }
                    PieEvent::Close {
                        ..
                    } => {
                        if scene.is_active() && !scene.charging {
                            *fading = Some((scene.clone(), now));
                        }
                        *charge_started = None;
                        *open_started = None;
                    }
                    PieEvent::Highlight {
                        ..
                    }
                    | PieEvent::Aim {
                        ..
                    } => {}
                }
                scene.apply(event, monitors);
            }
            if scene.highlight != *last_hl {
                *highlight = Some(HlAnim {
                    from:    previous_hl,
                    to:      scene.highlight,
                    started: now
                });
                *last_hl = scene.highlight;
            }
            if !connected.load(Ordering::Relaxed) && scene.is_active() {
                scene.reset();
                *fading = None;
                *charge_started = None;
                *open_started = None;
                *needle = None;
                *highlight = None;
                *last_hl = None;
            }
        }
        if changed {
            for area in areas.iter() {
                area.queue_draw();
            }
        }
        arm_animation(&stage, &areas, &anim_running);
    });
    PIE_HOOK.with(|cell| *cell.borrow_mut() = Some(hook));
}

/// Starts the repaint timer while a charge fills or a bloom runs, if it is
/// not running yet; the timer removes itself once both windows have passed.
fn arm_animation(
    stage: &Rc<RefCell<Stage>>,
    areas: &Rc<Vec<DrawingArea>>,
    running: &Rc<Cell<bool>>
) {
    if running.get() {
        return;
    }
    running.set(true);
    let stage_tick = Rc::clone(stage);
    let areas_tick = Rc::clone(areas);
    let flag = Rc::clone(running);
    glib::timeout_add_local(ANIM_TICK, move || {
        let alive = {
            let mut st = stage_tick.borrow_mut();
            let fade_alive = st
                .fading
                .as_ref()
                .is_some_and(|(_, started)| started.elapsed() < EXIT_ANIM);
            if !fade_alive {
                st.fading = None;
            }
            st.scene.is_active() || fade_alive
        };
        if !alive {
            flag.set(false);
            for area in areas_tick.iter() {
                area.queue_draw();
            }
            return glib::ControlFlow::Break;
        }
        for area in areas_tick.iter() {
            area.queue_draw();
        }
        glib::ControlFlow::Continue
    });
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

/// Linear `0..=1` fraction of `elapsed` inside `total`.
fn fraction(elapsed: Duration, total: Duration) -> f64 {
    if total.is_zero() {
        return 1.0;
    }
    (elapsed.as_secs_f64() / total.as_secs_f64()).clamp(0.0, 1.0)
}

/// Cubic ease-out, fast start and a soft landing.
fn ease_out(progress: f64) -> f64 {
    let inverse = 1.0 - progress;
    (inverse * inverse).mul_add(-inverse, 1.0)
}

/// Wraps an angle difference into `-π..=π`, the shorter way around.
fn wrap_diff(delta: f64) -> f64 {
    (delta + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) - std::f64::consts::PI
}
