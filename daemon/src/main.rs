// SPDX-FileCopyrightText: 2025 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

#[cfg(feature = "tray")]
mod tray;

use std::{collections::HashMap, time::Duration};

use logi_mx_driver::prelude::*;
use masterror::prelude::*;
use tokio::{
    select,
    signal::unix::{SignalKind, signal},
    sync::mpsc,
    time::sleep
};
use tracing::{debug, error, info, warn};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};
use udev::MonitorBuilder;

type Result<T> = std::result::Result<T, AppError>;

struct DeviceManager {
    devices: HashMap<String, MxMaster3s>,
    config:  Config
}

impl DeviceManager {
    fn new(config: Config) -> Self {
        Self {
            devices: HashMap::new(),
            config
        }
    }

    async fn handle_device_added(&mut self, device_path: String) -> Result<()> {
        info!("Device added: {}", device_path);

        match MxMaster3s::open_bolt_receiver(2) {
            Ok(mut device) => {
                if let Ok(name) = device.get_device_name() {
                    info!("Detected: {}", name);

                    if let Some(device_config) =
                        self.config.devices.iter().find(|d| d.name == name)
                    {
                        info!("Applying configuration for {}", name);
                        if let Err(e) = self.apply_config(&mut device, device_config).await {
                            error!("Failed to apply config: {}", e);
                        }
                    }

                    self.devices.insert(device_path, device);
                }
            }
            Err(e) => {
                warn!("Failed to open device: {}", e);
            }
        }

        Ok(())
    }

    async fn handle_device_removed(&mut self, device_path: &str) {
        info!("Device removed: {}", device_path);
        self.devices.remove(device_path);
    }

    async fn apply_config(&self, device: &mut MxMaster3s, config: &DeviceConfig) -> Result<()> {
        debug!("Setting DPI to {}", config.dpi);
        if let Err(e) = device.set_dpi(config.dpi) {
            error!("Failed to set DPI: {}", e);
        }

        debug!(
            "Setting SmartShift: enabled={}, threshold={}",
            config.smartshift.enabled, config.smartshift.threshold
        );
        if let Err(e) = device.set_smartshift(config.smartshift) {
            error!("Failed to set SmartShift: {}", e);
        }

        debug!(
            "Setting hi-res scroll: enabled={}, inverted={}",
            config.hiresscroll.enabled, config.hiresscroll.inverted
        );
        if let Err(e) = device.set_hires_scroll(config.hiresscroll) {
            error!("Failed to set hi-res scroll: {}", e);
        }

        for (button, action) in &config.buttons {
            debug!("Setting button {:?} to action {:?}", button, action);
            if let Err(e) = device.set_button_action(*button, action.clone()) {
                error!("Failed to set button action: {}", e);
            }
        }

        info!("Configuration applied successfully");
        Ok(())
    }

    #[allow(dead_code)]
    async fn monitor_battery(&mut self) {
        loop {
            sleep(Duration::from_secs(300)).await;

            let paths: Vec<String> = self.devices.keys().cloned().collect();

            for path in paths {
                if let Some(device) = self.devices.get_mut(&path) {
                    match device.get_battery_info() {
                        Ok(battery) => {
                            info!(
                                "Device {} battery: {}% ({:?})",
                                path, battery.level, battery.status
                            );

                            if battery.level < 10 {
                                warn!("Low battery on {}: {}%", path, battery.level);
                            }
                        }
                        Err(e) => {
                            debug!("Failed to get battery info for {}: {}", path, e);
                        }
                    }
                }
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_default_env())
        .init();

    info!("Starting logi-mx-daemon");

    let config = load_config().unwrap_or_else(|e| {
        warn!("Failed to load config: {}. Using default.", e);
        Config::default()
    });

    let mut manager = DeviceManager::new(config);

    #[cfg(feature = "tray")]
    {
        info!("Initializing system tray...");
        use std::sync::Arc;

        use crate::tray::spawn_tray;

        match spawn_tray().await {
            Ok(tray_status) => {
                info!("System tray initialized");

                let tray_status_clone = Arc::clone(&tray_status);
                tokio::spawn(async move {
                    loop {
                        sleep(Duration::from_secs(30)).await;
                        if let Ok(mut device) = MxMaster3s::open_bolt_receiver(2)
                            && let Ok(mut status) = tray_status_clone.lock()
                        {
                            status.connected = true;
                            if let Ok(battery) = device.get_battery_info() {
                                status.battery_level = battery.level;
                                status.battery_status = format!("{:?}", battery.status);
                            }
                            if let Ok(dpi) = device.get_dpi() {
                                status.dpi = dpi;
                            }
                            if let Ok(ss) = device.get_smartshift() {
                                status.smartshift = ss.enabled;
                                status.smartshift_threshold = ss.threshold;
                            }
                            debug!("Tray status auto-updated");
                        }
                    }
                });
            }
            Err(e) => {
                warn!("Failed to initialize tray: {}. Continuing without tray.", e);
            }
        }
    }

    let (tx, mut rx) = mpsc::channel::<UdevEvent>(32);

    std::thread::spawn(move || {
        if let Err(e) = monitor_udev_events_sync(tx) {
            error!("Udev monitor error: {}", e);
        }
    });

    let mut sigterm = signal(SignalKind::terminate())
        .map_err(|e| AppError::internal("Failed to setup SIGTERM handler").with_source(e))?;
    let mut sigint = signal(SignalKind::interrupt())
        .map_err(|e| AppError::internal("Failed to setup SIGINT handler").with_source(e))?;

    info!("Daemon started successfully");

    loop {
        select! {
            Some(event) = rx.recv() => {
                match event {
                    UdevEvent::Add(path) => {
                        if let Err(e) = manager.handle_device_added(path).await {
                            error!("Error handling device add: {}", e);
                        }
                    }
                    UdevEvent::Remove(path) => {
                        manager.handle_device_removed(&path).await;
                    }
                }
            }
            _ = sigterm.recv() => {
                info!("Received SIGTERM, shutting down...");
                break;
            }
            _ = sigint.recv() => {
                info!("Received SIGINT, shutting down...");
                break;
            }
        }
    }

    info!("Daemon stopped");
    Ok(())
}

#[derive(Debug)]
enum UdevEvent {
    Add(String),
    Remove(String)
}

fn monitor_udev_events_sync(tx: mpsc::Sender<UdevEvent>) -> Result<()> {
    let monitor = MonitorBuilder::new()
        .map_err(|e| AppError::internal("Failed to create udev monitor").with_source(e))?
        .match_subsystem("hidraw")
        .map_err(|e| AppError::internal("Failed to match subsystem").with_source(e))?
        .listen()
        .map_err(|e| AppError::internal("Failed to start udev monitor").with_source(e))?;

    info!("Monitoring udev events for hidraw devices");

    let iter = monitor.iter();
    for event in iter {
        let device_path = event
            .device()
            .devnode()
            .and_then(|p| p.to_str())
            .map(String::from);

        if let Some(path) = device_path {
            let udev_event = match event.event_type() {
                udev::EventType::Add => Some(UdevEvent::Add(path)),
                udev::EventType::Remove => Some(UdevEvent::Remove(path)),
                _ => None
            };

            if let Some(evt) = udev_event
                && tx.blocking_send(evt).is_err()
            {
                error!("Failed to send udev event");
                break;
            }
        }
    }

    Ok(())
}
