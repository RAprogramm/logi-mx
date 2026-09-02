// SPDX-FileCopyrightText: 2025 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

use std::io::Write as IoWrite;

use clap::{Parser, Subcommand};
use logi_mx_driver::prelude::*;
use masterror::prelude::*;
use tracing::info;
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

#[derive(Parser)]
#[command(name = "logi-mx")]
#[command(about = "Logitech MX series mouse configuration tool", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands
}

#[derive(Subcommand)]
enum Commands {
    Info,

    Battery,

    Hosts,

    Buttons,

    Set {
        #[command(subcommand)]
        setting: SetCommands
    },

    Config {
        #[command(subcommand)]
        action: ConfigCommands
    }
}

#[derive(Subcommand, Clone, Copy)]
enum SetCommands {
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
    }
}

#[derive(Subcommand)]
enum ConfigCommands {
    Show,

    Edit,

    Export { path: String },

    Import { path: String }
}

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
        Commands::Buttons => cmd_buttons(),
        Commands::Set {
            setting
        } => cmd_set(setting),
        Commands::Config {
            action
        } => cmd_config(action)
    }
}

fn cmd_info() -> Result<()> {
    info!("Opening device...");

    let mut device = MxMaster3s::open_bolt_receiver_discovered()?;

    let name = device.device_name()?;
    let dpi = device.dpi()?;
    let smartshift = device.smartshift()?;
    let hires = device.hires_scroll()?;

    let stdout = std::io::stdout();
    let mut out = std::io::BufWriter::new(stdout.lock());

    out.write_all(b"Device Information:\n")
        .map_err(|e| AppError::internal("Failed to write output").with_source(e))?;
    writeln!(out, "  Name: {name}")
        .map_err(|e| AppError::internal("Failed to write output").with_source(e))?;
    writeln!(out, "  DPI: {dpi}")
        .map_err(|e| AppError::internal("Failed to write output").with_source(e))?;
    writeln!(
        out,
        "  SmartShift: {} (threshold: {})",
        if smartshift.enabled {
            "enabled"
        } else {
            "disabled"
        },
        smartshift.threshold
    )
    .map_err(|e| AppError::internal("Failed to write output").with_source(e))?;
    writeln!(
        out,
        "  Hi-Res Scroll: {}",
        if hires.enabled { "enabled" } else { "disabled" }
    )
    .map_err(|e| AppError::internal("Failed to write output").with_source(e))?;
    out.flush()
        .map_err(|e| AppError::internal("Failed to flush output").with_source(e))?;

    Ok(())
}

fn cmd_battery() -> Result<()> {
    info!("Checking battery...");

    let mut device = MxMaster3s::open_bolt_receiver_discovered()?;
    let battery = device.battery_info()?;

    let stdout = std::io::stdout();
    let mut out = std::io::BufWriter::new(stdout.lock());

    out.write_all(b"Battery Status:\n")
        .map_err(|e| AppError::internal("Failed to write output").with_source(e))?;
    writeln!(out, "  Level: {}%", battery.level)
        .map_err(|e| AppError::internal("Failed to write output").with_source(e))?;
    writeln!(out, "  Status: {:?}", battery.status)
        .map_err(|e| AppError::internal("Failed to write output").with_source(e))?;
    out.flush()
        .map_err(|e| AppError::internal("Failed to flush output").with_source(e))?;

    Ok(())
}

fn cmd_hosts() -> Result<()> {
    info!("Reading Easy-Switch host info...");

    let mut device = MxMaster3s::open_bolt_receiver_discovered()?;
    let (hosts, current) = device.host_info()?;

    let stdout = std::io::stdout();
    let mut out = std::io::BufWriter::new(stdout.lock());

    out.write_all(b"Easy-Switch Hosts:\n")
        .map_err(|e| AppError::internal("Failed to write output").with_source(e))?;
    writeln!(out, "  Current host: {current} (zero-indexed)")
        .map_err(|e| AppError::internal("Failed to write output").with_source(e))?;
    writeln!(out, "  Supported hosts: {hosts}")
        .map_err(|e| AppError::internal("Failed to write output").with_source(e))?;
    out.flush()
        .map_err(|e| AppError::internal("Failed to flush output").with_source(e))?;

    Ok(())
}

fn cmd_buttons() -> Result<()> {
    info!("Listing reprogrammable controls...");

    let mut device = MxMaster3s::open_bolt_receiver_discovered()?;
    let controls = device.list_reprog_controls()?;

    let stdout = std::io::stdout();
    let mut out = std::io::BufWriter::new(stdout.lock());

    writeln!(out, "Reprogrammable Controls: {}", controls.len())
        .map_err(|e| AppError::internal("Failed to write output").with_source(e))?;

    for control in controls {
        let (divert_flags, remap) = device.control_divert(control.control_id)?;
        let diverted = divert_flags & 0x01 != 0;

        writeln!(
            out,
            "  0x{:04X} {:<18} flags {:#04x} divert {}",
            control.control_id,
            control_id_name(control.control_id),
            control.flags,
            if diverted {
                format!("on (remap {remap:#06x})")
            } else {
                "off".to_string()
            }
        )
        .map_err(|e| AppError::internal("Failed to write output").with_source(e))?;
    }

    out.flush()
        .map_err(|e| AppError::internal("Failed to flush output").with_source(e))?;

    Ok(())
}

fn cmd_set(setting: SetCommands) -> Result<()> {
    let mut device = MxMaster3s::open_bolt_receiver_discovered()?;

    let stdout = std::io::stdout();
    let mut out = std::io::BufWriter::new(stdout.lock());

    match setting {
        SetCommands::Dpi {
            value
        } => {
            info!("Setting DPI to {}...", value);
            device.set_dpi(value)?;
            writeln!(out, "DPI set to {value}")
                .map_err(|e| AppError::internal("Failed to write output").with_source(e))?;
        }
        SetCommands::Smartshift {
            enabled,
            threshold
        } => {
            info!(
                "Configuring SmartShift: enabled={}, threshold={}",
                enabled, threshold
            );
            device.set_smartshift(SmartShiftConfig {
                enabled,
                threshold
            })?;
            writeln!(
                out,
                "SmartShift configured: {} (threshold: {})",
                if enabled { "enabled" } else { "disabled" },
                threshold
            )
            .map_err(|e| AppError::internal("Failed to write output").with_source(e))?;
        }
        SetCommands::Hires {
            enabled,
            inverted
        } => {
            info!(
                "Configuring hi-res scroll: enabled={}, inverted={}",
                enabled, inverted
            );
            device.set_hires_scroll(HiResScrollConfig {
                enabled,
                inverted
            })?;
            writeln!(
                out,
                "Hi-res scroll: {}, inverted: {}",
                if enabled { "enabled" } else { "disabled" },
                if inverted { "yes" } else { "no" }
            )
            .map_err(|e| AppError::internal("Failed to write output").with_source(e))?;
        }
    }

    out.flush()
        .map_err(|e| AppError::internal("Failed to flush output").with_source(e))?;

    Ok(())
}
fn cmd_config(action: ConfigCommands) -> Result<()> {
    let stdout = std::io::stdout();
    let mut out = std::io::BufWriter::new(stdout.lock());

    match action {
        ConfigCommands::Show => {
            let config = load_config()?;
            let toml_str = toml::to_string_pretty(&config)
                .map_err(|e| AppError::internal("Failed to serialize config").with_source(e))?;
            writeln!(out, "{toml_str}")
                .map_err(|e| AppError::internal("Failed to write output").with_source(e))?;
        }
        ConfigCommands::Edit => {
            let config_path = config_path()?;
            writeln!(out, "Config location: {}", config_path.display())
                .map_err(|e| AppError::internal("Failed to write output").with_source(e))?;
            out.write_all(b"Edit the file with your preferred editor\n")
                .map_err(|e| AppError::internal("Failed to write output").with_source(e))?;
        }
        ConfigCommands::Export {
            path
        } => {
            let config = load_config()?;
            let toml_str = toml::to_string_pretty(&config)
                .map_err(|e| AppError::internal("Failed to serialize config").with_source(e))?;
            std::fs::write(&path, toml_str)
                .map_err(|e| AppError::internal("Failed to write config").with_source(e))?;
            writeln!(out, "Config exported to {path}")
                .map_err(|e| AppError::internal("Failed to write output").with_source(e))?;
        }
        ConfigCommands::Import {
            path
        } => {
            let config = load_config_from_path(std::path::Path::new(&path))?;
            save_config(&config)?;

            writeln!(out, "Config imported from {path}")
                .map_err(|e| AppError::internal("Failed to write output").with_source(e))?;
        }
    }

    out.flush()
        .map_err(|e| AppError::internal("Failed to flush output").with_source(e))?;

    Ok(())
}
