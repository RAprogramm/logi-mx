// SPDX-FileCopyrightText: 2025 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Monochrome tray icon rendering with per-state tinting.
//!
//! The SNI `IconPixmap` property carries raw pixels, so the daemon fully
//! controls the icon appearance: a single silhouette is rasterized once per
//! state color and published in every size the host may request. Hosts that
//! ignore pixmaps fall back to the `IconName` theme lookup.
//!
//! Pixel contract (matches Waybar, KDE and GNOME hosts): native ARGB32, i.e.
//! BGRA byte order on little-endian systems, with premultiplied alpha.

use std::cell::RefCell;

use ksni::Icon;
use tracing::error;

use crate::status::{
    BATTERY_CRITICAL_PERCENT, BATTERY_DISCHARGING, BATTERY_LOW_PERCENT, DeviceStatus
};

/// Mouse silhouette embedded from `assets/input-mouse-symbolic.svg`.
const MOUSE_SVG: &str = include_str!("../assets/input-mouse-symbolic.svg");

/// Icon sizes published to the SNI host, in pixels.
const ICON_SIZES: [u32; 3] = [16, 22, 32];

/// Opaque RGB color used to tint the silhouette.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb {
    /// Red channel.
    pub r: u8,
    /// Green channel.
    pub g: u8,
    /// Blue channel.
    pub b: u8
}

/// Connected and healthy device.
const COLOR_CONNECTED: Rgb = Rgb {
    r: 0xd3,
    g: 0xd3,
    b: 0xd3
};
/// Device unreachable.
const COLOR_DISCONNECTED: Rgb = Rgb {
    r: 0x80,
    g: 0x80,
    b: 0x80
};
/// Charge at or below the low threshold.
const COLOR_BATTERY_LOW: Rgb = Rgb {
    r: 0xf5,
    g: 0x79,
    b: 0x00
};
/// Charge at or below the critical threshold.
const COLOR_BATTERY_CRITICAL: Rgb = Rgb {
    r: 0xcc,
    g: 0x00,
    b: 0x00
};
/// Daemon reported an error.
const COLOR_ERROR: Rgb = Rgb {
    r: 0xcc,
    g: 0x00,
    b: 0x00
};

/// Maps a device status snapshot to the tray icon color.
///
/// Precedence: error, then disconnection, then battery thresholds while
/// discharging, then the healthy connected color.
///
/// # Examples
///
/// ```
/// use logi_mx_daemon::{status::DeviceStatus, tray_icon::color_for};
///
/// let offline = DeviceStatus::default();
///
/// let mut online = DeviceStatus::default();
/// online.connected = true;
/// online.battery_status = "Discharging".into();
/// online.battery_level = 80;
///
/// assert_ne!(color_for(&offline), color_for(&online));
/// ```
#[must_use]
pub fn color_for(status: &DeviceStatus) -> Rgb {
    if status.error.is_some() {
        return COLOR_ERROR;
    }
    if !status.connected {
        return COLOR_DISCONNECTED;
    }
    if status.battery_status == BATTERY_DISCHARGING {
        if status.battery_level <= BATTERY_CRITICAL_PERCENT {
            return COLOR_BATTERY_CRITICAL;
        }
        if status.battery_level <= BATTERY_LOW_PERCENT {
            return COLOR_BATTERY_LOW;
        }
    }
    COLOR_CONNECTED
}

/// Caches rendered pixmaps and re-renders only when the state color changes.
#[derive(Default)]
pub struct TrayIconCache {
    cached: RefCell<Option<(Rgb, Vec<Icon>)>>
}

impl TrayIconCache {
    /// Returns the published pixmaps for the given color.
    ///
    /// Rendering happens at most once per color change; later calls clone the
    /// cached buffers.
    pub fn icons(&self, color: Rgb) -> Vec<Icon> {
        let mut cached = self.cached.borrow_mut();
        let stale = cached
            .as_ref()
            .is_none_or(|(cached_color, icons)| *cached_color != color || icons.is_empty());
        if stale {
            *cached = Some((color, render(color)));
        }
        cached
            .as_ref()
            .map_or_else(Vec::new, |(_, icons)| icons.clone())
    }
}

/// Rasterizes and tints the silhouette at every published size.
fn render(color: Rgb) -> Vec<Icon> {
    let tree = match usvg::Tree::from_data(MOUSE_SVG.as_bytes(), &usvg::Options::default()) {
        Ok(tree) => tree,
        Err(e) => {
            error!("Failed to parse tray icon SVG: {e}");
            return Vec::new();
        }
    };

    ICON_SIZES
        .iter()
        .filter_map(|&size| render_size(&tree, size, color))
        .collect()
}

fn render_size(tree: &usvg::Tree, size: u32, color: Rgb) -> Option<Icon> {
    #[allow(
        clippy::cast_precision_loss,
        reason = "icon sizes are small and exactly representable as f32"
    )]
    let scale = size as f32 / tree.size().width();

    let mut pixmap = resvg::tiny_skia::Pixmap::new(size, size)?;
    let transform = resvg::tiny_skia::Transform::from_scale(scale, scale);
    resvg::render(tree, transform, &mut pixmap.as_mut());

    let mut data = pixmap.take();
    tint(&mut data, color);

    Some(Icon {
        width: size.cast_signed(),
        height: size.cast_signed(),
        data
    })
}

/// Replaces premultiplied RGBA pixels with a BGRA silhouette in `color`.
///
/// Only the source alpha is meaningful: the silhouette shape. RGB channels are
/// rebuilt from the state color and premultiplied by that alpha.
fn tint(data: &mut [u8], color: Rgb) {
    for pixel in data.as_chunks_mut::<4>().0 {
        let alpha = pixel[3];
        pixel[0] = premultiply(color.b, alpha);
        pixel[1] = premultiply(color.g, alpha);
        pixel[2] = premultiply(color.r, alpha);
    }
}

/// Scales a color channel by alpha on a 0-255 scale.
fn premultiply(channel: u8, alpha: u8) -> u8 {
    #[allow(
        clippy::cast_possible_truncation,
        reason = "a product of two u8 values divided by 255 always fits in u8"
    )]
    let scaled = (u16::from(channel) * u16::from(alpha) / 255) as u8;
    scaled
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(
        connected: bool,
        level: u8,
        battery_status: &str,
        error: Option<&str>
    ) -> DeviceStatus {
        DeviceStatus {
            connected,
            battery_level: level,
            battery_status: battery_status.to_string(),
            error: error.map(str::to_string),
            ..DeviceStatus::default()
        }
    }

    #[test]
    fn error_takes_precedence() {
        let status = status(true, 5, BATTERY_DISCHARGING, Some("boom"));
        assert_eq!(color_for(&status), COLOR_ERROR);
    }

    #[test]
    fn disconnected_is_dim() {
        let status = status(false, 90, "Full", None);
        assert_eq!(color_for(&status), COLOR_DISCONNECTED);
    }

    #[test]
    fn healthy_connected_is_light() {
        let status = status(true, 90, "Discharging", None);
        assert_eq!(color_for(&status), COLOR_CONNECTED);
    }

    #[test]
    fn battery_thresholds_select_warning_colors() {
        assert_eq!(
            color_for(&status(true, 20, BATTERY_DISCHARGING, None)),
            COLOR_BATTERY_LOW
        );
        assert_eq!(
            color_for(&status(true, 10, BATTERY_DISCHARGING, None)),
            COLOR_BATTERY_CRITICAL
        );
        assert_eq!(
            color_for(&status(true, 11, BATTERY_DISCHARGING, None)),
            COLOR_BATTERY_LOW
        );
        assert_eq!(
            color_for(&status(true, 21, BATTERY_DISCHARGING, None)),
            COLOR_CONNECTED
        );
    }

    #[test]
    fn charging_suppresses_battery_colors() {
        let status = status(true, 5, "Charging", None);
        assert_eq!(color_for(&status), COLOR_CONNECTED);
    }

    #[test]
    fn unknown_battery_status_keeps_connected_color() {
        let status = status(true, 0, "Unknown", None);
        assert_eq!(color_for(&status), COLOR_CONNECTED);
    }

    #[test]
    fn tint_maps_alpha_to_premultiplied_bgra() {
        let color = Rgb {
            r: 10,
            g: 20,
            b: 30
        };
        let mut data = vec![
            0u8, 0, 0, 0, // transparent
            255, 255, 255, 255, // opaque
            255, 255, 255, 128, // half alpha
        ];
        tint(&mut data, color);

        assert_eq!(&data[0..4], &[0, 0, 0, 0]);
        assert_eq!(&data[4..8], &[30, 20, 10, 255]);
        assert_eq!(&data[8..12], &[15, 10, 5, 128]);
    }

    #[test]
    fn premultiply_scales_channels() {
        assert_eq!(premultiply(255, 0), 0);
        assert_eq!(premultiply(255, 255), 255);
        assert_eq!(premultiply(200, 128), 100);
    }

    #[test]
    fn render_publishes_all_sizes_with_shape() {
        let icons = render(COLOR_CONNECTED);
        assert_eq!(icons.len(), ICON_SIZES.len());

        for (icon, size) in icons.iter().zip(ICON_SIZES) {
            assert_eq!(icon.width.cast_unsigned(), size);
            assert_eq!(icon.height.cast_unsigned(), size);
            assert_eq!(icon.data.len(), (size * size * 4) as usize);

            let opaque = icon
                .data
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|px| px[3] == 255)
                .count();
            assert!(opaque > 0);

            assert_eq!(
                &icon.data[0..4],
                &[0, 0, 0, 0],
                "corner must be transparent"
            );
        }
    }

    #[test]
    fn cache_reuses_pixmaps_until_color_changes() {
        let cache = TrayIconCache::default();

        let first = cache.icons(COLOR_CONNECTED);
        let second = cache.icons(COLOR_CONNECTED);
        assert_eq!(first.len(), second.len());

        let third = cache.icons(COLOR_ERROR);
        assert_eq!(third.len(), first.len());
        assert_ne!(first[0].data, third[0].data);
    }
}
