// SPDX-FileCopyrightText: 2025-2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Command-line interface for the Logitech MX Master 3S.
//!
//! Every read/write command first tries the running daemon over IPC and falls
//! back to direct HID++ device access when the daemon is unavailable.

use clap::Parser;
use logi_mx_driver::prelude::*;
use tracing::warn;
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

use crate::{
    args::{Cli, Commands},
    bindings::{cmd_gestures, cmd_keybinds, show_button_config},
    queries::{cmd_battery, cmd_buttons, cmd_hosts, cmd_info},
    service::{cmd_autostart, cmd_config, cmd_daemon},
    settings::cmd_set,
    setup::cmd_setup
};

mod args;
mod bindings;
mod out;
mod queries;
mod service;
mod settings;
mod setup;

fn main() -> Result<()> {
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Info => cmd_info(),
        Commands::Battery => cmd_battery(),
        Commands::Hosts => cmd_hosts(),
        Commands::Buttons {
            config
        } => {
            let listed = cmd_buttons();
            if !config {
                return listed;
            }
            // The daemon answers GetButtons even while the mouse is
            // disconnected, so the stored mappings print regardless of
            // whether the live device listing succeeded.
            let stored = show_button_config();
            match (listed, stored) {
                (Ok(()), stored) => stored,
                (Err(listing), Ok(())) => {
                    warn!("device listing failed: {listing}");
                    Ok(())
                }
                (Err(_), Err(stored)) => Err(stored)
            }
        }
        Commands::Set {
            setting
        } => cmd_set(setting),
        Commands::Config {
            action
        } => cmd_config(action),
        Commands::Autostart {
            action
        } => cmd_autostart(&action),
        Commands::Daemon {
            action
        } => cmd_daemon(&action),
        Commands::Gestures {
            action
        } => cmd_gestures(action.as_ref()),
        Commands::Keybinds => cmd_keybinds(),
        Commands::Setup => cmd_setup()
    }
}
