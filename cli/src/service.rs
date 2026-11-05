// SPDX-FileCopyrightText: 2025-2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Service and configuration management: config file commands, systemd
//! unit installation and daemon liveness checks.

use logi_mx_daemon::ipc::{Request, Response, try_request};
use logi_mx_driver::prelude::*;
use masterror::prelude::*;
use tracing::warn;

use crate::{
    args::{AutostartCommands, ConfigCommands, DaemonCommands},
    out::Out
};

pub fn cmd_config(action: ConfigCommands) -> Result<()> {
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
        ConfigCommands::Export {
            path
        } => {
            let config = load_config()?;
            let toml_str = toml::to_string_pretty(&config)
                .map_err(|e| AppError::internal("Failed to serialize config").with_source(e))?;
            std::fs::write(&path, toml_str)
                .map_err(|e| AppError::internal("Failed to write config").with_source(e))?;
            out.line(format_args!("Config exported to {path}"))?;
        }
        ConfigCommands::Import {
            path
        } => {
            let config = load_config_from_path(std::path::Path::new(&path))?;
            save_config(&config)?;
            out.line(format_args!("Config imported from {path}"))?;
        }
    }

    out.finish()
}

/// Runs `systemctl --user` with the given arguments.
fn systemctl(args: &[&str]) -> Result<std::process::ExitStatus> {
    std::process::Command::new("systemctl")
        .arg("--user")
        .args(args)
        .status()
        .map_err(|e| AppError::internal("Failed to run systemctl").with_source(e))
}

/// Asks systemd to re-read the user unit files, reporting a refusal.
fn reload_units() -> Result<()> {
    let status = systemctl(&["daemon-reload"])?;
    if !status.success() {
        return Err(AppError::internal(
            "systemd did not reload the unit files; run `systemctl --user daemon-reload` manually"
        ));
    }
    Ok(())
}

pub fn cmd_autostart(action: &AutostartCommands) -> Result<()> {
    let home =
        std::env::var("HOME").map_err(|e| AppError::internal("HOME not set").with_source(e))?;
    let service_dir = std::path::PathBuf::from(&home)
        .join(".config")
        .join("systemd")
        .join("user");
    let units: [(&str, &str); 3] = [
        (
            "logi-mx-daemon.socket",
            include_str!("../../logi-mx-daemon.socket")
        ),
        (
            "logi-mx-daemon.service",
            include_str!("../../logi-mx-daemon.service")
        ),
        (
            "logi-mx-overlay.service",
            include_str!("../../overlay/logi-mx-overlay.service")
        )
    ];

    match action {
        AutostartCommands::Install => {
            std::fs::create_dir_all(&service_dir).map_err(|e| {
                AppError::internal("Failed to create systemd user dir").with_source(e)
            })?;
            for (name, unit) in units {
                std::fs::write(service_dir.join(name), unit).map_err(|e| {
                    AppError::internal("Failed to write service file").with_source(e)
                })?;
            }

            reload_units()?;
            let socket = systemctl(&["enable", "--now", "logi-mx-daemon.socket"])?;
            if !socket.success() {
                return Err(AppError::internal("Failed to enable daemon socket"));
            }
            let status = systemctl(&["enable", "--now", "logi-mx-daemon.service"])?;
            if !status.success() {
                return Err(AppError::internal("Failed to enable daemon service"));
            }
            let overlay = systemctl(&["enable", "--now", "logi-mx-overlay.service"])?;
            if overlay.success() {
                println!("Autostart installed and daemon and overlay started.");
            } else {
                let binary = std::path::Path::new(&home).join(".local/bin/logi-mx-overlay");
                if binary.is_file() {
                    warn!(
                        "the overlay service did not start although its binary is installed; \
                         check `systemctl --user status logi-mx-overlay.service`"
                    );
                } else {
                    warn!(
                        "the overlay service did not start; install the logi-mx-overlay binary \
                         to ~/.local/bin"
                    );
                }
                println!("Autostart installed and daemon started; the overlay did not start.");
            }
        }
        AutostartCommands::Uninstall => {
            for (name, _) in units.iter().rev() {
                // Disable even when no local unit file exists: the unit may
                // be enabled from the packaged /usr/lib/systemd/user copy.
                // A failure without a local file means systemd never knew
                // the unit, which is nothing to undo.
                let local = service_dir.join(name).is_file();
                let status = systemctl(&["disable", "--now", name])?;
                if !status.success() && local {
                    return Err(AppError::internal(format!("Failed to disable {name}")));
                }
                match std::fs::remove_file(service_dir.join(name)) {
                    Ok(()) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => {
                        return Err(
                            AppError::internal("Failed to remove unit file").with_source(e)
                        );
                    }
                }
            }
            reload_units()?;
            println!("Autostart removed.");
        }
        AutostartCommands::Status => {
            let daemon = systemctl(&["is-active", "logi-mx-daemon.service"])?;
            if daemon.success() {
                println!("Daemon is active.");
            } else {
                println!("Daemon is not active.");
            }
            let overlay = systemctl(&["is-active", "logi-mx-overlay.service"])?;
            if overlay.success() {
                println!("Overlay is active.");
            } else {
                println!("Overlay is not active.");
            }
        }
    }

    Ok(())
}

pub fn cmd_daemon(action: &DaemonCommands) -> Result<()> {
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
            if matches!(try_request(&Request::Ping), Some(Response::Pong)) {
                out.line(format_args!("pong"))?;
            } else {
                out.line(format_args!("no daemon"))?;
            }
        }
    }
    out.finish()
}
