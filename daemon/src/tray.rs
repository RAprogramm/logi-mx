// SPDX-FileCopyrightText: 2025 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

use std::{
    env::current_exe,
    path::PathBuf,
    process::{Command, exit},
    sync::{Arc, Mutex}
};

use ksni::{Category, MenuItem, Tray, TrayService, menu::StandardItem};
use logi_mx_driver::prelude::*;
use tracing::{debug, error};

#[derive(Clone)]
pub struct DeviceStatus {
    pub connected:            bool,
    pub battery_level:        u8,
    pub battery_status:       String,
    pub dpi:                  u16,
    pub smartshift:           bool,
    pub smartshift_threshold: u8
}

impl Default for DeviceStatus {
    fn default() -> Self {
        Self {
            connected:            false,
            battery_level:        0,
            battery_status:       "Unknown".to_string(),
            dpi:                  1000,
            smartshift:           false,
            smartshift_threshold: 20
        }
    }
}

pub struct LogiTrayIcon {
    status: Arc<Mutex<DeviceStatus>>
}

impl LogiTrayIcon {
    pub fn new() -> Self {
        Self {
            status: Arc::new(Mutex::new(DeviceStatus::default()))
        }
    }

    pub fn get_status_handle(&self) -> Arc<Mutex<DeviceStatus>> {
        Arc::clone(&self.status)
    }

    pub fn update_status(&self) {
        if let Ok(mut device) = MxMaster3s::open_bolt_receiver(2) {
            let mut status = self.status.lock().unwrap();
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

            debug!("Tray status updated: battery={}%", status.battery_level);
        } else {
            let mut status = self.status.lock().unwrap();
            status.connected = false;
            debug!("Device not connected");
        }
    }
}

impl Tray for LogiTrayIcon {
    fn icon_name(&self) -> String {
        "input-mouse".to_string()
    }

    fn title(&self) -> String {
        let status = self.status.lock().unwrap();
        if status.connected {
            format!("MX Master 3S - {}%", status.battery_level)
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
        let status = self.status.lock().unwrap();

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

        if status.connected {
            menu.extend(vec![
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
                StandardItem {
                    label: "Open Configuration".into(),
                    icon_name: "preferences-system".into(),
                    activate: Box::new(|_this: &mut Self| {
                        let ui_path = current_exe()
                            .ok()
                            .and_then(|p| p.parent().map(|p| p.join("logi-mx-ui")))
                            .unwrap_or_else(|| PathBuf::from("logi-mx-ui"));

                        if let Err(e) = Command::new(&ui_path).spawn() {
                            error!("Failed to launch UI at {:?}: {}", ui_path, e);
                        }
                    }),
                    enabled: true,
                    ..Default::default()
                }
                .into(),
            ]);
        } else {
            menu.push(
                StandardItem {
                    label: "❌ Device Not Connected".into(),
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
                    exit(0);
                }),
                enabled: true,
                ..Default::default()
            }
            .into(),
        ]);

        menu
    }
}

pub fn spawn_tray() -> Result<Arc<Mutex<DeviceStatus>>, String> {
    let tray_icon = LogiTrayIcon::new();
    let status_handle = tray_icon.get_status_handle();

    tray_icon.update_status();

    let service = TrayService::new(tray_icon);
    service.spawn();

    Ok(status_handle)
}
