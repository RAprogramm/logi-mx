// SPDX-FileCopyrightText: 2025-2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Host permission checks: what the udev rules and kernel modules must
//! provide, and the exact commands that fix whatever is missing.

use logi_mx_daemon::ipc::{Request, Response, try_request};
use logi_mx_driver::prelude::*;

use crate::out::Out;

/// Checks host permissions and prints the fix for everything missing.
pub fn cmd_setup() -> Result<()> {
    let mut out = Out::new();
    out.line(format_args!("Setup checks:"))?;

    let mut healthy = true;
    let rules = RULES_CANDIDATES
        .iter()
        .any(|path| std::path::Path::new(path).exists());
    healthy &= report_check(
        &mut out,
        "udev rules",
        rules,
        "sudo install -m 0644 90-logi-mx.rules /etc/udev/rules.d/ && sudo udevadm control --reload-rules && sudo udevadm trigger"
    )?;

    let uinput_present = std::path::Path::new("/dev/uinput").exists();
    healthy &= report_check(
        &mut out,
        "uinput module",
        uinput_present,
        "sudo cp uinput.conf /etc/modules-load.d/ && sudo modprobe uinput"
    )?;

    let uinput_writable = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/uinput")
        .is_ok();
    healthy &= report_check(
        &mut out,
        "uinput access",
        uinput_writable,
        "sudo usermod -aG input $USER, then log out and back in"
    )?;

    let input_readable = std::fs::read_dir("/dev/input").is_ok_and(|entries| {
        entries.flatten().any(|entry| {
            entry.file_name().to_string_lossy().starts_with("event")
                && std::fs::File::open(entry.path()).is_ok()
        })
    });
    healthy &= report_check(
        &mut out,
        "input events",
        input_readable,
        "sudo usermod -aG input $USER, then log out and back in"
    )?;

    let daemon_up = matches!(try_request(&Request::Ping), Some(Response::Pong));
    healthy &= report_check(
        &mut out,
        "daemon",
        daemon_up,
        "systemctl --user enable --now logi-mx-daemon.socket logi-mx-daemon.service"
    )?;

    out.line(format_args!(
        "{}",
        if healthy {
            "All checks passed."
        } else {
            "Apply the commands above, then re-run `logi-mx setup`."
        }
    ))?;
    out.finish()
}

/// Locations the installer may place the udev rules file in.
const RULES_CANDIDATES: [&str; 3] = [
    "/etc/udev/rules.d/90-logi-mx.rules",
    "/usr/lib/udev/rules.d/90-logi-mx.rules",
    "/usr/local/lib/udev/rules.d/90-logi-mx.rules"
];

/// Prints one check line and reports whether it passed.
fn report_check(out: &mut Out, label: &str, ok: bool, fix: &str) -> Result<bool> {
    if ok {
        out.line(format_args!("  [ ok ] {label}"))?;
    } else {
        out.line(format_args!("  [miss] {label}: {fix}"))?;
    }
    Ok(ok)
}
