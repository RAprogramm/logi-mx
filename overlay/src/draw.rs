// SPDX-FileCopyrightText: 2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Cairo painting of the radial pie ring.
//!
//! The renderer draws only what the daemon streamed: while a hold charges, a
//! thin ring fills clockwise around the anchor; once the menu opens, the
//! bound sectors tile the full circle around the logical anchor, the
//! highlighted sector wears the accent colour, and the whole menu blooms out
//! of the charge ring with a springy overshoot and leaves with a quick
//! shrink-and-fade. Colours are fixed constants so the ring looks identical
//! on every theme and compositor.

use gtk4::cairo::{Context, FontSlant, FontWeight, LineCap, RadialGradient};

use crate::{
    icons::{self, ICON_SIZE, IconCache},
    scene::{PieScene, truncate_label, wedge_for}
};

/// Smoothed motion values the host computes per frame: where the aim needle
/// points and where the highlight wedge currently sits with its opacity.
#[derive(Debug, Default, Clone, Copy)]
pub struct Motion {
    /// Needle angle in radians, `None` hides the needle.
    pub needle:    Option<f64>,
    /// Highlight wedge centre angle and its alpha, `None` hides the wedge.
    pub highlight: Option<(f64, f64)>
}

/// Inner ring radius in logical pixels, mirroring the daemon dead zone.
pub const RING_INNER: f64 = 18.0;
/// Outer ring radius in logical pixels.
pub const RING_OUTER: f64 = 104.0;
/// Radius of the thin charge ring in logical pixels.
const CHARGE_RADIUS: f64 = 26.0;
/// Stroke width of the charge ring.
const CHARGE_WIDTH: f64 = 4.0;
/// Centre dot radius in logical pixels.
const CENTER_DOT: f64 = 3.5;
/// Stroke width of the aim needle.
const NEEDLE_WIDTH: f64 = 3.0;
/// Distance from the centre where the needle starts.
const NEEDLE_INNER: f64 = 10.0;
/// Distance from the ring edge where the needle tip stops.
const NEEDLE_TIP_INSET: f64 = 8.0;
/// Radius of the needle tip dot.
const NEEDLE_TIP_RADIUS: f64 = 3.0;
/// Stroke width of the ring outline.
const OUTLINE_WIDTH: f64 = 1.0;
/// Stroke width of the hairline between neighbouring sectors.
const SEPARATOR_WIDTH: f64 = 1.0;
/// Font family for sector labels.
const LABEL_FONT: &str = "Sans";
/// Sector label size in logical pixels.
const LABEL_SIZE: f64 = 12.0;
/// Maximum label length before truncation with an ellipsis.
const LABEL_MAX_CHARS: usize = 12;
/// Fraction of the ring band where labels sit, between inner and outer edge.
const LABEL_BAND: f64 = 0.55;
/// Horizontal breathing room a label keeps from the wedge borders.
const LABEL_SIDE_PAD: f64 = 10.0;
/// Sector icon side in logical pixels, mirroring [`ICON_SIZE`].
const ICON_SIDE: f64 = ICON_SIZE as f64;
/// Gap between a sector icon and its label.
const ICON_GAP: f64 = 3.0;
/// Half the icon-plus-gap block, the offset label and icon keep from the
/// band centre so the pair reads as one block.
const ICON_BLOCK: f64 = f64::midpoint(ICON_SIDE, ICON_GAP);
/// Scale the menu starts its bloom at, growing out of the charge ring.
const BLOOM_FROM: f64 = 0.30;
/// Overshoot of the springy bloom, in units of the remaining scale.
const BLOOM_OVERSHOOT: f64 = 0.55;
/// Fraction of the bloom spent fading in, so the menu is opaque early.
const BLOOM_FADE: f64 = 0.40;
/// Scale the menu shrinks to as it exits.
const EXIT_TO: f64 = 0.90;
/// Translucent dark fill of an unbound-highlighted sector.
const FILL_BASE: (f64, f64, f64, f64) = (0.07, 0.08, 0.10, 0.80);
/// Accent fill of the highlighted sector.
const FILL_HIGHLIGHT: (f64, f64, f64, f64) = (53.0 / 255.0, 132.0 / 255.0, 228.0 / 255.0, 0.88);
/// Thin ring outline colour.
const OUTLINE: (f64, f64, f64, f64) = (0.91, 0.93, 0.96, 0.30);
/// Hairline between neighbouring sectors.
const SEPARATOR: (f64, f64, f64, f64) = (0.02, 0.03, 0.04, 0.55);
/// Sector label text colour.
const TEXT: (f64, f64, f64, f64) = (0.94, 0.96, 0.98, 0.95);
/// Centre dot colour, the same accent as the highlight.
const DOT: (f64, f64, f64, f64) = (53.0 / 255.0, 132.0 / 255.0, 228.0 / 255.0, 0.95);
/// Faint track behind the charge fill.
const CHARGE_TRACK: (f64, f64, f64, f64) = (0.92, 0.94, 0.97, 0.22);
/// Accent of the charge fill, the same as the highlight.
const CHARGE_FILL: (f64, f64, f64, f64) = (53.0 / 255.0, 132.0 / 255.0, 228.0 / 255.0, 0.95);
/// Inner edge of the soft drop shadow, just inside the ring.
const SHADOW_INNER: f64 = RING_OUTER - 6.0;
/// Outer edge of the soft drop shadow.
const SHADOW_OUTER: f64 = RING_OUTER + 16.0;
/// Darkest alpha of the drop shadow.
const SHADOW_ALPHA: f64 = 0.32;

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
/// * `progress` - Linear animation progress in `0..=1`: how far the charge ring
///   has filled while charging, or how far the menu has bloomed.
/// * `motion` - Smoothed needle and highlight positions for this frame.
/// * `icons` - Cached sector icons resolved from the bound commands.
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
pub fn render(
    cr: &Context,
    monitor_index: usize,
    scene: &PieScene,
    progress: f64,
    motion: &Motion,
    icons: &IconCache
) {
    if !targets(monitor_index, scene) {
        return;
    }
    let (cx, cy) = scene.anchor;
    let progress = progress.clamp(0.0, 1.0);
    if scene.charging {
        paint_charge(cr, cx, cy, progress);
        return;
    }
    let scale = (1.0 - BLOOM_FROM).mul_add(ease_out_back(progress), BLOOM_FROM);
    let alpha = (progress / BLOOM_FADE).min(1.0);
    paint_bloomed(cr, scene, scale, alpha, motion, icons);
}

/// Paints the exit of a menu that already closed: shrinking away and fading
/// out with an accelerating ease-in.
///
/// # Arguments
///
/// * `cr` - Cairo context of the monitor's drawing surface.
/// * `monitor_index` - Index of this window's monitor.
/// * `scene` - Snapshot of the menu as it was when it closed.
/// * `progress` - Linear exit progress in `0..=1`.
/// * `motion` - Needle and highlight positions frozen at close.
/// * `icons` - Cached sector icons resolved from the bound commands.
pub fn render_exit(
    cr: &Context,
    monitor_index: usize,
    scene: &PieScene,
    progress: f64,
    motion: &Motion,
    icons: &IconCache
) {
    if !targets(monitor_index, scene) {
        return;
    }
    let eased = progress.clamp(0.0, 1.0);
    let eased = eased * eased;
    let scale = (EXIT_TO - 1.0).mul_add(eased, 1.0);
    paint_bloomed(cr, scene, scale, 1.0 - eased, motion, icons);
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

/// Draws the menu at a given scale and opacity: shadow, sectors, the sliding
/// highlight, hairlines, the aim needle, outline, labels and the centre dot,
/// all inside one transform so the bloom and the exit move as a single
/// object.
fn paint_bloomed(
    cr: &Context,
    scene: &PieScene,
    scale: f64,
    alpha: f64,
    motion: &Motion,
    icons: &IconCache
) {
    let (cx, cy) = scene.anchor;
    let _ = cr.save();
    cr.translate(cx, cy);
    cr.scale(scale, scale);
    cr.translate(-cx, -cy);
    cr.push_group();
    paint_shadow(cr, cx, cy);
    paint_sectors(cr, cx, cy, scene);
    if let Some((angle, highlight_alpha)) = motion.highlight {
        paint_highlight(cr, cx, cy, scene.sectors.len(), angle, highlight_alpha);
    }
    paint_separators(cr, cx, cy, scene);
    if let Some(angle) = motion.needle {
        paint_needle(cr, cx, cy, angle);
    }
    paint_outline(cr, cx, cy);
    paint_labels(cr, cx, cy, scene, icons);
    paint_dot(cr, cx, cy);
    let _ = cr.pop_group_to_source();
    let _ = cr.paint_with_alpha(alpha);
    let _ = cr.restore();
}

/// Fills a soft radial halo behind the ring for depth.
fn paint_shadow(cr: &Context, cx: f64, cy: f64) {
    let gradient = RadialGradient::new(cx, cy, SHADOW_INNER, cx, cy, SHADOW_OUTER);
    gradient.add_color_stop_rgba(0.0, 0.0, 0.0, 0.0, SHADOW_ALPHA);
    gradient.add_color_stop_rgba(1.0, 0.0, 0.0, 0.0, 0.0);
    let _ = cr.set_source(&gradient);
    cr.new_sub_path();
    cr.arc(cx, cy, SHADOW_OUTER, 0.0, std::f64::consts::TAU);
    let _ = cr.fill();
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

/// Fills every bound sector in the base colour; wedges tile the full circle,
/// so no gap is left between sections regardless of how many directions are
/// bound. The highlight is a separate sliding wedge.
fn paint_sectors(cr: &Context, cx: f64, cy: f64, scene: &PieScene) {
    let (red, green, blue, alpha) = FILL_BASE;
    cr.set_source_rgba(red, green, blue, alpha);
    let count = scene.sectors.len();
    for sector in &scene.sectors {
        let Some((start, end)) = wedge_for(sector.direction, count) else {
            continue;
        };
        cr.new_path();
        cr.move_to(cx, cy);
        cr.arc(cx, cy, RING_OUTER, start, end);
        cr.close_path();
        let _ = cr.fill();
    }
}

/// Fills the highlight wedge centred on `angle`, the host having animated
/// the slide from the previous sector, so selection glides instead of
/// jumping.
fn paint_highlight(cr: &Context, cx: f64, cy: f64, count: usize, angle: f64, alpha: f64) {
    if alpha <= 0.0 || count == 0 {
        return;
    }
    let Ok(sectors) = u32::try_from(count) else {
        return;
    };
    let half = std::f64::consts::PI / f64::from(sectors);
    let (red, green, blue, base) = FILL_HIGHLIGHT;
    cr.set_source_rgba(red, green, blue, base * alpha);
    cr.new_path();
    cr.move_to(cx, cy);
    cr.arc(cx, cy, RING_OUTER, angle - half, angle + half);
    cr.close_path();
    let _ = cr.fill();
}

/// Draws the aim needle: a tapered accent line from near the centre to the
/// ring edge at the exact cursor angle, so the user always sees where they
/// point inside a sector, plus a tip dot.
fn paint_needle(cr: &Context, cx: f64, cy: f64, angle: f64) {
    let (red, green, blue, alpha) = CHARGE_FILL;
    cr.set_source_rgba(red, green, blue, alpha);
    cr.set_line_width(NEEDLE_WIDTH);
    cr.set_line_cap(LineCap::Round);
    cr.new_path();
    cr.move_to(
        NEEDLE_INNER.mul_add(angle.cos(), cx),
        NEEDLE_INNER.mul_add(angle.sin(), cy)
    );
    cr.line_to(
        (RING_OUTER - NEEDLE_TIP_INSET).mul_add(angle.cos(), cx),
        (RING_OUTER - NEEDLE_TIP_INSET).mul_add(angle.sin(), cy)
    );
    let _ = cr.stroke();
    cr.new_sub_path();
    cr.arc(
        (RING_OUTER - NEEDLE_TIP_INSET).mul_add(angle.cos(), cx),
        (RING_OUTER - NEEDLE_TIP_INSET).mul_add(angle.sin(), cy),
        NEEDLE_TIP_RADIUS,
        0.0,
        std::f64::consts::TAU
    );
    let _ = cr.fill();
}

/// Strokes the hairline where each sector begins, keeping neighbours crisp
/// without leaving empty gaps between them.
fn paint_separators(cr: &Context, cx: f64, cy: f64, scene: &PieScene) {
    let count = scene.sectors.len();
    let (red, green, blue, alpha) = SEPARATOR;
    cr.set_source_rgba(red, green, blue, alpha);
    cr.set_line_width(SEPARATOR_WIDTH);
    for sector in &scene.sectors {
        let Some((start, _)) = wedge_for(sector.direction, count) else {
            continue;
        };
        cr.new_path();
        cr.move_to(
            RING_INNER.mul_add(start.cos(), cx),
            RING_INNER.mul_add(start.sin(), cy)
        );
        cr.line_to(
            RING_OUTER.mul_add(start.cos(), cx),
            RING_OUTER.mul_add(start.sin(), cy)
        );
        let _ = cr.stroke();
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
/// Draws one content block per bound sector, mid-band in its wedge: the
/// application icon when the command resolved to one, the truncated label
/// under it, or just the label centred when there is no icon.
fn paint_labels(cr: &Context, cx: f64, cy: f64, scene: &PieScene, icons: &IconCache) {
    cr.select_font_face(LABEL_FONT, FontSlant::Normal, FontWeight::Normal);
    cr.set_font_size(LABEL_SIZE);
    let radius = (RING_OUTER - RING_INNER).mul_add(LABEL_BAND, RING_INNER);
    let count = scene.sectors.len();
    let scale = f64::from(scene.scale.max(1));
    let px = ICON_SIZE.saturating_mul(scene.scale.max(1));
    for sector in &scene.sectors {
        let Some((start, end)) = wedge_for(sector.direction, count) else {
            continue;
        };
        let middle = f64::midpoint(start, end);
        let bx = radius.mul_add(middle.cos(), cx);
        let by = radius.mul_add(middle.sin(), cy);
        let label = truncate_label(&sector.label, LABEL_MAX_CHARS);
        let Ok(sectors) = u32::try_from(count.max(1)) else {
            continue;
        };
        let half_wedge = std::f64::consts::PI / f64::from(sectors);
        let chord = (2.0 * radius).mul_add(half_wedge.sin(), -LABEL_SIDE_PAD);
        let label = fit_to_width(cr, &label, chord.max(0.0));
        let icon = icons.surface(sector.command.as_deref(), px);
        if let Some(surface) = icon {
            let icon_cy = if label.is_empty() {
                by
            } else {
                by - ICON_BLOCK
            };
            icons::paint_icon(cr, surface, bx, icon_cy, scale);
        }
        if label.is_empty() {
            continue;
        }
        let label_cy = if icon.is_some() { by + ICON_BLOCK } else { by };
        let (red, green, blue, alpha) = TEXT;
        cr.set_source_rgba(red, green, blue, alpha);
        let Ok(extents) = cr.text_extents(&label) else {
            continue;
        };
        let x = bx - extents.x_bearing() - extents.width() / 2.0;
        let y = label_cy - extents.y_bearing() - extents.height() / 2.0;
        cr.move_to(x, y);
        let _ = cr.show_text(&label);
    }
}

/// Shortens `label` with an ellipsis until its measured width fits `max_px`.
///
/// The font must already be selected on `cr`; the label is returned
/// unchanged when it fits or measurement fails.
fn fit_to_width(cr: &Context, label: &str, max_px: f64) -> String {
    let Ok(extents) = cr.text_extents(label) else {
        return label.to_string();
    };
    if extents.width() <= max_px {
        return label.to_string();
    }
    let mut keep = label.chars().count();
    while keep > 1 {
        keep -= 1;
        let candidate: String = label.chars().take(keep).collect();
        let Ok(measured) = cr.text_extents(&format!("{candidate}…")) else {
            break;
        };
        if measured.width() <= max_px {
            return format!("{candidate}…");
        }
    }
    "…".to_string()
}

/// Paints the accent dot at the anchor.
fn paint_dot(cr: &Context, cx: f64, cy: f64) {
    let (red, green, blue, alpha) = DOT;
    cr.new_sub_path();
    cr.arc(cx, cy, CENTER_DOT, 0.0, std::f64::consts::TAU);
    cr.set_source_rgba(red, green, blue, alpha);
    let _ = cr.fill();
}

/// Ease-out-back: accelerates to the target and overshoots slightly before
/// settling, the springy entrance recommended for scale animations.
fn ease_out_back(progress: f64) -> f64 {
    let c1 = BLOOM_OVERSHOOT;
    let c3 = c1 + 1.0;
    let shifted = progress - 1.0;
    let squared = shifted * shifted;
    c3.mul_add(shifted, c1).mul_add(squared, 1.0)
}
