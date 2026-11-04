// SPDX-FileCopyrightText: 2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! The live wheel preview: equal wedges painted from the ring being edited,
//! slot 0 at 12 o'clock, empty slots dimmed, and the click hit test that
//! selects a wedge.

use gtk4::cairo::{Context, FontSlant, FontWeight};
use logi_mx_daemon::pie::{
    geometry::{slot_for_angle, wedge_angle, wedge_for},
    labels::action_summary
};
use logi_mx_driver::config::PieSlot;

/// Wheel side length in logical pixels.
pub const WHEEL_SIZE: i32 = 260;
/// Radius of the transparent hole at the centre.
const INNER_RADIUS: f64 = 34.0;
/// Gap between the ring and the drawing area edge.
const OUTER_MARGIN: f64 = 8.0;
/// Sector label font and size.
const LABEL_FONT: &str = "Sans";
const LABEL_SIZE: f64 = 11.0;
/// Maximum characters painted inside a wedge.
const LABEL_MAX_CHARS: usize = 10;
/// Translucent dark fill of a bound wedge.
const FILL_BASE: (f64, f64, f64, f64) = (0.07, 0.08, 0.10, 0.80);
/// Dimmer fill of an empty slot.
const FILL_EMPTY: (f64, f64, f64, f64) = (0.05, 0.06, 0.07, 0.42);
/// Accent fill of the selected wedge.
const FILL_SELECTED: (f64, f64, f64, f64) = (53.0 / 255.0, 132.0 / 255.0, 228.0 / 255.0, 0.88);
/// Ring outline colour.
const OUTLINE: (f64, f64, f64, f64) = (0.91, 0.93, 0.96, 0.30);
/// Wedge label text colour.
const TEXT: (f64, f64, f64, f64) = (0.94, 0.96, 0.98, 0.95);
/// Label colour of an empty slot.
const TEXT_DIM: (f64, f64, f64, f64) = (0.94, 0.96, 0.98, 0.35);

/// Paints the preview ring: `ring.len()` equal wedges tile the circle, the
/// selected one wears the accent, empty slots stay dim with a dash. An empty
/// ring paints only a faint guide circle.
pub fn paint_ring(
    cr: &Context,
    width: f64,
    height: f64,
    ring: &[PieSlot],
    selected: Option<usize>,
    scope_hint: &str
) {
    let centre = width.min(height) / 2.0;
    let outer = (centre - OUTER_MARGIN).max(INNER_RADIUS + 1.0);
    let count = ring.len();
    if count == 0 {
        paint_empty_guide(cr, centre, outer);
        paint_hint(cr, centre, scope_hint);
        return;
    }
    for (index, slot) in ring.iter().enumerate() {
        let (start, end) = wedge_for(index, count);
        let (red, green, blue, alpha) = if selected == Some(index) {
            FILL_SELECTED
        } else if slot.is_bound() {
            FILL_BASE
        } else {
            FILL_EMPTY
        };
        cr.new_path();
        cr.arc(centre, centre, outer, start, end);
        cr.arc_negative(centre, centre, INNER_RADIUS, end, start);
        cr.close_path();
        cr.set_source_rgba(red, green, blue, alpha);
        let _ = cr.fill();
    }
    let (red, green, blue, alpha) = OUTLINE;
    cr.set_source_rgba(red, green, blue, alpha);
    cr.set_line_width(1.0);
    cr.new_sub_path();
    cr.arc(centre, centre, outer, 0.0, std::f64::consts::TAU);
    let _ = cr.stroke();
    cr.new_sub_path();
    cr.arc(centre, centre, INNER_RADIUS, 0.0, std::f64::consts::TAU);
    let _ = cr.stroke();
    paint_labels(cr, centre, outer, ring);
}

/// Faint dashed guide circle for a ring with no slots yet.
fn paint_empty_guide(cr: &Context, centre: f64, outer: f64) {
    let (red, green, blue, alpha) = OUTLINE;
    cr.set_source_rgba(red, green, blue, alpha);
    cr.set_line_width(2.0);
    cr.set_dash(&[6.0, 6.0], 0.0);
    cr.arc(
        centre,
        centre,
        f64::midpoint(outer, INNER_RADIUS),
        0.0,
        std::f64::consts::TAU
    );
    let _ = cr.stroke();
    cr.set_dash(&[], 0.0);
}

/// Paints the centred hint line inside the empty guide.
fn paint_hint(cr: &Context, centre: f64, hint: &str) {
    cr.select_font_face(LABEL_FONT, FontSlant::Normal, FontWeight::Normal);
    cr.set_font_size(LABEL_SIZE);
    let (red, green, blue, alpha) = TEXT_DIM;
    cr.set_source_rgba(red, green, blue, alpha);
    let first_line: String = hint.chars().take(24).collect();
    let Ok(extents) = cr.text_extents(&first_line) else {
        return;
    };
    cr.move_to(centre - extents.width() / 2.0, centre);
    let _ = cr.show_text(&first_line);
}

/// Draws one truncated summary per wedge, mid-band; empty slots show a dash.
fn paint_labels(cr: &Context, centre: f64, outer: f64, ring: &[PieSlot]) {
    cr.select_font_face(LABEL_FONT, FontSlant::Normal, FontWeight::Normal);
    cr.set_font_size(LABEL_SIZE);
    let radius = f64::midpoint(outer, INNER_RADIUS);
    let count = ring.len();
    for (index, slot) in ring.iter().enumerate() {
        let raw = slot
            .label
            .clone()
            .unwrap_or_else(|| slot.action.as_ref().map_or(String::new(), action_summary));
        let mut label: String = raw.chars().take(LABEL_MAX_CHARS).collect();
        if raw.chars().count() > LABEL_MAX_CHARS {
            label.push('…');
        }
        let (red, green, blue, alpha) = if slot.is_bound() { TEXT } else { TEXT_DIM };
        cr.set_source_rgba(red, green, blue, alpha);
        let text = if label.is_empty() {
            "—".to_string()
        } else {
            label
        };
        let middle = wedge_angle(index, count);
        let Ok(extents) = cr.text_extents(&text) else {
            continue;
        };
        let x = radius.mul_add(middle.cos(), centre) - extents.x_bearing() - extents.width() / 2.0;
        let y =
            radius.mul_add(middle.sin(), centre) - extents.y_bearing() - extents.height() / 2.0;
        cr.move_to(x, y);
        let _ = cr.show_text(&text);
    }
}

/// Maps a click position to the wedge under it, `None` inside the centre
/// hole or outside the ring. The angle maths is the daemon's own
/// [`slot_for_angle`], so the preview and the overlay select the same wedge.
#[must_use]
pub fn slot_at(x: f64, y: f64, size: f64, count: usize) -> Option<usize> {
    let centre = size / 2.0;
    let outer = centre - OUTER_MARGIN;
    let (dx, dy) = (x - centre, y - centre);
    let distance = dx.hypot(dy);
    if distance < INNER_RADIUS || distance > outer {
        return None;
    }
    slot_for_angle(dy.atan2(dx), count)
}

#[cfg(test)]
#[path = "preview_tests.rs"]
mod tests;
