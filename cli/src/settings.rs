// SPDX-FileCopyrightText: 2025-2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Device write commands: DPI, `SmartShift`, hi-res scroll, control
//! diversion, remapping and host switching, each through the daemon with a
//! direct HID++ fallback.

use logi_mx_daemon::ipc::{Request, Response, try_request};
use logi_mx_driver::prelude::*;
use masterror::prelude::*;
use tracing::{info, warn};

use crate::{args::SetCommands, out::Out};

/// Applies a setting through the daemon, falling back to direct device
/// access. Returns whether the direct fallback was used: a diverted control
/// only stays functional while a daemon pumps its notifications.
///
/// # Arguments
///
/// * `request` - IPC request carrying the desired change.
/// * `direct` - closure applying the same change to an opened device.
/// * `message` - confirmation line printed on success.
fn apply_setting(
    request: &Request,
    direct: impl FnOnce(&mut MxMaster3s) -> Result<()>,
    message: std::fmt::Arguments<'_>
) -> Result<bool> {
    let fallback = match try_request(request) {
        Some(Response::Ok) => false,
        Some(Response::Error {
            message
        }) => return Err(AppError::internal(message)),
        _ => {
            warn!("Daemon unavailable, falling back to direct device access");
            let mut device = MxMaster3s::open_bolt_receiver_discovered()?;
            direct(&mut device)?;
            true
        }
    };

    let mut out = Out::new();
    out.line(message)?;
    out.finish()?;
    Ok(fallback)
}

pub fn cmd_set(setting: SetCommands) -> Result<()> {
    match setting {
        SetCommands::Dpi {
            value
        } => {
            info!("Setting DPI to {}...", value);
            apply_setting(
                &Request::SetDpi {
                    value
                },
                |device| device.set_dpi(value),
                format_args!("DPI set to {value}")
            )?;
            Ok(())
        }
        SetCommands::Smartshift {
            enabled,
            threshold
        } => set_smartshift(enabled, threshold),
        SetCommands::Hires {
            enabled,
            inverted
        } => set_hires(enabled, inverted),
        SetCommands::Button {
            control_id,
            divert,
            raw_xy
        } => {
            info!("Configuring button {control_id:#06x}: divert={divert}, raw_xy={raw_xy}");
            let direct = apply_setting(
                &Request::SetButtonDivert {
                    control_id,
                    divert,
                    raw_xy
                },
                |device| device.set_control_divert(control_id, divert, raw_xy),
                format_args!("Button {control_id:#06x} divert: {divert}")
            )?;
            if divert && direct {
                warn!(
                    "the divert was written without a running daemon; the control stays diverted \
                     and produces no events until a daemon pumps it or `logi-mx set button \
                     {control_id:#06x}` clears it"
                );
            }
            Ok(())
        }
        SetCommands::Remap {
            control_id,
            target_cid
        } => {
            info!("Remapping control {control_id:#06x} to {target_cid:#06x}");
            apply_setting(
                &Request::SetButtonRemap {
                    control_id,
                    target_cid
                },
                |device| device.set_control_remap(control_id, target_cid),
                format_args!("Control {control_id:#06x} remapped to {target_cid:#06x}")
            )?;
            Ok(())
        }
        SetCommands::Host {
            index
        } => {
            info!("Switching to host {index}...");
            apply_setting(
                &Request::SetHost {
                    host: index
                },
                |device| device.set_host(index),
                format_args!("Switched to host {index}")
            )?;
            Ok(())
        }
    }
}

/// Applies a `SmartShift` configuration through the daemon or the device.
fn set_smartshift(enabled: bool, threshold: u8) -> Result<()> {
    info!("Configuring SmartShift: enabled={enabled}, threshold={threshold}");
    apply_setting(
        &Request::SetSmartShift {
            enabled,
            threshold
        },
        |device| {
            device.set_smartshift(SmartShiftConfig {
                enabled,
                threshold
            })
        },
        format_args!(
            "SmartShift configured: {} (threshold: {threshold})",
            if enabled { "enabled" } else { "disabled" }
        )
    )?;
    Ok(())
}

/// Applies a hi-res scroll configuration through the daemon or the device.
fn set_hires(enabled: bool, inverted: bool) -> Result<()> {
    info!("Configuring hi-res scroll: enabled={enabled}, inverted={inverted}");
    apply_setting(
        &Request::SetHires {
            enabled,
            inverted
        },
        |device| {
            device.set_hires_scroll(HiResScrollConfig {
                enabled,
                inverted
            })
        },
        format_args!(
            "Hi-res scroll: {}, inverted: {}",
            if enabled { "enabled" } else { "disabled" },
            if inverted { "yes" } else { "no" }
        )
    )?;
    Ok(())
}
