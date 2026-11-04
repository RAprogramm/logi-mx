// SPDX-FileCopyrightText: 2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Cairo painting of the radial pie ring.
//!
//! The renderer draws only what the daemon streamed: while a hold charges, a
//! thin ring fills clockwise around the anchor; once the menu opens, the
//! bound sectors tile the full circle around the logical anchor, the
//! highlighted sector wears the accent colour, and the whole menu blooms in
//! with a short scale-and-fade animation. Colours are fixed constants so the
//! ring looks identical on every theme and compositor.

use gtk4::cairo::{Context, FontSlant, FontWeight, LineCap};

use crate::scene::{PieScene, truncate_label, wedge_for};

/// Inner ring radius in logical pixels, mirroring the daemon dead zone.
pub const RING_INNER: f64 = 24.0;
/// Outer ring radius in logical pixels.
pub const RING_OUTER: f64 = 150.0;
/// Radius of the thin charge ring in logical pixels.
const CHARGE_RADIUS: f64 = 34.0;
/// Stroke width of the charge ring.
const CHARGE_WIDTH: f64 = 5.0;
/// Centre dot radius in logical pixels.
const CENTER_DOT: f64 = 4.0;
/// Stroke width of the ring outline.
const OUTLINE_WIDTH: f64 = 1.5;
/// Font family for sector labels.
const LABEL_FONT: &str = "Sans";
/// Sector label size in logical pixels.
const LABEL_SIZE: f64 = 14.0;
/// Maximum label length before truncation with an ellipsis.
const LABEL_MAX_CHARS: usize = 12;
/// Fraction of the ring band where labels sit, between inner and outer edge.
const LABEL_BAND: f64 = 0.5;
/// Scale the menu starts its bloom at, growing to 1.0.
const BLOOM_FROM: f64 = 0.80;
/// Translucent dark fill of an unbound-highlighted sector.
const FILL_BASE: (f64, f64, f64, f64) = (0.07, 0.08, 0.10, 0.78);
/// Accent fill of the highlighted sector.
const FILL_HIGHLIGHT: (f64, f64, f64, f64) = (53.0 / 255.0, 132.0 / 255.0, 228.0 / 255.0, 0.85);
/// Thin ring outline colour.
const OUTLINE: (f64, f64, f64, f64) = (0.91, 0.93, 0.96, 0.35);
/// Sector label text colour.
const TEXT: (f64, f64, f64, f64) = (0.94, 0.96, 0.98, 0.95);
/// Centre dot colour, the same accent as the highlight.
const DOT: (f64, f64, f64, f64) = (53.0 / 255.0, 132.0 / 255.0, 228.0 / 255.0, 0.95);
/// Faint track behind the charge fill.
const CHARGE_TRACK: (f64, f64, f64, f64) = (0.92, 0.94, 0.97, 0.22);
/// Accent of the charge fill, the same as the highlight.
const CHARGE_FILL: (f64, f64, f64, f64) = (53.0 / 255.0, 132.0 / 255.0, 228.0 / 255.0, 0.95);

/// Paints the pie for one monitor window, if the scene targets it.
///
/// The window covers its whole monitor, so the scene's logical anchor is
/// already in this surface's coordinates. An inactive scene, or a scene on
/// another monitor, paints nothing and leaves the surface transparent.
///
/// # Arguments
///
/// * `cr` - Cairo context of the monitor's drawing surface.
/// * `monitor_index` - Index of this window's monitor in the scene layout.
/// * `scene` - Current folded renderer state.
/// * `progress` - Animation progress in `0..=1`: how far the charge ring has
///   filled while charging, or how far the menu has bloomed once open.
///
/// # Examples
///
/// ```
/// use logi_mx_overlay::{draw::targets, scene::PieScene};
///
/// assert!(
///     !targets(0, &PieScene::default()),
///     "an idle scene paints nothing"
/// );
/// ```
pub fn render(cr: &Context, monitor_index: usize, scene: &PieScene, progress: f64) {
    if !targets(monitor_index, scene) {
        return;
    }
    let (cx, cy) = scene.anchor;
    let progress = progress.clamp(0.0, 1.0);
    if scene.charging {
        paint_charge(cr, cx, cy, progress);
        return;
    }
    let _ = cr.save();
    cr.translate(cx, cy);
    let scale = (1.0 - BLOOM_FROM).mul_add(progress, BLOOM_FROM);
    cr.scale(scale, scale);
    cr.translate(-cx, -cy);
    cr.push_group();
    paint_sectors(cr, cx, cy, scene);
    paint_outline(cr, cx, cy);
    paint_labels(cr, cx, cy, scene);
    paint_dot(cr, cx, cy);
    let _ = cr.pop_group_to_source();
    let _ = cr.paint_with_alpha(progress);
    let _ = cr.restore();
}

/// Whether a scene should be painted on a given monitor window.
///
/// # Arguments
///
/// * `monitor_index` - Index of the window's monitor.
/// * `scene` - Current folded renderer state.
///
/// # Returns
///
/// `true` only for an active scene anchored on this monitor.
#[must_use]
pub const fn targets(monitor_index: usize, scene: &PieScene) -> bool {
    scene.is_active() && scene.monitor == monitor_index
}

/// Draws the thin hold-gate ring: a faint track and an accent arc that fills
/// clockwise from the top as `progress` grows.
fn paint_charge(cr: &Context, cx: f64, cy: f64, progress: f64) {
    cr.set_line_width(CHARGE_WIDTH);
    cr.set_line_cap(LineCap::Round);
    let (red, green, blue, alpha) = CHARGE_TRACK;
    cr.set_source_rgba(red, green, blue, alpha);
    cr.new_sub_path();
    cr.arc(cx, cy, CHARGE_RADIUS, 0.0, std::f64::consts::TAU);
    let _ = cr.stroke();
    if progress <= 0.0 {
        return;
    }
    let start = -std::f64::consts::FRAC_PI_2;
    let (red, green, blue, alpha) = CHARGE_FILL;
    cr.set_source_rgba(red, green, blue, alpha);
    cr.new_sub_path();
    cr.arc(
        cx,
        cy,
        CHARGE_RADIUS,
        start,
        std::f64::consts::TAU.mul_add(progress, start)
    );
    let _ = cr.stroke();
}

/// Fills every bound sector; wedges tile the full circle, so no gap is left
/// between sections regardless of how many directions are bound.
fn paint_sectors(cr: &Context, cx: f64, cy: f64, scene: &PieScene) {
    let count = scene.sectors.len();
    for sector in &scene.sectors {
        let Some((start, end)) = wedge_for(sector.direction, count) else {
            continue;
        };
        let highlighted = scene.highlight == Some(sector.direction);
        let (red, green, blue, alpha) = if highlighted {
            FILL_HIGHLIGHT
        } else {
            FILL_BASE
        };
        cr.new_path();
        cr.move_to(cx, cy);
        cr.arc(cx, cy, RING_OUTER, start, end);
        cr.close_path();
        cr.set_source_rgba(red, green, blue, alpha);
        let _ = cr.fill();
    }
}

/// Strokes the inner and outer ring circles.
fn paint_outline(cr: &Context, cx: f64, cy: f64) {
    let (red, green, blue, alpha) = OUTLINE;
    cr.set_line_width(OUTLINE_WIDTH);
    cr.set_source_rgba(red, green, blue, alpha);
    cr.new_sub_path();
    cr.arc(cx, cy, RING_OUTER, 0.0, std::f64::consts::TAU);
    let _ = cr.stroke();
    cr.new_sub_path();
    cr.arc(cx, cy, RING_INNER, 0.0, std::f64::consts::TAU);
    let _ = cr.stroke();
}

/// Draws one centred label per bound sector, mid-band in its wedge.
fn paint_labels(cr: &Context, cx: f64, cy: f64, scene: &PieScene) {
    cr.select_font_face(LABEL_FONT, FontSlant::Normal, FontWeight::Normal);
    cr.set_font_size(LABEL_SIZE);
    let (red, green, blue, alpha) = TEXT;
    cr.set_source_rgba(red, green, blue, alpha);
    let radius = (RING_OUTER - RING_INNER).mul_add(LABEL_BAND, RING_INNER);
    let count = scene.sectors.len();
    for sector in &scene.sectors {
        let Some((start, end)) = wedge_for(sector.direction, count) else {
            continue;
        };
        let middle = f64::midpoint(start, end);
        let label = truncate_label(&sector.label, LABEL_MAX_CHARS);
        if label.is_empty() {
            continue;
        }
        let Ok(extents) = cr.text_extents(&label) else {
            continue;
        };
        let x = radius.mul_add(middle.cos(), cx) - extents.x_bearing() - extents.width() / 2.0;
        let y = radius.mul_add(middle.sin(), cy) - extents.y_bearing() - extents.height() / 2.0;
        cr.move_to(x, y);
        let _ = cr.show_text(&label);
    }
}

/// Paints the accent dot at the anchor.
fn paint_dot(cr: &Context, cx: f64, cy: f64) {
    let (red, green, blue, alpha) = DOT;
    cr.new_sub_path();
    cr.arc(cx, cy, CENTER_DOT, 0.0, std::f64::consts::TAU);
    cr.set_source_rgba(red, green, blue, alpha);
    let _ = cr.fill();
}
