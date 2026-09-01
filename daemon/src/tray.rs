// SPDX-FileCopyrightText: 2025 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! System tray integration for the daemon.
//!
//! Exposes the device state through an `ksni` tray icon and offers menu
//! actions such as status refresh, UI launch and graceful shutdown.

use std::{
    env::current_exe,
    path::PathBuf,
    process::{Command, exit},
    sync::{Arc, Mutex}
};

#[cfg(feature = "tray")]
use gtk4::{AlertDialog, Window, glib};
use ksni::{Category, MenuItem, Tray, TrayMethods, menu::StandardItem};
use logi_mx_driver::prelude::*;
use tracing::{debug, error, info};

/// Snapshot of device state shared between the tray icon and the daemon.
#[derive(Clone)]
pub struct DeviceStatus {
    /// Whether the mouse is currently reachable.
    pub connected:            bool,
    /// Last observed battery charge percentage.
    pub battery_level:        u8,
    /// Last observed battery charge state.
    pub battery_status:       String,
    /// Last observed DPI setting.
    pub dpi:                  u16,
    /// Whether `SmartShift` automatic switching is enabled.
    pub smartshift:           bool,
    /// `SmartShift` scroll-speed threshold.
    pub smartshift_threshold: u8,
    /// Last fatal error message, if any.
    pub error:                Option<String>
}

impl Default for DeviceStatus {
    fn default() -> Self {
        Self {
            connected:            false,
            battery_level:        0,
            battery_status:       "Unknown".to_string(),
            dpi:                  1000,
            smartshift:           false,
            smartshift_threshold: 20,
            error:                None
        }
    }
}

/// Tray icon driven by [`DeviceStatus`].
pub struct LogiTrayIcon {
    status: Arc<Mutex<DeviceStatus>>
}

impl LogiTrayIcon {
    /// Creates a tray icon with default status.
    #[must_use]
    pub fn new() -> Self {
        Self {
            status: Arc::new(Mutex::new(DeviceStatus::default()))
        }
    }

    /// Returns a shared handle to the status so the daemon can update it.
    #[must_use]
    pub fn get_status_handle(&self) -> Arc<Mutex<DeviceStatus>> {
        Arc::clone(&self.status)
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
    pub fn update_status(&self) {
        if let Ok(mut device) = MxMaster3s::open_bolt_receiver_discovered() {
            let battery_level = {
                let mut status = self
                    .status
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                status.connected = true;

                if let Ok(battery) = device.get_battery_info() {
                    status.battery_level = battery.level;
                    status.battery_status = format!("{:?}", battery.status);
                }

                if let Ok(dpi) = device.get_dpi() {
                    status.dpi = dpi;
                }

                if let Ok(ss_config) = device.get_smartshift() {
                    status.smartshift = ss_config.enabled;
                    status.smartshift_threshold = ss_config.threshold;
                }

                status.battery_level
            };

            debug!("Tray status updated: battery={battery_level}%");
        } else {
            {
                let mut status = self
                    .status
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                status.connected = false;
            }
            debug!("Device not connected");
        }
    }

    fn lock_status(&self) -> std::sync::MutexGuard<'_, DeviceStatus> {
        self.status
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
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

                for var in [
                    "DISPLAY",
                    "WAYLAND_DISPLAY",
                    "XDG_RUNTIME_DIR",
                    "XDG_SESSION_TYPE",
                    "DBUS_SESSION_BUS_ADDRESS"
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

/// Spawns the tray icon and returns the shared status handle.
///
/// # Errors
///
/// Returns a human-readable error string when the tray cannot be registered.
pub async fn spawn_tray() -> std::result::Result<Arc<Mutex<DeviceStatus>>, String> {
    let tray_icon = LogiTrayIcon::new();
    let status_handle = tray_icon.get_status_handle();
    tray_icon.update_status();

    tray_icon
        .spawn()
        .await
        .map(|_| status_handle)
        .map_err(|e| format!("Failed to spawn tray: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_device_status_default_with_error() {
        let status = DeviceStatus::default();
        assert!(status.error.is_none());
    }

    #[test]
    fn test_device_status_with_error() {
        let status = DeviceStatus {
            connected:            false,
            battery_level:        0,
            battery_status:       "Unknown".to_string(),
            dpi:                  1000,
            smartshift:           false,
            smartshift_threshold: 20,
            error:                Some("Test error".to_string())
        };
        assert_eq!(status.error, Some("Test error".to_string()));
    }

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
        let handle1 = tray.get_status_handle();
        let handle2 = tray.get_status_handle();

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
    fn test_device_status_all_fields() {
        let status = DeviceStatus {
            connected:            true,
            battery_level:        95,
            battery_status:       "Discharging".to_string(),
            dpi:                  3200,
            smartshift:           true,
            smartshift_threshold: 30,
            error:                None
        };

        assert!(status.connected);
        assert_eq!(status.battery_level, 95);
        assert_eq!(status.battery_status, "Discharging");
        assert_eq!(status.dpi, 3200);
        assert!(status.smartshift);
        assert_eq!(status.smartshift_threshold, 30);
        assert!(status.error.is_none());
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
