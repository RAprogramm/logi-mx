// SPDX-FileCopyrightText: 2025 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Logitech MX Master 3S configuration UI.
//!
//! Entry point only: checks the graphical session, loads CSS, and dispatches to
//! either the settings window ([`settings`]) or the full-screen button-mapping
//! overlay ([`overlay`], enabled with `LOGI_MX_OVERLAY=1`).

pub mod hyprland;
pub mod overlay;
pub mod settings;
pub mod snapshot;

use gtk4::{CssProvider, glib, prelude::*};
use libadwaita::Application;

const APP_ID: &str = "com.logitech.mx.configurator";
/// Distinct application id for the overlay mode. Sharing one id with the
/// settings window would make the second launch delegate activation to the
/// first instance, so the overlay would never appear while the UI is open.
const OVERLAY_APP_ID: &str = "com.logitech.mx.button-mapper";

/// Runs the GTK application, or exits with an error when no graphical session
/// is available.
///
/// # Returns
///
/// The process exit code produced by the GTK main loop.
fn main() -> glib::ExitCode {
    if std::env::var("WAYLAND_DISPLAY").is_err() && std::env::var("DISPLAY").is_err() {
        eprintln!("No graphical session detected; logi-mx-ui requires Wayland or X11");
        return glib::ExitCode::FAILURE;
    }

    let overlay = std::env::var("LOGI_MX_OVERLAY").is_ok();
    let app = Application::builder()
        .application_id(if overlay { OVERLAY_APP_ID } else { APP_ID })
        .build();
    app.connect_startup(|_| load_css());
    if overlay {
        app.connect_activate(overlay::build_overlay);
    } else {
        app.connect_activate(settings::build_ui);
    }
    app.run()
}

/// Loads the application stylesheet into the default display, when one exists.
fn load_css() {
    let provider = CssProvider::new();
    provider.load_from_string(
        "
        .status-banner {
            padding: 6px 12px;
        }
        .settings-page {
            margin-top: 12px;
            margin-bottom: 24px;
        }
        .level-bar {
            min-height: 8px;
        }
        "
    );
    if let Some(display) = gtk4::gdk::Display::default() {
        gtk4::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION
        );
    }
}
