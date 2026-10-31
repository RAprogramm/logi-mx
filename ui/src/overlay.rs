// SPDX-FileCopyrightText: 2025 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

use std::{cell::RefCell, collections::HashMap, rc::Rc};

use gtk4::{Box, Button, GestureClick, Label, Orientation, prelude::*};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use libadwaita::{Application, ApplicationWindow, prelude::*};

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
    const ALL: [MouseButton; 7] = [
        MouseButton::Left,
        MouseButton::Right,
        MouseButton::Middle,
        MouseButton::Back,
        MouseButton::Forward,
        MouseButton::Gesture,
        MouseButton::ModeShift
    ];

    const fn label(self) -> &'static str {
        match self {
            MouseButton::Left => "Left",
            MouseButton::Right => "Right",
            MouseButton::Middle => "Middle",
            MouseButton::Back => "Back",
            MouseButton::Forward => "Forward",
            MouseButton::Gesture => "Gesture",
            MouseButton::ModeShift => "Mode Shift"
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
            MouseButton::ModeShift => (300.0, 100.0)
        }
    }
}

#[derive(Default)]
struct OverlayState {
    selected:    Option<MouseButton>,
    assignments: HashMap<MouseButton, String>
}

pub(crate) fn build_overlay(app: &Application) {
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
        ("None", "None")
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

        cr.set_source_rgb(0.18, 0.18, 0.22);
        rounded_rect(cr, 120.0, 60.0, 360.0, 480.0, 120.0);
        let _ = cr.fill_preserve();
        cr.set_source_rgb(0.35, 0.35, 0.42);
        cr.set_line_width(3.0);
        let _ = cr.stroke();

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
        std::f64::consts::PI
    );
    cr.arc(
        x + r,
        y + r,
        r,
        std::f64::consts::PI,
        3.0 * std::f64::consts::FRAC_PI_2
    );
    cr.close_path();
}
