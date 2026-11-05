// SPDX-FileCopyrightText: 2025-2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Read-only commands: device info, battery, hosts and control listing,
//! each through the daemon with a direct HID++ fallback.

use logi_mx_daemon::ipc::{Request, Response, try_request};
use logi_mx_driver::prelude::*;
use tracing::{info, warn};

use crate::out::Out;

pub fn cmd_info() -> Result<()> {
    info!("Requesting device info...");

    if let Some(Response::Info {
        name,
        dpi,
        smartshift,
        hires
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
    hires: HiResScrollConfig
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

pub fn cmd_battery() -> Result<()> {
    info!("Checking battery...");

    if let Some(Response::Battery {
        level,
        status
    }) = try_request(&Request::Battery)
    {
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

pub fn cmd_hosts() -> Result<()> {
    info!("Reading Easy-Switch host info...");

    if let Some(Response::Hosts {
        hosts,
        current
    }) = try_request(&Request::Hosts)
    {
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

pub fn cmd_buttons() -> Result<()> {
    info!("Listing reprogrammable controls...");

    if let Some(Response::Buttons {
        controls
    }) = try_request(&Request::Buttons)
    {
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
