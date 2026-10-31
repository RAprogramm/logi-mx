// SPDX-FileCopyrightText: 2025 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

use std::io::Write as IoWrite;

use clap::{Parser, Subcommand};
use logi_mx_daemon::ipc::{Request, Response, try_request};
use logi_mx_driver::prelude::*;
use masterror::prelude::*;
use tracing::{info, warn};
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
    },

    Autostart {
        #[command(subcommand)]
        action: AutostartCommands
    },

    Daemon {
        #[command(subcommand)]
        action: DaemonCommands
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
    },

    Button {
        /// Control ID, e.g. 0x00C3 for the gesture button
        control_id: u16,

        #[arg(long)]
        divert: bool,

        #[arg(long)]
        raw_xy: bool
    }
}

#[derive(Subcommand)]
enum ConfigCommands {
    Show,

    Edit,

    Export { path: String },

    Import { path: String }
}

#[derive(Subcommand)]
enum AutostartCommands {
    Install,
    Uninstall,
    Status
}

#[derive(Subcommand)]
enum DaemonCommands {
    Status,
    Ping
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
        } => cmd_config(action),
        Commands::Autostart {
            action
        } => cmd_autostart(action),
        Commands::Daemon {
            action
        } => cmd_daemon(action)
    }
}

fn cmd_info() -> Result<()> {
    info!("Requesting device info...");

    if let Some(Response::Info {
        name,
        dpi,
        smartshift,
        hires
    }) = try_request(&Request::Info)
    {
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

        return Ok(());
    }

    warn!("Daemon unavailable, falling back to direct device access");
    cmd_info_direct()
}

fn cmd_info_direct() -> Result<()> {
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

    if let Some(Response::Battery {
        level,
        status
    }) = try_request(&Request::Battery)
    {
        let stdout = std::io::stdout();
        let mut out = std::io::BufWriter::new(stdout.lock());

        out.write_all(b"Battery Status:\n")
            .map_err(|e| AppError::internal("Failed to write output").with_source(e))?;
        writeln!(out, "  Level: {level}%")
            .map_err(|e| AppError::internal("Failed to write output").with_source(e))?;
        writeln!(out, "  Status: {status}")
            .map_err(|e| AppError::internal("Failed to write output").with_source(e))?;
        out.flush()
            .map_err(|e| AppError::internal("Failed to flush output").with_source(e))?;

        return Ok(());
    }

    warn!("Daemon unavailable, falling back to direct device access");
    cmd_battery_direct()
}

fn cmd_battery_direct() -> Result<()> {
    info!("Opening device...");

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

    if let Some(Response::Hosts {
        hosts,
        current
    }) = try_request(&Request::Hosts)
    {
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

        return Ok(());
    }

    warn!("Daemon unavailable, falling back to direct device access");
    cmd_hosts_direct()
}

fn cmd_hosts_direct() -> Result<()> {
    info!("Opening device...");

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

    if let Some(Response::Buttons {
        controls
    }) = try_request(&Request::Buttons)
    {
        let stdout = std::io::stdout();
        let mut out = std::io::BufWriter::new(stdout.lock());

        writeln!(out, "Reprogrammable Controls: {}", controls.len())
            .map_err(|e| AppError::internal("Failed to write output").with_source(e))?;
        for control in controls {
            writeln!(
                out,
                "  0x{:04X} {:<18} flags {:#04x}",
                control.control_id,
                control_id_name(control.control_id),
                control.flags
            )
            .map_err(|e| AppError::internal("Failed to write output").with_source(e))?;
        }
        out.flush()
            .map_err(|e| AppError::internal("Failed to flush output").with_source(e))?;

        return Ok(());
    }

    warn!("Daemon unavailable, falling back to direct device access");
    cmd_buttons_direct()
}

fn cmd_buttons_direct() -> Result<()> {
    info!("Opening device...");

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
    let stdout = std::io::stdout();
    let mut out = std::io::BufWriter::new(stdout.lock());

    match setting {
        SetCommands::Dpi {
            value
        } => {
            info!("Setting DPI to {}...", value);
            match try_request(&Request::SetDpi {
                value
            }) {
                Some(Response::Ok) => {
                    writeln!(out, "DPI set to {value}")
                        .map_err(|e| AppError::internal("Failed to write output").with_source(e))?;
                }
                Some(Response::Error {
                    message
                }) => {
                    return Err(AppError::internal(message));
                }
                _ => {
                    warn!("Daemon unavailable, falling back to direct device access");
                    let mut device = MxMaster3s::open_bolt_receiver_discovered()?;
                    device.set_dpi(value)?;
                    writeln!(out, "DPI set to {value}")
                        .map_err(|e| AppError::internal("Failed to write output").with_source(e))?;
                }
            }
        }
        SetCommands::Smartshift {
            enabled,
            threshold
        } => {
            info!(
                "Configuring SmartShift: enabled={}, threshold={}",
                enabled, threshold
            );
            match try_request(&Request::SetSmartShift {
                enabled,
                threshold
            }) {
                Some(Response::Ok) => {
                    writeln!(
                        out,
                        "SmartShift configured: {} (threshold: {})",
                        if enabled { "enabled" } else { "disabled" },
                        threshold
                    )
                    .map_err(|e| AppError::internal("Failed to write output").with_source(e))?;
                }
                Some(Response::Error {
                    message
                }) => {
                    return Err(AppError::internal(message));
                }
                _ => {
                    warn!("Daemon unavailable, falling back to direct device access");
                    let mut device = MxMaster3s::open_bolt_receiver_discovered()?;
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
            }
        }
        SetCommands::Hires {
            enabled,
            inverted
        } => {
            info!(
                "Configuring hi-res scroll: enabled={}, inverted={}",
                enabled, inverted
            );
            match try_request(&Request::SetHires {
                enabled,
                inverted
            }) {
                Some(Response::Ok) => {
                    writeln!(
                        out,
                        "Hi-res scroll: {}, inverted: {}",
                        if enabled { "enabled" } else { "disabled" },
                        if inverted { "yes" } else { "no" }
                    )
                    .map_err(|e| AppError::internal("Failed to write output").with_source(e))?;
                }
                Some(Response::Error {
                    message
                }) => {
                    return Err(AppError::internal(message));
                }
                _ => {
                    warn!("Daemon unavailable, falling back to direct device access");
                    let mut device = MxMaster3s::open_bolt_receiver_discovered()?;
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
        }
        SetCommands::Button {
            control_id,
            divert,
            raw_xy
        } => {
            info!("Configuring button {control_id:#06x}: divert={divert}, raw_xy={raw_xy}");
            match try_request(&Request::SetButtonDivert {
                control_id,
                divert,
                raw_xy
            }) {
                Some(Response::Ok) => {
                    writeln!(out, "Button {control_id:#06x} divert: {divert}")
                        .map_err(|e| AppError::internal("Failed to write output").with_source(e))?;
                }
                Some(Response::Error {
                    message
                }) => {
                    return Err(AppError::internal(message));
                }
                _ => {
                    warn!("Daemon unavailable, falling back to direct device access");
                    let mut device = MxMaster3s::open_bolt_receiver_discovered()?;
                    device.set_control_divert(control_id, divert, raw_xy)?;
                    writeln!(out, "Button {control_id:#06x} divert: {divert}")
                        .map_err(|e| AppError::internal("Failed to write output").with_source(e))?;
                }
            }
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

fn cmd_autostart(action: AutostartCommands) -> Result<()> {
    let home = std::env::var("HOME")
        .map_err(|e| AppError::internal("HOME not set").with_source(e))?;
    let service_dir = std::path::PathBuf::from(&home)
        .join(".config")
        .join("systemd")
        .join("user");
    let service_path = service_dir.join("logi-mx-daemon.service");
    let service_unit = include_str!("../../logi-mx-daemon.service");

    match action {
        AutostartCommands::Install => {
            std::fs::create_dir_all(&service_dir)
                .map_err(|e| AppError::internal("Failed to create systemd user dir").with_source(e))?;
            std::fs::write(&service_path, service_unit)
                .map_err(|e| AppError::internal("Failed to write service file").with_source(e))?;

            let _ = std::process::Command::new("systemctl")
                .args(["--user", "daemon-reload"])
                .status();
            let status = std::process::Command::new("systemctl")
                .args(["--user", "enable", "--now", "logi-mx-daemon.service"])
                .status()
                .map_err(|e| AppError::internal("Failed to run systemctl").with_source(e))?;
            if !status.success() {
                return Err(AppError::internal("Failed to enable daemon service"));
            }
            println!("Autostart installed and daemon started.");
        }
        AutostartCommands::Uninstall => {
            let _ = std::process::Command::new("systemctl")
                .args(["--user", "disable", "--now", "logi-mx-daemon.service"])
                .status();
            let _ = std::fs::remove_file(&service_path);
            let _ = std::process::Command::new("systemctl")
                .args(["--user", "daemon-reload"])
                .status();
            println!("Autostart removed.");
        }
        AutostartCommands::Status => {
            let status = std::process::Command::new("systemctl")
                .args(["--user", "is-active", "logi-mx-daemon.service"])
                .status()
                .map_err(|e| AppError::internal("Failed to run systemctl").with_source(e))?;
            if status.success() {
                println!("Daemon is active.");
            } else {
                println!("Daemon is not active.");
            }
        }
    }

    Ok(())
}

fn cmd_daemon(action: DaemonCommands) -> Result<()> {
    match action {
        DaemonCommands::Status => {
            if let Some(Response::Pong) = try_request(&Request::Ping) {
                println!("Daemon is running.");
            } else {
                println!("Daemon is not running.");
            }
        }
        DaemonCommands::Ping => {
            if try_request(&Request::Ping).is_some() {
                println!("pong");
            } else {
                println!("no daemon");
            }
        }
    }
    Ok(())
}
