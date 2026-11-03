// SPDX-FileCopyrightText: 2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Top-anchored toast strip for the HUD panel.
//!
//! `AdwToastOverlay` pins its toasts to the bottom of the widget and offers no
//! way to move them, while the HUD panel is full height and its bottom edge is
//! covered by content. This module shows a single pill at the top of the panel
//! instead: it slides down, stays for [`TOAST_LIFETIME`], and can be dismissed
//! early with its close button.
//!
//! ```ignore
//! let toasts = Toasts::new();
//! let mut overlay = gtk4::Overlay::new();
//! overlay.set_child(Some(&panel));
//! overlay.add_overlay(&toasts.widget());
//! toasts.show("DPI set to 1600");
//! ```

use std::{cell::RefCell, rc::Rc, time::Duration};

use gtk4::{
    Box as GtkBox, Button, Label, Orientation, Revealer, RevealerTransitionType,
    glib::{self, SourceId},
    prelude::*
};

/// How long a toast stays on screen before it slides away.
const TOAST_LIFETIME: Duration = Duration::from_secs(5);

/// A single notification pill anchored to the top of the panel.
///
/// Cheap to clone: every handle drives the same underlying widgets, so pages
/// can keep their own copy of the handle.
#[derive(Clone)]
pub struct Toasts {
    revealer: Revealer,
    label:    Label,
    timer:    Rc<RefCell<Option<SourceId>>>
}

impl Default for Toasts {
    fn default() -> Self {
        Self::new()
    }
}

impl Toasts {
    /// Builds the strip and its pill widget in the hidden state.
    #[must_use]
    pub fn new() -> Self {
        let revealer = Revealer::new();
        revealer.set_transition_type(RevealerTransitionType::SlideDown);
        revealer.set_halign(gtk4::Align::Center);
        revealer.set_valign(gtk4::Align::Start);
        revealer.set_margin_top(10);

        let pill = GtkBox::new(Orientation::Horizontal, 8);
        pill.add_css_class("hud-toast");

        let label = Label::new(None);
        label.add_css_class("heading");
        label.set_wrap(true);
        label.set_max_width_chars(48);
        label.set_xalign(0.0);

        let close = Button::from_icon_name("window-close-symbolic");
        close.add_css_class("flat");
        close.add_css_class("circular");
        close.set_valign(gtk4::Align::Center);
        close.set_tooltip_text(Some("Dismiss"));

        pill.append(&label);
        pill.append(&close);
        revealer.set_child(Some(&pill));

        let timer = Rc::new(RefCell::new(None::<SourceId>));
        let close_timer = Rc::clone(&timer);
        let close_revealer = revealer.clone();
        close.connect_clicked(move |_| {
            close_revealer.set_reveal_child(false);
            if let Some(id) = close_timer.borrow_mut().take() {
                id.remove();
            }
        });

        Self {
            revealer,
            label,
            timer
        }
    }

    /// The strip widget to add as an overlay child of the panel.
    #[must_use]
    pub fn widget(&self) -> Revealer {
        self.revealer.clone()
    }

    /// Shows `text` at the top of the panel, replacing any visible toast, and
    /// retracts it after [`TOAST_LIFETIME`].
    ///
    /// # Arguments
    ///
    /// * `text` - the message to display.
    pub fn show(&self, text: &str) {
        self.label.set_text(text);
        self.revealer.set_reveal_child(true);
        if let Some(id) = self.timer.borrow_mut().take() {
            id.remove();
        }
        let timer = Rc::clone(&self.timer);
        let revealer = self.revealer.clone();
        let id = glib::timeout_add_local(TOAST_LIFETIME, move || {
            revealer.set_reveal_child(false);
            timer.borrow_mut().take();
            glib::ControlFlow::Break
        });
        *self.timer.borrow_mut() = Some(id);
    }
}
