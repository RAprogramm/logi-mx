// SPDX-FileCopyrightText: 2025 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

use evdev::{Device, EventType, InputEvent, MiscCode, RelativeAxisCode, uinput::VirtualDevice};
use logi_mx_driver::prelude::*;
use masterror::prelude::*;
use tokio::task;
use tracing::{debug, error, info};

type Result<T> = std::result::Result<T, AppError>;

const VID_LOGITECH: u16 = 0x046D;
const PID_MX_MASTER_3S_USB: u16 = 0x4082;
const PID_MX_MASTER_3S_BT: u16 = 0xB034;
const PID_BOLT_RECEIVER: u16 = 0xC548;

pub struct ScrollHandler {
    config:            ScrollWheelConfig,
    #[allow(dead_code)]
    thumbwheel_config: ThumbWheelConfig
}

impl ScrollHandler {
    pub fn new(config: ScrollWheelConfig, thumbwheel_config: ThumbWheelConfig) -> Self {
        Self {
            config,
            thumbwheel_config
        }
    }

    pub fn spawn(
        scroll_config: ScrollWheelConfig,
        thumbwheel_config: ThumbWheelConfig
    ) -> Result<()> {
        task::spawn_blocking(move || {
            if let Err(e) = Self::run_blocking(scroll_config, thumbwheel_config) {
                error!("Scroll handler error: {:?}", e);
            }
        });

        Ok(())
    }

    fn run_blocking(
        scroll_config: ScrollWheelConfig,
        thumbwheel_config: ThumbWheelConfig
    ) -> Result<()> {
        let handler = Self::new(scroll_config, thumbwheel_config);

        let device = handler.find_device()?;
        info!("Found device: {:?}", device.name());

        let mut device = device;

        device
            .grab()
            .map_err(|e| AppError::internal("Failed to grab device").with_source(e))?;
        info!("Device grabbed successfully");

        let virtual_device = handler.create_virtual_device(&device)?;
        info!("Virtual device created");

        handler.process_events(device, virtual_device)?;

        Ok(())
    }

    #[cfg(test)]
    pub fn config(&self) -> &ScrollWheelConfig {
        &self.config
    }

    #[cfg(test)]
    pub fn thumbwheel_config(&self) -> &ThumbWheelConfig {
        &self.thumbwheel_config
    }

    fn find_device(&self) -> Result<Device> {
        let devices = evdev::enumerate()
            .map(|(_, device)| device)
            .collect::<Vec<_>>();

        for device in devices {
            let input_id = device.input_id();
            let name = device.name().unwrap_or("");

            // Skip virtual devices created by this daemon
            if name.contains("Virtual") {
                continue;
            }

            // Only consider Mouse or Pointer devices (not Consumer Control, Keyboard, etc.)
            if !name.contains("Mouse") && !name.contains("Pointer") {
                continue;
            }

            if (input_id.vendor() == VID_LOGITECH)
                && (input_id.product() == PID_MX_MASTER_3S_USB
                    || input_id.product() == PID_MX_MASTER_3S_BT
                    || input_id.product() == PID_BOLT_RECEIVER)
                && device.supported_events().contains(EventType::RELATIVE)
            {
                debug!(
                    "Found matching device: {} (VID:{:04x} PID:{:04x})",
                    name,
                    input_id.vendor(),
                    input_id.product()
                );
                return Ok(device);
            }
        }

        Err(AppError::not_found("MX Master 3S input device not found"))
    }

    fn create_virtual_device(&self, source: &Device) -> Result<evdev::uinput::VirtualDevice> {
        let mut builder = VirtualDevice::builder()
            .map_err(|e| AppError::internal("Failed to create uinput builder").with_source(e))?;

        builder = builder.name("MX Master 3S Virtual");
        builder = builder.input_id(source.input_id());

        if let Some(keys) = source.supported_keys() {
            builder = builder
                .with_keys(keys)
                .map_err(|e| AppError::internal("Failed to add keys").with_source(e))?;
        }

        builder = builder
            .with_relative_axes(&evdev::AttributeSet::from_iter([
                RelativeAxisCode::REL_X,
                RelativeAxisCode::REL_Y,
                RelativeAxisCode::REL_WHEEL,
                RelativeAxisCode::REL_HWHEEL,
                RelativeAxisCode::REL_WHEEL_HI_RES,
                RelativeAxisCode::REL_HWHEEL_HI_RES
            ]))
            .map_err(|e| AppError::internal("Failed to add relative axes").with_source(e))?;

        builder = builder
            .with_msc(&evdev::AttributeSet::from_iter([MiscCode::MSC_SCAN]))
            .map_err(|e| AppError::internal("Failed to add msc").with_source(e))?;

        builder
            .build()
            .map_err(|e| AppError::internal("Failed to build virtual device").with_source(e))
    }

    fn process_events(
        &self,
        mut device: Device,
        mut virtual_device: evdev::uinput::VirtualDevice
    ) -> Result<()> {
        // Accumulators for fractional scroll speed support
        let mut vertical_accumulator = 0.0f32;
        let mut horizontal_accumulator = 0.0f32;

        loop {
            let events = device
                .fetch_events()
                .map_err(|e| AppError::internal("Failed to fetch events").with_source(e))?;

            for event in events {
                if event.event_type() == EventType::RELATIVE {
                    let code = event.code();

                    if code == RelativeAxisCode::REL_WHEEL.0 {
                        let value = event.value();
                        // Normalize to ±1 per event (one detent = one line)
                        let normalized = if value > 0 {
                            1.0
                        } else if value < 0 {
                            -1.0
                        } else {
                            0.0
                        };

                        // Accumulate fractional scroll amount
                        vertical_accumulator += normalized * self.config.vertical_speed;

                        // Extract integer part to emit, keep fractional remainder
                        let emit_value = vertical_accumulator.trunc() as i32;
                        vertical_accumulator = vertical_accumulator.fract();

                        debug!(
                            "Vertical scroll: raw={} normalized={} speed={} emit={} accumulator={}",
                            value,
                            normalized,
                            self.config.vertical_speed,
                            emit_value,
                            vertical_accumulator
                        );

                        // Only emit if we have a non-zero value
                        if emit_value != 0 {
                            let modified_event =
                                InputEvent::new(event.event_type().0, code, emit_value);
                            virtual_device.emit(&[modified_event]).map_err(|e| {
                                AppError::internal("Failed to emit event").with_source(e)
                            })?;
                        }
                        continue;
                    } else if code == RelativeAxisCode::REL_HWHEEL.0 {
                        let value = event.value();
                        // Normalize to ±1 per event
                        let normalized = if value > 0 {
                            1.0
                        } else if value < 0 {
                            -1.0
                        } else {
                            0.0
                        };

                        // Accumulate fractional scroll amount
                        horizontal_accumulator += normalized * self.config.horizontal_speed;

                        // Extract integer part to emit, keep fractional remainder
                        let emit_value = horizontal_accumulator.trunc() as i32;
                        horizontal_accumulator = horizontal_accumulator.fract();

                        debug!(
                            "Horizontal scroll: raw={} normalized={} speed={} emit={} accumulator={}",
                            value,
                            normalized,
                            self.config.horizontal_speed,
                            emit_value,
                            horizontal_accumulator
                        );

                        // Only emit if we have a non-zero value
                        if emit_value != 0 {
                            let modified_event =
                                InputEvent::new(event.event_type().0, code, emit_value);
                            virtual_device.emit(&[modified_event]).map_err(|e| {
                                AppError::internal("Failed to emit event").with_source(e)
                            })?;
                        }
                        continue;
                    } else if code == RelativeAxisCode::REL_WHEEL_HI_RES.0
                        || code == RelativeAxisCode::REL_HWHEEL_HI_RES.0
                    {
                        // Skip hi-res events to avoid duplication (regular REL_WHEEL is enough)
                        continue;
                    }
                }

                // Forward all other events unchanged
                virtual_device
                    .emit(&[event])
                    .map_err(|e| AppError::internal("Failed to emit event").with_source(e))?;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scroll_handler_new() {
        let scroll_config = ScrollWheelConfig {
            vertical_speed:   2.5,
            horizontal_speed: 1.5,
            smooth_scrolling: true
        };
        let thumbwheel_config = ThumbWheelConfig {
            speed:            3.0,
            smooth_scrolling: false
        };

        let handler = ScrollHandler::new(scroll_config, thumbwheel_config);

        assert_eq!(handler.config().vertical_speed, 2.5);
        assert_eq!(handler.config().horizontal_speed, 1.5);
        assert!(handler.config().smooth_scrolling);
        assert_eq!(handler.thumbwheel_config().speed, 3.0);
        assert!(!handler.thumbwheel_config().smooth_scrolling);
    }

    #[test]
    fn test_scroll_handler_with_defaults() {
        let scroll_config = ScrollWheelConfig::default();
        let thumbwheel_config = ThumbWheelConfig::default();

        let handler = ScrollHandler::new(scroll_config, thumbwheel_config);

        assert_eq!(handler.config().vertical_speed, 1.0);
        assert_eq!(handler.config().horizontal_speed, 1.0);
        assert!(!handler.config().smooth_scrolling);
        assert_eq!(handler.thumbwheel_config().speed, 1.0);
        assert!(handler.thumbwheel_config().smooth_scrolling);
    }

    #[test]
    fn test_scroll_handler_fractional_speeds() {
        let scroll_config = ScrollWheelConfig {
            vertical_speed:   0.5,
            horizontal_speed: 0.7,
            smooth_scrolling: false
        };
        let thumbwheel_config = ThumbWheelConfig {
            speed:            0.3,
            smooth_scrolling: true
        };

        let handler = ScrollHandler::new(scroll_config, thumbwheel_config);

        assert_eq!(handler.config().vertical_speed, 0.5);
        assert_eq!(handler.config().horizontal_speed, 0.7);
        assert_eq!(handler.thumbwheel_config().speed, 0.3);
    }

    #[test]
    fn test_constants() {
        assert_eq!(VID_LOGITECH, 0x046D);
        assert_eq!(PID_MX_MASTER_3S_USB, 0x4082);
        assert_eq!(PID_MX_MASTER_3S_BT, 0xB034);
        assert_eq!(PID_BOLT_RECEIVER, 0xC548);
    }
}
