// SPDX-FileCopyrightText: 2025 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Full-screen button-mapping overlay drawn over the desktop.
//!
//! Rendered with `gtk4-layer-shell`: a scaled mouse diagram with clickable
//! buttons on the left and an assignment panel on the right. Activated with
//! `LOGI_MX_OVERLAY=1`.

use std::{cell::RefCell, collections::HashMap, rc::Rc};

use gtk4::{Box, Button, GestureClick, Label, Orientation, prelude::*};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use libadwaita::{Application, ApplicationWindow, prelude::*};

/// Logical width of the mouse diagram in drawing units.
const DIAGRAM_SIZE: f64 = 600.0;
/// Radius of a clickable button hotspot in drawing units.
const BUTTON_RADIUS: f64 = 36.0;
/// Hit-test tolerance around a button center in drawing units.
const HIT_TOLERANCE: f64 = 40.0;
/// Width of the assignment panel in pixels.
const PANEL_WIDTH: i32 = 360;
/// Margin between the overlay window and the screen edges in pixels.
const OVERLAY_MARGIN: i32 = 80;

/// Reprogrammable buttons drawn on the mouse diagram.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum MouseButton {
    Left,
    Right,
    Middle,
    Back,
    Forward,
    Gesture,
    ModeShift
}

impl MouseButton {
    const ALL: [Self; 7] = [
        Self::Left,
        Self::Right,
        Self::Middle,
        Self::Back,
        Self::Forward,
        Self::Gesture,
        Self::ModeShift
    ];

    const fn label(self) -> &'static str {
        match self {
            Self::Left => "Left",
            Self::Right => "Right",
            Self::Middle => "Middle",
            Self::Back => "Back",
            Self::Forward => "Forward",
            Self::Gesture => "Gesture",
            Self::ModeShift => "Mode Shift"
        }
    }

    const fn position(self) -> (f64, f64) {
        match self {
            Self::Left => (200.0, 150.0),
            Self::Right => (400.0, 150.0),
            Self::Middle => (300.0, 250.0),
            Self::Back => (180.0, 350.0),
            Self::Forward => (220.0, 350.0),
            Self::Gesture => (180.0, 450.0),
            Self::ModeShift => (300.0, 100.0)
        }
    }
}

#[derive(Default)]
struct OverlayState {
    selected:    Option<MouseButton>,
    assignments: HashMap<MouseButton, String>
}

/// Builds and presents the full-screen button-mapping overlay.
pub fn build_overlay(app: &Application) {
    let window = create_window(app);
    let state = Rc::new(RefCell::new(OverlayState::default()));
    let drawing = create_drawing_area();
    let (panel, status_label) = create_panel(&state, &drawing, &window);

    attach_hit_testing(&state, &drawing, &status_label);
    attach_draw_callback(&state, &drawing);

    let root = Box::new(Orientation::Horizontal, 0);
    root.add_css_class("background");
    root.append(&drawing);
    root.append(&panel);

    window.set_content(Some(&root));
    window.present();
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
    window.set_margin_top(OVERLAY_MARGIN);
    window.set_margin_bottom(OVERLAY_MARGIN);
    window.set_margin_start(OVERLAY_MARGIN);
    window.set_margin_end(OVERLAY_MARGIN);

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

    let selected_label = Label::new(Some("Select a button on the mouse diagram"));
    selected_label.set_wrap(true);
    selected_label.set_xalign(0.0);
    panel.append(&selected_label);

    let actions = [
        ("Toggle SmartShift", "Toggle SmartShift"),
        ("Keypress: Ctrl+C", "Keypress: Ctrl+C"),
        ("Keypress: Ctrl+V", "Keypress: Ctrl+V"),
        ("None", "None")
    ];
    for (label, action) in actions {
        let button = Button::with_label(label);
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
    gesture.connect_released(move |_gesture, _n_press, x, y| {
        let mut st = state_gesture.borrow_mut();
        for button in MouseButton::ALL {
            let (bx, by) = button.position();
            let dx = x - bx;
            let dy = y - by;
            if dx.mul_add(dx, dy * dy) < HIT_TOLERANCE * HIT_TOLERANCE {
                st.selected = Some(button);
                status_gesture.set_text(&format!("Selected: {}", button.label()));
                drawing_gesture.queue_draw();
                return;
            }
        }
    });
    drawing.add_controller(gesture);
}

fn attach_draw_callback(state: &Rc<RefCell<OverlayState>>, drawing: &gtk4::DrawingArea) {
    let state_draw = Rc::clone(state);
    drawing.set_draw_func(move |_, cr, width, height| {
        let st = state_draw.borrow();
        draw_mouse(&st, cr, f64::from(width), f64::from(height));
    });
}

fn draw_mouse(st: &OverlayState, cr: &gtk4::cairo::Context, width: f64, height: f64) {
    let scale_x = width / DIAGRAM_SIZE;
    let scale_y = height / DIAGRAM_SIZE;
    let scale = scale_x.min(scale_y);
    let content = DIAGRAM_SIZE * scale;
    let offset_x = (width - content) / 2.0;
    let offset_y = (height - content) / 2.0;

    let _ = cr.save();
    cr.translate(offset_x, offset_y);
    cr.scale(scale, scale);

    cr.set_source_rgb(0.18, 0.18, 0.22);
    rounded_rect(cr, 120.0, 60.0, 360.0, 480.0, 120.0);
    let _ = cr.fill_preserve();
    cr.set_source_rgb(0.35, 0.35, 0.42);
    cr.set_line_width(3.0);
    let _ = cr.stroke();

    for button in MouseButton::ALL {
        draw_button(st, cr, button);
    }

    if let Some(button) = st.selected {
        let (x, y) = button.position();
        cr.set_source_rgb(0.95, 0.75, 0.65);
        cr.set_line_width(2.0);
        cr.move_to(x + BUTTON_RADIUS, y);
        cr.line_to(DIAGRAM_SIZE, y);
        let _ = cr.stroke();
    }

    let _ = cr.restore();
}

fn draw_button(st: &OverlayState, cr: &gtk4::cairo::Context, button: MouseButton) {
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
    cr.arc(x, y, BUTTON_RADIUS, 0.0, 2.0 * std::f64::consts::PI);
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
