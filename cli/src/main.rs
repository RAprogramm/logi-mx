// SPDX-FileCopyrightText: 2025-2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Command-line interface for the Logitech MX Master 3S.
//!
//! Every read/write command first tries the running daemon over IPC and falls
//! back to direct HID++ device access when the daemon is unavailable.

use std::io::Write as IoWrite;

use clap::{Parser, Subcommand};
use logi_mx_daemon::ipc::{try_request, Request, Response};
use logi_mx_driver::prelude::*;
use masterror::prelude::*;
use tracing::{info, warn};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

#[derive(Parser)]
#[command(name = "logi-mx")]
#[command(about = "Logitech MX series mouse configuration tool", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Info,

    Battery,

    Hosts,

    Buttons,

    Set {
        #[command(subcommand)]
        setting: SetCommands,
    },

    Config {
        #[command(subcommand)]
        action: ConfigCommands,
    },

    Autostart {
        #[command(subcommand)]
        action: AutostartCommands,
    },

    Daemon {
        #[command(subcommand)]
        action: DaemonCommands,
    },
}

#[derive(Subcommand, Clone, Copy)]
enum SetCommands {
    Dpi {
        value: u16,
    },

    Smartshift {
        #[arg(long)]
        enabled: bool,

        #[arg(long, default_value_t = 20)]
        threshold: u8,
    },

    Hires {
        #[arg(long)]
        enabled: bool,

        #[arg(long)]
        inverted: bool,
    },

    Button {
        /// Control ID, e.g. 0x00C3 for the gesture button
        control_id: u16,

        #[arg(long)]
        divert: bool,

        #[arg(long)]
        raw_xy: bool,
    },

    Remap {
        /// Control ID to remap, e.g. 0x00C3 for the gesture button
        control_id: u16,

        /// Target control ID, or 0 to restore native behaviour
        target_cid: u16,
    },

    Host {
        /// Zero-based host index
        index: u8,
    },
}

#[derive(Subcommand)]
enum ConfigCommands {
    Show,

    Edit,

    Export { path: String },

    Import { path: String },
}

#[derive(Subcommand)]
enum AutostartCommands {
    Install,
    Uninstall,
    Status,
}

#[derive(Subcommand)]
enum DaemonCommands {
    Status,
    Ping,
}

/// Buffered stdout writer that maps I/O failures to [`AppError`].
struct Out(std::io::BufWriter<std::io::StdoutLock<'static>>);

impl Out {
    /// Locks stdout and wraps it in a buffered writer.
    fn new() -> Self {
        Self(std::io::BufWriter::new(std::io::stdout().lock()))
    }

    /// Writes one newline-terminated line.
    fn line(&mut self, args: std::fmt::Arguments<'_>) -> Result<()> {
        writeln!(self.0, "{args}")
            .map_err(|e| AppError::internal("Failed to write output").with_source(e))
    }

    /// Flushes buffered bytes to the terminal.
    fn finish(&mut self) -> Result<()> {
        self.0
            .flush()
            .map_err(|e| AppError::internal("Failed to flush output").with_source(e))
    }
}

/// Applies a setting through the daemon, falling back to direct device access.
///
/// # Arguments
///
/// * `request` - IPC request carrying the desired change.
/// * `direct` - closure applying the same change to an opened device.
/// * `message` - confirmation line printed on success.
fn apply_setting(
    request: &Request,
    direct: impl FnOnce(&mut MxMaster3s) -> Result<()>,
    message: std::fmt::Arguments<'_>,
) -> Result<()> {
    match try_request(request) {
        Some(Response::Ok) => {}
        Some(Response::Error { message }) => return Err(AppError::internal(message)),
        _ => {
            warn!("Daemon unavailable, falling back to direct device access");
            let mut device = MxMaster3s::open_bolt_receiver_discovered()?;
            direct(&mut device)?;
        }
    }

    let mut out = Out::new();
    out.line(message)?;
    out.finish()
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
        Commands::Set { setting } => cmd_set(setting),
        Commands::Config { action } => cmd_config(action),
        Commands::Autostart { action } => cmd_autostart(&action),
        Commands::Daemon { action } => cmd_daemon(&action),
    }
}

fn cmd_info() -> Result<()> {
    info!("Requesting device info...");

    if let Some(Response::Info {
        name,
        dpi,
        smartshift,
        hires,
    }) = try_request(&Request::Info)
    {
        let mut out = Out::new();
        render_info(&mut out, &name, dpi, smartshift, hires)?;
        return out.finish();
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

    let mut out = Out::new();
    render_info(&mut out, &name, dpi, smartshift, hires)?;
    out.finish()
}

fn render_info(
    out: &mut Out,
    name: &str,
    dpi: u16,
    smartshift: SmartShiftConfig,
    hires: HiResScrollConfig,
) -> Result<()> {
    out.line(format_args!("Device Information:"))?;
    out.line(format_args!("  Name: {name}"))?;
    out.line(format_args!("  DPI: {dpi}"))?;
    out.line(format_args!(
        "  SmartShift: {} (threshold: {})",
        if smartshift.enabled {
            "enabled"
        } else {
            "disabled"
        },
        smartshift.threshold
    ))?;
    out.line(format_args!(
        "  Hi-Res Scroll: {}",
        if hires.enabled { "enabled" } else { "disabled" }
    ))
}

fn cmd_battery() -> Result<()> {
    info!("Checking battery...");

    if let Some(Response::Battery { level, status }) = try_request(&Request::Battery) {
        let mut out = Out::new();
        render_battery(&mut out, level, &status)?;
        return out.finish();
    }

    warn!("Daemon unavailable, falling back to direct device access");
    cmd_battery_direct()
}

fn cmd_battery_direct() -> Result<()> {
    info!("Opening device...");

    let mut device = MxMaster3s::open_bolt_receiver_discovered()?;
    let battery = device.battery_info()?;

    let mut out = Out::new();
    render_battery(&mut out, battery.level, &format!("{:?}", battery.status))?;
    out.finish()
}

fn render_battery(out: &mut Out, level: u8, status: &str) -> Result<()> {
    out.line(format_args!("Battery Status:"))?;
    out.line(format_args!("  Level: {level}%"))?;
    out.line(format_args!("  Status: {status}"))
}

fn cmd_hosts() -> Result<()> {
    info!("Reading Easy-Switch host info...");

    if let Some(Response::Hosts { hosts, current }) = try_request(&Request::Hosts) {
        let mut out = Out::new();
        render_hosts(&mut out, hosts, current)?;
        return out.finish();
    }

    warn!("Daemon unavailable, falling back to direct device access");
    cmd_hosts_direct()
}

fn cmd_hosts_direct() -> Result<()> {
    info!("Opening device...");

    let mut device = MxMaster3s::open_bolt_receiver_discovered()?;
    let (hosts, current) = device.host_info()?;

    let mut out = Out::new();
    render_hosts(&mut out, hosts, current)?;
    out.finish()
}

fn render_hosts(out: &mut Out, hosts: u8, current: u8) -> Result<()> {
    out.line(format_args!("Easy-Switch Hosts:"))?;
    out.line(format_args!("  Current host: {current} (zero-indexed)"))?;
    out.line(format_args!("  Supported hosts: {hosts}"))
}

fn cmd_buttons() -> Result<()> {
    info!("Listing reprogrammable controls...");

    if let Some(Response::Buttons { controls }) = try_request(&Request::Buttons) {
        let mut out = Out::new();
        out.line(format_args!("Reprogrammable Controls: {}", controls.len()))?;
        for control in controls {
            out.line(format_args!(
                "  0x{:04X} {:<18} flags {:#04x}",
                control.control_id,
                control_id_name(control.control_id),
                control.flags
            ))?;
        }
        return out.finish();
    }

    warn!("Daemon unavailable, falling back to direct device access");
    cmd_buttons_direct()
}

fn cmd_buttons_direct() -> Result<()> {
    info!("Opening device...");

    let mut device = MxMaster3s::open_bolt_receiver_discovered()?;
    let controls = device.list_reprog_controls()?;

    let mut out = Out::new();
    out.line(format_args!("Reprogrammable Controls: {}", controls.len()))?;
    for control in controls {
        let (divert_flags, remap) = device.control_divert(control.control_id)?;
        let diverted = divert_flags & 0x01 != 0;

        out.line(format_args!(
            "  0x{:04X} {:<18} flags {:#04x} divert {}",
            control.control_id,
            control_id_name(control.control_id),
            control.flags,
            if diverted {
                format!("on (remap {remap:#06x})")
            } else {
                "off".to_string()
            }
        ))?;
    }
    out.finish()
}

fn cmd_set(setting: SetCommands) -> Result<()> {
    match setting {
        SetCommands::Dpi { value } => {
            info!("Setting DPI to {}...", value);
            apply_setting(
                &Request::SetDpi { value },
                |device| device.set_dpi(value),
                format_args!("DPI set to {value}"),
            )
        }
        SetCommands::Smartshift { enabled, threshold } => {
            info!(
                "Configuring SmartShift: enabled={}, threshold={}",
                enabled, threshold
            );
            apply_setting(
                &Request::SetSmartShift { enabled, threshold },
                |device| device.set_smartshift(SmartShiftConfig { enabled, threshold }),
                format_args!(
                    "SmartShift configured: {} (threshold: {})",
                    if enabled { "enabled" } else { "disabled" },
                    threshold
                ),
            )
        }
        SetCommands::Hires { enabled, inverted } => {
            info!(
                "Configuring hi-res scroll: enabled={}, inverted={}",
                enabled, inverted
            );
            apply_setting(
                &Request::SetHires { enabled, inverted },
                |device| device.set_hires_scroll(HiResScrollConfig { enabled, inverted }),
                format_args!(
                    "Hi-res scroll: {}, inverted: {}",
                    if enabled { "enabled" } else { "disabled" },
                    if inverted { "yes" } else { "no" }
                ),
            )
        }
        SetCommands::Button {
            control_id,
            divert,
            raw_xy,
        } => {
            info!("Configuring button {control_id:#06x}: divert={divert}, raw_xy={raw_xy}");
            apply_setting(
                &Request::SetButtonDivert {
                    control_id,
                    divert,
                    raw_xy,
                },
                |device| device.set_control_divert(control_id, divert, raw_xy),
                format_args!("Button {control_id:#06x} divert: {divert}"),
            )
        }
        SetCommands::Remap {
            control_id,
            target_cid,
        } => {
            info!("Remapping control {control_id:#06x} to {target_cid:#06x}");
            apply_setting(
                &Request::SetButtonRemap {
                    control_id,
                    target_cid,
                },
                |device| device.set_control_remap(control_id, target_cid),
                format_args!("Control {control_id:#06x} remapped to {target_cid:#06x}"),
            )
        }
        SetCommands::Host { index } => {
            info!("Switching to host {index}...");
            apply_setting(
                &Request::SetHost { host: index },
                |device| device.set_host(index),
                format_args!("Switched to host {index}"),
            )
        }
    }
}

fn cmd_config(action: ConfigCommands) -> Result<()> {
    let mut out = Out::new();

    match action {
        ConfigCommands::Show => {
            let config = load_config()?;
            let toml_str = toml::to_string_pretty(&config)
                .map_err(|e| AppError::internal("Failed to serialize config").with_source(e))?;
            out.line(format_args!("{toml_str}"))?;
        }
        ConfigCommands::Edit => {
            let config_path = config_path()?;
            out.line(format_args!("Config location: {}", config_path.display()))?;
            out.line(format_args!("Edit the file with your preferred editor"))?;
        }
        ConfigCommands::Export { path } => {
            let config = load_config()?;
            let toml_str = toml::to_string_pretty(&config)
                .map_err(|e| AppError::internal("Failed to serialize config").with_source(e))?;
            std::fs::write(&path, toml_str)
                .map_err(|e| AppError::internal("Failed to write config").with_source(e))?;
            out.line(format_args!("Config exported to {path}"))?;
        }
        ConfigCommands::Import { path } => {
            let config = load_config_from_path(std::path::Path::new(&path))?;
            save_config(&config)?;
            out.line(format_args!("Config imported from {path}"))?;
        }
    }

    out.finish()
}

fn cmd_autostart(action: &AutostartCommands) -> Result<()> {
    let home =
        std::env::var("HOME").map_err(|e| AppError::internal("HOME not set").with_source(e))?;
    let service_dir = std::path::PathBuf::from(&home)
        .join(".config")
        .join("systemd")
        .join("user");
    let service_path = service_dir.join("logi-mx-daemon.service");
    let service_unit = include_str!("../../logi-mx-daemon.service");

    match action {
        AutostartCommands::Install => {
            std::fs::create_dir_all(&service_dir).map_err(|e| {
                AppError::internal("Failed to create systemd user dir").with_source(e)
            })?;
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

fn cmd_daemon(action: &DaemonCommands) -> Result<()> {
    let mut out = Out::new();
    match action {
        DaemonCommands::Status => {
            if matches!(try_request(&Request::Ping), Some(Response::Pong)) {
                out.line(format_args!("Daemon is running."))?;
            } else {
                out.line(format_args!("Daemon is not running."))?;
            }
        }
        DaemonCommands::Ping => {
            if try_request(&Request::Ping).is_some() {
                out.line(format_args!("pong"))?;
            } else {
                out.line(format_args!("no daemon"))?;
            }
        }
    }
    out.finish()
}
