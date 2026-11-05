// SPDX-FileCopyrightText: 2025-2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Clap command-line grammar: every subcommand and argument the CLI accepts.

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "logi-mx")]
#[command(about = "Logitech MX series mouse configuration tool", long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands
}

#[derive(Subcommand)]
pub enum Commands {
    Info,

    Battery,

    Hosts,

    Buttons {
        /// Also print the software remap of the thumb buttons, global and
        /// per application.
        #[arg(long)]
        config: bool
    },

    Set {
        #[command(subcommand)]
        setting: SetCommands
    },

    Config {
        #[command(subcommand)]
        action: ConfigCommands
    },

    Autostart {
        #[command(subcommand)]
        action: AutostartCommands
    },

    Daemon {
        #[command(subcommand)]
        action: DaemonCommands
    },

    /// Show or change the gesture bindings recognised by the daemon.
    Gestures {
        #[command(subcommand)]
        action: Option<GestureCommands>
    },

    /// List the active Hyprland keyboard bindings that can be attached to a
    /// gesture or a thumb button remap.
    Keybinds,

    /// Check host permissions and print the commands that fix what is missing.
    Setup
}

#[derive(Subcommand)]
pub enum GestureCommands {
    /// Print the current gesture configuration.
    Show,

    /// Divert the gesture button to the daemon for software recognition.
    Enable,

    /// Return the gesture button to its native firmware behaviour.
    Disable
}

/// Parses a control ID written in decimal or with a `0x` hexadecimal prefix.
fn parse_control_id(value: &str) -> std::result::Result<u16, String> {
    let (radix, digits) = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
        .map_or((10, value), |digits| (16, digits));
    u16::from_str_radix(digits, radix).map_err(|e| format!("invalid control ID {value:?}: {e}"))
}

#[derive(Subcommand, Clone, Copy)]
pub enum SetCommands {
    Dpi {
        value: u16
    },

    Smartshift {
        #[arg(long)]
        enabled: bool,

        #[arg(long, default_value_t = 20)]
        threshold: u8
    },

    Hires {
        #[arg(long)]
        enabled: bool,

        #[arg(long)]
        inverted: bool
    },

    Button {
        /// Control ID, e.g. 0x00C3 for the gesture button
        #[arg(value_parser = parse_control_id)]
        control_id: u16,

        #[arg(long)]
        divert: bool,

        #[arg(long)]
        raw_xy: bool
    },

    Remap {
        /// Control ID to remap, e.g. 0x00C3 for the gesture button
        #[arg(value_parser = parse_control_id)]
        control_id: u16,

        /// Target control ID, or 0 to restore native behaviour
        #[arg(value_parser = parse_control_id)]
        target_cid: u16
    },

    Host {
        /// Zero-based host index
        index: u8
    }
}

#[derive(Subcommand)]
pub enum ConfigCommands {
    Show,

    Edit,

    Export { path: String },

    Import { path: String }
}

#[derive(Subcommand)]
pub enum AutostartCommands {
    Install,
    Uninstall,
    Status
}

#[derive(Subcommand)]
pub enum DaemonCommands {
    Status,
    Ping
}
