// SPDX-FileCopyrightText: 2025 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Full-screen button-mapping overlay drawn over the desktop.
//!
//! The device diagram is the reference MX Master 3S layout SVG rendered with
//! `resvg`; clickable hotspots are aligned with the SVG button geometry and
//! highlighted on top of the image. An assignment panel sits on the right.
//! Activated with `LOGI_MX_OVERLAY=1`.

use std::{
    cell::{OnceCell, RefCell},
    collections::HashMap,
    rc::Rc
};

use gtk4::{Box, Button, EventControllerKey, GestureClick, Label, Orientation, glib, prelude::*};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use libadwaita::{Application, ApplicationWindow, prelude::*};

/// Device layout embedded from `assets/logitech-mx-master-3s.svg`.
const DEVICE_SVG: &str = include_str!("../assets/logitech-mx-master-3s.svg");
/// Width of the device layout coordinate space.
const LAYOUT_W: f64 = 128.32296;
/// Height of the device layout coordinate space.
const LAYOUT_H: f64 = 117.73977;
/// Width the SVG is rasterized to once and cached, in pixels.
const SVG_RENDER_WIDTH: u32 = 1024;
/// Width of the assignment panel in pixels.
const PANEL_WIDTH: i32 = 360;
/// Margin between the overlay window and the screen edges in pixels.
const OVERLAY_MARGIN: i32 = 80;
/// Padding added to thin hotspots so they remain easy to click.
const HIT_PADDING: f64 = 2.5;
/// Minimum hotspot width before padding is applied, in layout units.
const THIN_HOTSPOT: f64 = 6.0;

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
            Self::Gesture => (32.6, 59.5, 1.7, 6.3)
        }
    }

    /// Hit rectangle: the drawn region padded when too thin to click.
    const fn hit_region(self) -> (f64, f64, f64, f64) {
        let (x, y, w, h) = self.region();
        let pad = if w < THIN_HOTSPOT { HIT_PADDING } else { 0.0 };
        (x - pad, y - pad, w + 2.0 * pad, h + 2.0 * pad)
    }

    /// Reports whether a layout-space point falls inside the hit rectangle.
    const fn contains(self, x: f64, y: f64) -> bool {
        let (rx, ry, rw, rh) = self.hit_region();
        x >= rx && x <= rx + rw && y >= ry && y <= ry + rh
    }
}

#[derive(Default)]
struct OverlayState {
    selected:    Option<MouseButton>,
    assignments: HashMap<MouseButton, String>
}

thread_local! {
    /// The overlay window while it is open, so a second activation closes it
    /// instead of stacking another instance.
    static ACTIVE_WINDOW: RefCell<Option<ApplicationWindow>> = const { RefCell::new(None) };
}

/// Opens the overlay, or closes it when it is already open.
///
/// This is the activation entry point: repeated launches (e.g. a Hyprland
/// keybind pressed twice) toggle the overlay instead of stacking windows.
pub fn toggle_overlay(app: &Application) {
    let open_new = ACTIVE_WINDOW.with(|cell| {
        cell.borrow().as_ref().is_none_or(|window| {
            window.close();
            false
        })
    });

    if open_new {
        build_overlay(app);
    }
}

/// Builds and presents the full-screen button-mapping overlay.
fn build_overlay(app: &Application) {
    let window = create_window(app);
    let state = Rc::new(RefCell::new(OverlayState::default()));
    let image = Rc::new(OnceCell::new());
    let drawing = create_drawing_area();
    let (panel, status_label) = create_panel(&state, &drawing, &window);

    attach_hit_testing(&state, &drawing, &status_label);
    attach_draw_callback(&state, &drawing, &image);
    attach_escape_to_close(&window);

    let root = Box::new(Orientation::Horizontal, 0);
    root.add_css_class("background");
    root.append(&drawing);
    root.append(&panel);

    window.set_content(Some(&root));

    ACTIVE_WINDOW.with(|cell| *cell.borrow_mut() = Some(window.clone()));
    window.connect_close_request(|_| {
        ACTIVE_WINDOW.with(|cell| *cell.borrow_mut() = None);
        glib::Propagation::Proceed
    });

    window.present();
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

fn create_window(app: &Application) -> ApplicationWindow {
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
    window.set_margin(Edge::Top, OVERLAY_MARGIN);
    window.set_margin(Edge::Bottom, OVERLAY_MARGIN);
    window.set_margin(Edge::Left, OVERLAY_MARGIN);
    window.set_margin(Edge::Right, OVERLAY_MARGIN);

    window
}

fn create_drawing_area() -> gtk4::DrawingArea {
    let drawing = gtk4::DrawingArea::builder()
        .hexpand(true)
        .vexpand(true)
        .content_width(600)
        .content_height(600)
        .build();
    drawing.add_css_class("card");
    drawing
}

fn create_panel(
    state: &Rc<RefCell<OverlayState>>,
    drawing: &gtk4::DrawingArea,
    window: &ApplicationWindow
) -> (Box, Label) {
    let panel = Box::new(Orientation::Vertical, 12);
    panel.set_margin_top(24);
    panel.set_margin_bottom(24);
    panel.set_margin_start(24);
    panel.set_margin_end(24);
    panel.set_size_request(PANEL_WIDTH, -1);
    panel.add_css_class("card");

    let title = Label::new(Some("Button Mapper"));
    title.add_css_class("title-2");
    panel.append(&title);

    let selected_label = Label::new(Some("Select a button on the device diagram"));
    selected_label.set_wrap(true);
    selected_label.set_xalign(0.0);
    panel.append(&selected_label);

    let actions = [
        "Toggle SmartShift",
        "Keypress: Ctrl+C",
        "Keypress: Ctrl+V",
        "None"
    ];
    for action in actions {
        let button = Button::with_label(action);
        button.add_css_class("pill");
        let state_clone = Rc::clone(state);
        let drawing_clone = drawing.clone();
        let selected_clone = selected_label.clone();
        let action = action.to_string();
        button.connect_clicked(move |_| {
            let mut st = state_clone.borrow_mut();
            if let Some(selected) = st.selected {
                st.assignments.insert(selected, action.clone());
                selected_clone.set_text(&format!("{} assigned to {}", selected.label(), action));
                drawing_clone.queue_draw();
            }
        });
        panel.append(&button);
    }

    let close_btn = Button::with_label("Close");
    close_btn.add_css_class("flat");
    let win_close = window.clone();
    close_btn.connect_clicked(move |_| win_close.close());
    panel.append(&close_btn);

    (panel, selected_label)
}

fn attach_hit_testing(
    state: &Rc<RefCell<OverlayState>>,
    drawing: &gtk4::DrawingArea,
    status_label: &Label
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
    state: &Rc<RefCell<OverlayState>>,
    drawing: &gtk4::DrawingArea,
    image: &Rc<OnceCell<Option<gtk4::cairo::ImageSurface>>>
) {
    let state_draw = Rc::clone(state);
    let image_draw = Rc::clone(image);
    drawing.set_draw_func(move |_, cr, width, height| {
        let st = state_draw.borrow();
        let (scale, offset_x, offset_y) = fit_transform(f64::from(width), f64::from(height));

        let _ = cr.save();
        cr.translate(offset_x, offset_y);
        cr.scale(scale, scale);

        if let Some(surface) = image_draw.get_or_init(render_device_svg).as_ref() {
            let sx = LAYOUT_W / f64::from(surface.width());
            let sy = LAYOUT_H / f64::from(surface.height());
            let _ = cr.save();
            cr.scale(sx, sy);
            let _ = cr.set_source_surface(surface, 0.0, 0.0);
            let _ = cr.paint();
            let _ = cr.restore();
        }

        draw_highlights(&st, cr);
        let _ = cr.restore();
    });
}

/// Rasterizes the embedded device SVG once into a cairo surface.
fn render_device_svg() -> Option<gtk4::cairo::ImageSurface> {
    let tree = usvg::Tree::from_data(DEVICE_SVG.as_bytes(), &usvg::Options::default()).ok()?;
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

fn draw_highlights(st: &OverlayState, cr: &gtk4::cairo::Context) {
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

    if let Some(button) = st.selected {
        draw_leader(cr, button);
    }
}

fn draw_leader(cr: &gtk4::cairo::Context, button: MouseButton) {
    let (x, y, w, h) = button.region();
    let start_y = y + h / 2.0;
    cr.set_source_rgb(0.93, 0.66, 0.46);
    cr.set_line_width(0.4);
    cr.move_to(x + w, start_y);
    cr.line_to(LAYOUT_W, start_y);
    let _ = cr.stroke();
}

fn rounded_rect(
    cr: &gtk4::cairo::Context,
    left: f64,
    top: f64,
    width: f64,
    height: f64,
    radius: f64
) {
    cr.new_sub_path();
    cr.arc(
        left + width - radius,
        top + radius,
        radius,
        -std::f64::consts::FRAC_PI_2,
        0.0
    );
    cr.arc(
        left + width - radius,
        top + height - radius,
        radius,
        0.0,
        std::f64::consts::FRAC_PI_2
    );
    cr.arc(
        left + radius,
        top + height - radius,
        radius,
        std::f64::consts::FRAC_PI_2,
        std::f64::consts::PI
    );
    cr.arc(
        left + radius,
        top + radius,
        radius,
        std::f64::consts::PI,
        3.0 * std::f64::consts::FRAC_PI_2
    );
    cr.close_path();
}
