// SPDX-FileCopyrightText: 2025-2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Logitech MX Master 3S configuration UI.
//!
//! Entry point only: checks the graphical session, loads CSS and opens the
//! full-screen HUD ([`hud`]), the only window of this application. A second
//! activation toggles the HUD closed, so the tray item and the configured
//! keybind share one code path.

pub mod hud;
pub mod hyprland;
pub mod snapshot;

use gtk4::{CssProvider, glib, prelude::*};
use libadwaita::Application;

const APP_ID: &str = "com.logitech.mx.configurator";

/// Runs the GTK application, or exits with an error when no graphical session
/// is available.
///
/// The headless `--integrate-hyprland` flag applies the Hyprland integration
/// and exits, which lets installers wire it up without starting the UI.
///
/// # Returns
///
/// The process exit code produced by the GTK main loop.
fn main() -> glib::ExitCode {
    if std::env::args().any(|arg| arg == "--integrate-hyprland") {
        return run_hyprland_integration();
    }

    if std::env::var("WAYLAND_DISPLAY").is_err() && std::env::var("DISPLAY").is_err() {
        eprintln!("No graphical session detected; logi-mx-ui requires Wayland or X11");
        return glib::ExitCode::FAILURE;
    }

    let app = Application::builder().application_id(APP_ID).build();
    app.connect_startup(|_| load_css());
    app.connect_activate(hud::toggle_hud);
    app.run()
}

/// Applies the Hyprland integration from the command line.
fn run_hyprland_integration() -> glib::ExitCode {
    use hyprland::Outcome;

    match hyprland::ensure_integration() {
        Ok(Outcome::Configured {
            path
        }) => {
            println!("Hyprland integration configured: {path}");
            glib::ExitCode::SUCCESS
        }
        Ok(Outcome::AlreadyConfigured {
            path
        }) => {
            println!("Hyprland integration already configured: {path}");
            glib::ExitCode::SUCCESS
        }
        Ok(Outcome::NotHyprland) => {
            eprintln!("Not running under Hyprland; nothing to configure");
            glib::ExitCode::FAILURE
        }
        Ok(Outcome::NoConfig) => {
            eprintln!("Hyprland config not found in ~/.config/hypr");
            glib::ExitCode::FAILURE
        }
        Err(e) => {
            eprintln!("Hyprland integration failed: {e}");
            glib::ExitCode::FAILURE
        }
    }
}

fn load_css() {
    let provider = CssProvider::new();
    provider.load_from_string(
        "
        window.transparent-overlay {
            background: none;
            background-color: transparent;
        }
        window.hud-dim {
            background: rgba(0, 0, 0, 0.45);
        }
        window.hud-tint {
            background: rgba(0, 0, 0, 0.02);
        }
        window.hud-blur {
            backdrop-filter: blur(30px);
        }
        flowbox.hud-tiles {
            padding: 0;
        }
        flowbox.hud-tiles flowboxchild {
            padding: 12px 16px;
            border-radius: 14px;
            background-color: alpha(#ffffff, 0.04);
            border: 1px solid alpha(#ffffff, 0.07);
        }
        flowbox.hud-tiles flowboxchild:hover {
            background-color: alpha(#ffffff, 0.08);
        }
        flowbox.hud-tiles flowboxchild.hud-focus {
            background-color: alpha(#edaa75, 0.16);
            border-color: alpha(#edaa75, 0.45);
        }
        dropdown.hud-assign button {
            min-width: 120px;
        }
        scrolledwindow.hud-page scrollbar {
            background-color: transparent;
            border: none;
            padding: 0;
        }
        scrolledwindow.hud-page scrollbar slider {
            min-width: 6px;
            min-height: 6px;
            margin: 2px 4px;
            border: none;
            border-radius: 99px;
            background-color: alpha(#ffffff, 0.16);
        }
        scrolledwindow.hud-page scrollbar slider:hover {
            background-color: alpha(#ffffff, 0.32);
        }
        scrolledwindow.hud-page undershoot,
        scrolledwindow.hud-page overshoot {
            background-color: transparent;
            background-image: none;
            box-shadow: none;
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
