// SPDX-FileCopyrightText: 2025-2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! System tray integration for the daemon.
//!
//! Exposes the device state from [`crate::status`] through an `ksni` tray
//! icon and offers menu actions such as status refresh, UI launch, a full
//! restart of daemon, driver, tray and UI, and graceful shutdown.

use std::{
    env::current_exe,
    fs,
    os::unix::ffi::OsStrExt as _,
    path::PathBuf,
    process::{Command, exit},
    sync::{Arc, Mutex},
    thread
};

use ksni::{Category, MenuItem, Tray, TrayMethods, menu::StandardItem};
use tracing::{error, info};

use crate::status::{DeviceStatus, SharedStatus};

/// Tray icon driven by the shared [`DeviceStatus`].
pub struct LogiTrayIcon {
    status: SharedStatus,
    icons:  crate::tray_icon::TrayIconCache
}

impl LogiTrayIcon {
    /// Creates a tray icon with default status.
    #[must_use]
    pub fn new() -> Self {
        Self::from_status(Arc::new(Mutex::new(DeviceStatus::default())))
    }

    /// Creates a tray icon sharing an existing status handle.
    #[must_use]
    pub fn from_status(status: SharedStatus) -> Self {
        Self {
            status,
            icons: crate::tray_icon::TrayIconCache::default()
        }
    }

    fn shutdown_daemon() {
        info!("Initiating daemon shutdown");

        if let Err(e) = Command::new("systemctl")
            .args(["--user", "stop", "logi-mx-daemon.service"])
            .status()
        {
            error!("Failed to stop systemd service: {e}");
            exit(1);
        }

        info!("Daemon shutdown complete");
        exit(0);
    }

    fn lock_status(&self) -> std::sync::MutexGuard<'_, DeviceStatus> {
        self.status
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// Spawns a detached tray task, logging instead of panicking when the OS
/// refuses the thread.
fn spawn_tray_task<F>(name: &str, f: F)
where
    F: FnOnce() + Send + 'static
{
    if let Err(e) = thread::Builder::new().name(name.to_string()).spawn(f) {
        error!("Failed to spawn {name} thread: {e}");
    }
}

/// Runtime-dir marker recording that the configuration UI was open when a
/// full restart was requested.
fn ui_restart_marker() -> PathBuf {
    let runtime_dir =
        std::env::var("XDG_RUNTIME_DIR").map_or_else(|_| std::env::temp_dir(), PathBuf::from);
    runtime_dir.join("logi-mx-ui-restart")
}

/// Reports whether the process belongs to the configuration UI.
fn process_is_ui(pid: i32) -> bool {
    fs::read(format!("/proc/{pid}/cmdline")).is_ok_and(|cmdline| {
        cmdline
            .split(|byte| *byte == 0)
            .next()
            .is_some_and(|argv0| {
                PathBuf::from(std::ffi::OsStr::from_bytes(argv0))
                    .file_name()
                    .is_some_and(|name| name == "logi-mx-ui")
            })
    })
}

/// Sends SIGTERM to every running `logi-mx-ui` process.
///
/// Returns whether at least one process was signalled, so the restart flow
/// can reopen the window afterwards.
fn terminate_configuration_ui() -> bool {
    let Ok(entries) = fs::read_dir("/proc") else {
        return false;
    };

    let mut terminated = false;
    for entry in entries.flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse().ok())
        else {
            continue;
        };
        if !process_is_ui(pid) {
            continue;
        }

        // SAFETY: the PID was just read from /proc and verified to belong to
        // a logi-mx-ui process; signalling it only requests a graceful exit.
        if unsafe { libc::kill(pid, libc::SIGTERM) } == 0 {
            info!("Terminated configuration UI process {pid}");
            terminated = true;
        }
    }
    terminated
}

/// Restarts the whole stack: UI windows, daemon, driver worker and tray.
///
/// Under systemd a graceful exit is enough — `Restart=always` starts a fresh
/// instance while the socket stays listening. When started manually, a
/// replacement daemon is spawned first; it takes over through the instance
/// lock.
fn restart_everything() {
    info!("Full restart requested from the tray");

    if terminate_configuration_ui() {
        let _ = fs::write(ui_restart_marker(), "");
    }

    if std::env::var("INVOCATION_ID").is_err() {
        match current_exe() {
            Ok(exe) => {
                if let Err(e) = Command::new(&exe).spawn() {
                    error!("Failed to spawn replacement daemon: {e}");
                }
            }
            Err(e) => error!("Failed to resolve daemon path for restart: {e}")
        }
    }

    // SAFETY: SIGTERM to self is delivered through the daemon's own signal
    // listener and runs the regular graceful shutdown path.
    unsafe {
        libc::kill(libc::getpid(), libc::SIGTERM);
    }
}

/// Reopens the configuration UI when a full restart found it open.
pub fn restore_ui_after_restart() {
    let marker = ui_restart_marker();
    if marker.is_file() {
        let _ = fs::remove_file(&marker);
        info!("Restoring configuration UI after restart");
        spawn_tray_task("tray-restore-ui", launch_configuration_ui);
    }
}

impl Default for LogiTrayIcon {
    fn default() -> Self {
        Self::new()
    }
}

impl Tray for LogiTrayIcon {
    fn icon_name(&self) -> String {
        if !self
            .icons
            .icons(crate::tray_icon::color_for(&self.lock_status()))
            .is_empty()
        {
            return String::new();
        }

        let status = self.lock_status();
        if status.error.is_some() {
            "dialog-error".to_string()
        } else if status.connected {
            "input-mouse".to_string()
        } else {
            "input-mouse-symbolic".to_string()
        }
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        self.icons
            .icons(crate::tray_icon::color_for(&self.lock_status()))
    }

    fn title(&self) -> String {
        let snapshot = self.lock_status().clone();
        if let Some(error) = snapshot.error.as_ref() {
            format!("MX Master 3S - Error: {error}")
        } else if snapshot.connected {
            format!(
                "MX Master 3S - Battery: {}% ({}), DPI: {}",
                snapshot.battery_level, snapshot.battery_status, snapshot.dpi
            )
        } else {
            "MX Master 3S - Disconnected".to_string()
        }
    }

    fn id(&self) -> String {
        "logi-mx-daemon".to_string()
    }

    fn category(&self) -> Category {
        Category::Hardware
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let snapshot = self.lock_status().clone();

        let mut menu = vec![
            StandardItem {
                label: "Logitech MX Master 3S".into(),
                icon_name: "input-mouse".into(),
                activate: Box::new(|_| {}),
                enabled: false,
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
        ];

        if snapshot.connected {
            menu.extend(Self::device_menu_items(&snapshot));
        } else {
            menu.push(
                StandardItem {
                    label: "Device Not Connected".into(),
                    icon_name: "dialog-error".into(),
                    activate: Box::new(|_| {}),
                    enabled: false,
                    ..Default::default()
                }
                .into()
            );
        }

        menu.extend(vec![
            MenuItem::Separator,
            StandardItem {
                label: "Restart".into(),
                icon_name: "system-restart".into(),
                activate: Box::new(|_this: &mut Self| {
                    spawn_tray_task("tray-restart", restart_everything);
                }),
                enabled: true,
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Exit".into(),
                icon_name: "application-exit".into(),
                activate: Box::new(|_this: &mut Self| {
                    spawn_tray_task("tray-shutdown", Self::shutdown_daemon);
                }),
                enabled: true,
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: env!("CARGO_PKG_VERSION").to_string(),
                activate: Box::new(|_| {}),
                enabled: false,
                ..Default::default()
            }
            .into(),
        ]);

        menu
    }
}

impl LogiTrayIcon {
    /// Builds the informational and action entries shown while a device is
    /// connected.
    fn device_menu_items(status: &DeviceStatus) -> Vec<MenuItem<Self>> {
        vec![
            StandardItem {
                label: format!(
                    "Battery: {}% ({})",
                    status.battery_level, status.battery_status
                ),
                icon_name: "battery".into(),
                activate: Box::new(|_| {}),
                enabled: false,
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: format!("DPI: {}", status.dpi),
                icon_name: "preferences-desktop".into(),
                activate: Box::new(|_| {}),
                enabled: false,
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: format!(
                    "SmartShift: {} ({})",
                    if status.smartshift { "On" } else { "Off" },
                    status.smartshift_threshold
                ),
                icon_name: "preferences-system".into(),
                activate: Box::new(|_| {}),
                enabled: false,
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Refresh Status".into(),
                icon_name: "view-refresh".into(),
                activate: Box::new(|this: &mut Self| {
                    let status = Arc::clone(&this.status);
                    spawn_tray_task("tray-refresh", move || {
                        crate::status::refresh_from_device(&status);
                    });
                }),
                enabled: true,
                ..Default::default()
            }
            .into(),
            Self::open_configuration_item(),
        ]
    }

    /// Builds the menu entry that launches the configuration UI.
    fn open_configuration_item() -> MenuItem<Self> {
        StandardItem {
            label: "Open Configuration".into(),
            icon_name: "preferences-system".into(),
            activate: Box::new(|_this: &mut Self| {
                spawn_tray_task("tray-open-ui", launch_configuration_ui);
            }),
            enabled: true,
            ..Default::default()
        }
        .into()
    }
}

/// Launches `logi-mx-ui` next to the daemon binary and waits for it.
///
/// Runs on a dedicated thread: the `ksni` service task must never block on
/// child processes, otherwise every further tray menu activation freezes.
/// Waiting on the child also prevents zombie accumulation.
fn launch_configuration_ui() {
    let ui_path = current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.join("logi-mx-ui")))
        .unwrap_or_else(|| PathBuf::from("logi-mx-ui"));

    let mut cmd = Command::new(&ui_path);
    cmd.env_clear();
    for var in [
        "HOME",
        "DISPLAY",
        "WAYLAND_DISPLAY",
        "XDG_RUNTIME_DIR",
        "XDG_SESSION_TYPE",
        "XDG_CURRENT_DESKTOP",
        "DBUS_SESSION_BUS_ADDRESS",
        "PATH"
    ] {
        if let Ok(val) = std::env::var(var) {
            cmd.env(var, val);
        }
    }

    match cmd.spawn() {
        Ok(mut child) => {
            info!("Launched configuration UI");
            match child.wait() {
                Ok(status) => info!("Configuration UI exited with {status}"),
                Err(e) => error!("Failed to wait for configuration UI: {e}")
            }
        }
        Err(e) => error!("Failed to launch UI at {ui_path:?}: {e}")
    }
}

/// Spawns the tray icon bound to the shared status handle.
///
/// # Arguments
///
/// * `status` - Handle owned by the device worker; the tray only reads it.
///
/// # Errors
///
/// Returns a human-readable error string when the tray cannot be registered.
pub async fn spawn_tray(
    status: SharedStatus
) -> std::result::Result<ksni::Handle<LogiTrayIcon>, String> {
    let tray_icon = LogiTrayIcon::from_status(status);

    tray_icon
        .assume_sni_available(true)
        .spawn()
        .await
        .map_err(|e| format!("Failed to spawn tray: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn first_pixmap(tray: &LogiTrayIcon) -> Vec<u8> {
        tray.icon_pixmap()
            .first()
            .map_or_else(Vec::new, |icon| icon.data.clone())
    }

    #[test]
    fn test_tray_prefers_pixmap_over_theme_name() {
        let tray = LogiTrayIcon::new();
        assert!(!tray.icon_pixmap().is_empty());
        assert_eq!(tray.icon_name(), "");
    }

    #[test]
    fn test_tray_pixmap_color_follows_state() {
        let tray = LogiTrayIcon::new();
        let offline = first_pixmap(&tray);

        {
            let mut status = tray.status.lock().unwrap();
            status.connected = true;
            status.battery_status = "Discharging".to_string();
            status.battery_level = 50;
        }
        let online = first_pixmap(&tray);

        assert_ne!(offline, Vec::<u8>::new());
        assert_ne!(online, Vec::<u8>::new());
        assert_ne!(offline, online);
    }

    #[test]
    fn test_tray_pixmap_error_state_differs_from_connected() {
        let tray = LogiTrayIcon::new();
        {
            let mut status = tray.status.lock().unwrap();
            status.connected = true;
            status.battery_status = "Discharging".to_string();
            status.battery_level = 50;
        }
        let connected = first_pixmap(&tray);

        {
            let mut status = tray.status.lock().unwrap();
            status.error = Some("Test error".to_string());
        }
        let error = first_pixmap(&tray);

        assert_ne!(connected, error);
    }

    #[test]
    fn test_tray_title_with_error() {
        let tray = LogiTrayIcon::new();
        {
            let mut status = tray.status.lock().unwrap();
            status.error = Some("Device failure".to_string());
        }
        let title = tray.title();
        assert!(title.contains("Error: Device failure"));
    }

    #[test]
    fn test_tray_title_connected_with_details() {
        let tray = LogiTrayIcon::new();
        {
            let mut status = tray.status.lock().unwrap();
            status.connected = true;
            status.battery_level = 85;
            status.battery_status = "Charging".to_string();
            status.dpi = 2400;
        }
        let title = tray.title();
        assert!(title.contains("85%"));
        assert!(title.contains("Charging"));
        assert!(title.contains("2400"));
    }

    #[test]
    fn test_tray_title_disconnected() {
        let tray = LogiTrayIcon::new();
        let title = tray.title();
        assert_eq!(title, "MX Master 3S - Disconnected");
    }

    #[test]
    fn test_tray_id() {
        let tray = LogiTrayIcon::new();
        assert_eq!(tray.id(), "logi-mx-daemon");
    }

    #[test]
    fn test_tray_category() {
        let tray = LogiTrayIcon::new();
        assert_eq!(tray.category(), Category::Hardware);
    }

    #[test]
    fn test_get_status_handle() {
        let tray = LogiTrayIcon::new();
        let handle1 = Arc::clone(&tray.status);
        let handle2 = Arc::clone(&tray.status);

        {
            let mut status = handle1.lock().unwrap();
            status.connected = true;
            status.battery_level = 50;
        }

        {
            let status = handle2.lock().unwrap();
            assert!(status.connected);
            assert_eq!(status.battery_level, 50);
        }
    }

    #[test]
    fn test_from_status_shares_handle() {
        let status: SharedStatus = Arc::new(Mutex::new(DeviceStatus::default()));
        let tray = LogiTrayIcon::from_status(Arc::clone(&status));

        {
            let mut guard = status.lock().unwrap();
            guard.connected = true;
        }

        assert!(tray.lock_status().connected);
    }

    #[test]
    fn test_menu_ends_with_disabled_version_item() {
        let tray = LogiTrayIcon::new();
        let menu = tray.menu();

        match menu.last() {
            Some(MenuItem::Standard(item)) => {
                assert_eq!(item.label, env!("CARGO_PKG_VERSION"));
                assert_eq!(item.icon_name, "");
                assert!(!item.enabled);
            }
            _ => panic!("Expected a trailing version item")
        }
    }

    #[test]
    fn test_menu_offers_restart_before_exit() {
        let tray = LogiTrayIcon::new();
        let menu = tray.menu();

        let labels: Vec<&str> = menu
            .iter()
            .filter_map(|item| match item {
                MenuItem::Standard(standard) => Some(standard.label.as_str()),
                _ => None
            })
            .collect();
        let restart = labels
            .iter()
            .position(|label| *label == "Restart")
            .expect("Restart item present");
        let exit = labels
            .iter()
            .position(|label| *label == "Exit")
            .expect("Exit item present");
        assert!(restart < exit);
    }

    #[test]
    fn test_process_is_ui_rejects_foreign_processes() {
        let self_pid = i32::try_from(std::process::id()).unwrap_or(1);
        assert!(!process_is_ui(1));
        assert!(!process_is_ui(self_pid));
    }

    #[test]
    fn test_ui_restart_marker_lives_in_runtime_dir() {
        assert!(
            ui_restart_marker()
                .to_string_lossy()
                .contains("logi-mx-ui-restart")
        );
    }

    #[test]
    fn test_restore_ui_without_marker_is_noop() {
        let marker = ui_restart_marker();
        let _ = fs::remove_file(&marker);
        restore_ui_after_restart();
        assert!(!marker.exists());
    }

    #[test]
    fn test_title_priority_error_over_connected() {
        let tray = LogiTrayIcon::new();
        {
            let mut status = tray.status.lock().unwrap();
            status.connected = true;
            status.battery_level = 90;
            status.error = Some("Critical failure".to_string());
        }
        let title = tray.title();
        assert!(title.contains("Error: Critical failure"));
        assert!(!title.contains("90%"));
    }
}
