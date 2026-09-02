// SPDX-FileCopyrightText: 2025 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! System tray integration for the daemon.
//!
//! Exposes the device state from [`crate::status`] through an `ksni` tray
//! icon and offers menu actions such as status refresh, UI launch and
//! graceful shutdown.

use std::{
    env::current_exe,
    path::PathBuf,
    process::{Command, exit},
    sync::{Arc, Mutex}
};

#[cfg(feature = "tray")]
use gtk4::{AlertDialog, Window, glib};
use ksni::{Category, MenuItem, Tray, TrayMethods, menu::StandardItem};
use tracing::{error, info};

use crate::status::{DeviceStatus, SharedStatus};

/// Tray icon driven by the shared [`DeviceStatus`].
pub struct LogiTrayIcon {
    status: SharedStatus
}

impl LogiTrayIcon {
    /// Creates a tray icon with default status.
    #[must_use]
    pub fn new() -> Self {
        Self {
            status: Arc::new(Mutex::new(DeviceStatus::default()))
        }
    }

    /// Creates a tray icon sharing an existing status handle.
    #[must_use]
    pub const fn from_status(status: SharedStatus) -> Self {
        Self {
            status
        }
    }

    #[cfg(feature = "tray")]
    fn show_exit_confirmation() -> bool {
        gtk4::init().ok();

        let (tx, rx) = std::sync::mpsc::channel();

        glib::MainContext::default().spawn_local(async move {
            let dialog = AlertDialog::builder()
                .message("Stop Logitech MX Daemon?")
                .detail(
                    "The daemon will be stopped and the following features will become unavailable:\n\n\
                     - Custom scroll wheel speed\n\
                     - DPI adjustment\n\
                     - SmartShift configuration\n\
                     - Hi-res scrolling\n\
                     - Battery monitoring\n\n\
                     Your mouse will use default Linux drivers."
                )
                .buttons(vec!["Cancel", "Stop Daemon"])
                .default_button(0)
                .cancel_button(0)
                .build();

            let response = dialog.choose_future(None::<&Window>).await;
            let confirmed = response.is_ok_and(|r| r == 1);
            tx.send(confirmed).ok();
        });

        let result = rx.recv().unwrap_or(false);
        info!("Exit confirmation dialog result: {result}");
        result
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

    /// Queries the device and refreshes the shared status.
    ///
    /// Runs on the `ksni` worker thread from the tray menu, so the blocking
    /// device I/O never stalls the daemon event loop.
    pub fn update_status(&self) {
        crate::status::refresh_from_device(&self.status);
    }

    fn lock_status(&self) -> std::sync::MutexGuard<'_, DeviceStatus> {
        self.status
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl Default for LogiTrayIcon {
    fn default() -> Self {
        Self::new()
    }
}

impl Tray for LogiTrayIcon {
    fn icon_name(&self) -> String {
        let status = self.lock_status();
        if status.error.is_some() {
            "dialog-error".to_string()
        } else if status.connected {
            "input-mouse".to_string()
        } else {
            "input-mouse-symbolic".to_string()
        }
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
                label: "Exit".into(),
                icon_name: "application-exit".into(),
                activate: Box::new(|_this: &mut Self| {
                    #[cfg(feature = "tray")]
                    {
                        if Self::show_exit_confirmation() {
                            Self::shutdown_daemon();
                        } else {
                            info!("User cancelled daemon shutdown");
                        }
                    }
                    #[cfg(not(feature = "tray"))]
                    Self::shutdown_daemon();
                }),
                enabled: true,
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
                    this.update_status();
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
                let ui_path = current_exe()
                    .ok()
                    .and_then(|p| p.parent().map(|p| p.join("logi-mx-ui")))
                    .unwrap_or_else(|| PathBuf::from("logi-mx-ui"));

                let mut cmd = Command::new(&ui_path);
                cmd.env_clear();
                for var in [
                    "DISPLAY",
                    "WAYLAND_DISPLAY",
                    "XDG_RUNTIME_DIR",
                    "XDG_SESSION_TYPE",
                    "DBUS_SESSION_BUS_ADDRESS",
                    "PATH"
                ] {
                    if let Ok(val) = std::env::var(var) {
                        cmd.env(var, val);
                    }
                }

                if let Err(e) = cmd.spawn() {
                    error!("Failed to launch UI at {ui_path:?}: {e}");
                } else {
                    info!("Launched configuration UI");
                }
            }),
            enabled: true,
            ..Default::default()
        }
        .into()
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
pub async fn spawn_tray(status: SharedStatus) -> std::result::Result<(), String> {
    let tray_icon = LogiTrayIcon::from_status(status);

    tray_icon
        .spawn()
        .await
        .map(|_| ())
        .map_err(|e| format!("Failed to spawn tray: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tray_icon_name_disconnected() {
        let tray = LogiTrayIcon::new();
        assert_eq!(tray.icon_name(), "input-mouse-symbolic");
    }

    #[test]
    fn test_tray_icon_name_connected() {
        let tray = LogiTrayIcon::new();
        {
            let mut status = tray.status.lock().unwrap();
            status.connected = true;
        }
        assert_eq!(tray.icon_name(), "input-mouse");
    }

    #[test]
    fn test_tray_icon_name_error() {
        let tray = LogiTrayIcon::new();
        {
            let mut status = tray.status.lock().unwrap();
            status.error = Some("Test error".to_string());
        }
        assert_eq!(tray.icon_name(), "dialog-error");
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
    fn test_icon_name_priority_error_over_connected() {
        let tray = LogiTrayIcon::new();
        {
            let mut status = tray.status.lock().unwrap();
            status.connected = true;
            status.error = Some("Test error".to_string());
        }
        assert_eq!(tray.icon_name(), "dialog-error");
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
