// SPDX-FileCopyrightText: 2025 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

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

    Set {
        #[command(subcommand)]
        setting: SetCommands
    },

    Config {
        #[command(subcommand)]
        action: ConfigCommands
    }
}

#[derive(Subcommand)]
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

    Scroll {
        #[arg(long)]
        hires: bool,

        #[arg(long)]
        inverted: bool
    },

    ScrollWheel {
        #[arg(long, default_value_t = 1.0)]
        vertical_speed: f32,

        #[arg(long, default_value_t = 1.0)]
        horizontal_speed: f32,

        #[arg(long)]
        smooth: bool
    },

    ThumbWheel {
        #[arg(long, default_value_t = 1.0)]
        speed: f32,

        #[arg(long)]
        smooth: bool
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

    let mut device = MxMaster3s::open_bolt_receiver(2)?;

    let name = device.get_device_name()?;
    let dpi = device.get_dpi()?;
    let smartshift = device.get_smartshift()?;
    let scroll = device.get_hires_scroll()?;
    let scroll_wheel = device.get_scroll_wheel()?;
    let thumbwheel = device.get_thumb_wheel()?;

    println!("Device Information:");
    println!("  Name: {}", name);
    println!("  DPI: {}", dpi);
    println!(
        "  SmartShift: {} (threshold: {})",
        if smartshift.enabled {
            "enabled"
        } else {
            "disabled"
        },
        smartshift.threshold
    );
    println!(
        "  Hi-Res Scroll: {}",
        if scroll.enabled {
            "enabled"
        } else {
            "disabled"
        }
    );
    println!(
        "  Scroll Wheel: vertical speed={}, horizontal speed={}, smooth scrolling {}",
        scroll_wheel.vertical_speed,
        scroll_wheel.horizontal_speed,
        if scroll_wheel.smooth_scrolling {
            "enabled"
        } else {
            "disabled"
        }
    );
    println!(
        "  Thumb Wheel: speed={}, smooth scrolling {}",
        thumbwheel.speed,
        if thumbwheel.smooth_scrolling {
            "enabled"
        } else {
            "disabled"
        }
    );

    Ok(())
}

fn cmd_battery() -> Result<()> {
    info!("Checking battery...");

    let mut device = MxMaster3s::open_bolt_receiver(2)?;
    let battery = device.get_battery_info()?;

    println!("Battery Status:");
    println!("  Level: {}%", battery.level);
    println!("  Status: {:?}", battery.status);

    Ok(())
}

fn cmd_set(setting: SetCommands) -> Result<()> {
    let mut device = MxMaster3s::open_bolt_receiver(2)?;

    match setting {
        SetCommands::Dpi {
            value
        } => {
            info!("Setting DPI to {}...", value);
            device.set_dpi(value)?;
            println!("DPI set to {}", value);
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
            println!(
                "SmartShift configured: {} (threshold: {})",
                if enabled { "enabled" } else { "disabled" },
                threshold
            );
        }
        SetCommands::Scroll {
            hires,
            inverted
        } => {
            info!("Configuring scroll: hires={}, inverted={}", hires, inverted);
            device.set_hires_scroll(HiResScrollConfig {
                enabled: hires,
                inverted
            })?;
            println!(
                "Scroll configured: hi-res {}, inverted {}",
                if hires { "enabled" } else { "disabled" },
                if inverted { "yes" } else { "no" }
            );
        }
        SetCommands::ScrollWheel {
            vertical_speed,
            horizontal_speed,
            smooth
        } => {
            info!(
                "Configuring scroll wheel: vertical={}, horizontal={}, smooth={}",
                vertical_speed, horizontal_speed, smooth
            );
            device.set_scroll_wheel(ScrollWheelConfig {
                vertical_speed,
                horizontal_speed,
                smooth_scrolling: smooth
            })?;
            println!(
                "Scroll wheel configured: vertical speed={}, horizontal speed={}, smooth scrolling {}",
                vertical_speed,
                horizontal_speed,
                if smooth { "enabled" } else { "disabled" }
            );
        }
        SetCommands::ThumbWheel {
            speed,
            smooth
        } => {
            info!(
                "Configuring thumb wheel: speed={}, smooth={}",
                speed, smooth
            );
            device.set_thumb_wheel(ThumbWheelConfig {
                speed,
                smooth_scrolling: smooth
            })?;
            println!(
                "Thumb wheel configured: speed={}, smooth scrolling {}",
                speed,
                if smooth { "enabled" } else { "disabled" }
            );
        }
    }

    Ok(())
}

fn cmd_config(action: ConfigCommands) -> Result<()> {
    match action {
        ConfigCommands::Show => {
            let config = load_config()?;
            let toml_str = toml::to_string_pretty(&config)
                .map_err(|e| AppError::internal("Failed to serialize config").with_source(e))?;
            println!("{}", toml_str);
        }
        ConfigCommands::Edit => {
            let config_path = get_config_path()?;
            println!("Config location: {}", config_path.display());
            println!("Edit the file with your preferred editor");
        }
        ConfigCommands::Export {
            path
        } => {
            let config = load_config()?;
            let toml_str = toml::to_string_pretty(&config)
                .map_err(|e| AppError::internal("Failed to serialize config").with_source(e))?;
            std::fs::write(&path, toml_str)
                .map_err(|e| AppError::internal("Failed to write config").with_source(e))?;
            println!("Config exported to {}", path);
        }
        ConfigCommands::Import {
            path
        } => {
            let config = load_config_from_path(std::path::Path::new(&path))?;
            save_config(&config)?;
            println!("Config imported from {}", path);
        }
    }

    Ok(())
}
