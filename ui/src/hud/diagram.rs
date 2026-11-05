// SPDX-FileCopyrightText: 2025-2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Device diagram rendering: SVG rasterization, button alpha masks, cairo
//! painting of highlights and presses, the live-frame wheel overlay and the
//! leader lines from diagram anchors to the settings rows.

use std::{
    cell::{OnceCell, RefCell},
    collections::HashMap,
    rc::Rc,
    sync::OnceLock
};

use libadwaita::{ApplicationWindow, prelude::*};

use super::{HudState, MouseButton, ROWS_REGISTRY};

/// Device layout embedded from `assets/logitech-mx-master-3s.svg`.
const DEVICE_SVG: &str = include_str!("../../assets/logitech-mx-master-3s.svg");
/// Width of the device layout coordinate space.
const LAYOUT_W: f64 = 128.32296;
/// Height of the device layout coordinate space.
const LAYOUT_H: f64 = 117.73977;
/// Vertical offset of the device group inside the layout SVG.
const DEVICE_GROUP_DY: f64 = -179.26022;
/// Width the SVG is rasterized to once and cached, in pixels.
const SVG_RENDER_WIDTH: u32 = 1024;
/// Scroll velocity that saturates the wheel chevrons, in notches/s.
const WHEEL_FULL_SCALE: f64 = 25.0;

/// Wires the diagram draw callback: base image, button masks and the live
/// wheel overlay, painted in one pass.
pub(super) fn attach_draw_callback(
    state: &Rc<RefCell<HudState>>,
    drawing: &gtk4::DrawingArea,
    image: &Rc<OnceCell<Option<gtk4::cairo::ImageSurface>>>,
    masks: &Rc<ButtonMasks>
) {
    let state_draw = Rc::clone(state);
    let image_draw = Rc::clone(image);
    let masks_draw = Rc::clone(masks);
    drawing.set_draw_func(move |_, cr, width, height| {
        let st = state_draw.borrow();
        let (scale, offset_x, offset_y) = fit_transform(f64::from(width), f64::from(height));

        let _ = cr.save();
        cr.translate(offset_x, offset_y);
        cr.scale(scale, scale);

        let surface_scale = image_draw
            .get_or_init(render_device_svg)
            .as_ref()
            .map(|surface| {
                let sx = LAYOUT_W / f64::from(surface.width());
                let sy = LAYOUT_H / f64::from(surface.height());
                let _ = cr.save();
                cr.scale(sx, sy);
                let _ = cr.set_source_surface(surface, 0.0, 0.0);
                let _ = cr.paint();
                let _ = cr.restore();
                (sx, sy)
            });

        if let Some((sx, sy)) = surface_scale {
            draw_selection(&st, cr, sx, sy, &masks_draw);
            draw_pressed_shapes(&st, cr, sx, sy, &masks_draw.fill);
        }
        let _ = cr.restore();

        draw_wheel_indicator(&st, cr, scale, offset_x, offset_y);
    });
}

/// Rasterizes an SVG document once into a cairo surface sized to the layout.
fn render_svg_to_surface(svg: &[u8]) -> Option<gtk4::cairo::ImageSurface> {
    let tree = usvg::Tree::from_data(svg, &usvg::Options::default()).ok()?;
    let size = tree.size();
    #[allow(
        clippy::cast_precision_loss,
        reason = "the raster width constant is exactly representable as f32"
    )]
    let scale = SVG_RENDER_WIDTH as f32 / size.width();
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "the raster height is clamped to a sane pixel range"
    )]
    let height = (size.height() * scale).round().clamp(1.0, 8192.0) as u32;
    let width = SVG_RENDER_WIDTH;

    let mut pixmap = resvg::tiny_skia::Pixmap::new(width, height)?;
    let transform = resvg::tiny_skia::Transform::from_scale(scale, scale);
    resvg::render(&tree, transform, &mut pixmap.as_mut());

    let mut data = pixmap.take();
    for pixel in data.as_chunks_mut::<4>().0 {
        pixel.swap(0, 2);
    }

    gtk4::cairo::ImageSurface::create_for_data(
        data,
        gtk4::cairo::Format::ARgb32,
        width.cast_signed(),
        height.cast_signed(),
        (width * 4).cast_signed()
    )
    .ok()
}

/// Rasterizes the embedded device SVG once into a cairo surface.
fn render_device_svg() -> Option<gtk4::cairo::ImageSurface> {
    render_svg_to_surface(DEVICE_SVG.as_bytes())
}

/// Builds a standalone SVG containing only the device element with the
/// given id, injecting `style` presentation attributes into its tag.
///
/// The layout SVG keeps button shapes attribute-free, so injected `fill`
/// and `stroke` attributes always apply without conflicts.
fn device_element_svg_styled(id: &str, style: &str) -> Option<String> {
    let marker = format!("id=\"{id}\"");
    let at = DEVICE_SVG.find(&marker)?;
    let open = DEVICE_SVG[..at].rfind('<')?;
    let close = DEVICE_SVG[at..].find("/>")? + at + 2;
    let element = &DEVICE_SVG[open..close];
    let element = if style.is_empty() {
        std::borrow::Cow::Borrowed(element)
    } else {
        std::borrow::Cow::Owned(element.replacen("/>", &format!(" {style}/>"), 1))
    };
    Some(format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {LAYOUT_W} {LAYOUT_H}\">\
         <g transform=\"translate(0,{DEVICE_GROUP_DY})\">{element}</g></svg>"
    ))
}

/// Builds a standalone SVG containing only the device element with the
/// given id, so it can be rendered as an alpha mask aligned with the base
/// image.
fn device_element_svg(id: &str) -> Option<String> {
    device_element_svg_styled(id, "")
}

/// Alpha masks of every button shape, rendered once from the layout SVG.
#[derive(Default)]
pub(super) struct ButtonMasks {
    /// Exact shape of each button, tinted for highlights and presses.
    fill:    HashMap<MouseButton, gtk4::cairo::ImageSurface>,
    /// Contour of each button, painted as the selection outline.
    outline: HashMap<MouseButton, gtk4::cairo::ImageSurface>
}

/// Stroke width of the selection outline, in layout units.
const OUTLINE_STROKE: f64 = 0.5;

/// Renders the fill and outline alpha masks of every button shape.
pub(super) fn build_button_masks() -> ButtonMasks {
    let mut masks = ButtonMasks::default();
    let outline_style =
        format!("fill=\"none\" stroke=\"#000000\" stroke-width=\"{OUTLINE_STROKE}\"");
    for button in MouseButton::ALL {
        if let Some(svg) = device_element_svg(button.element_id())
            && let Some(surface) = render_svg_to_surface(svg.as_bytes())
        {
            masks.fill.insert(button, surface);
        }
        if let Some(svg) = device_element_svg_styled(button.element_id(), &outline_style)
            && let Some(surface) = render_svg_to_surface(svg.as_bytes())
        {
            masks.outline.insert(button, surface);
        }
    }
    masks
}

/// Computes uniform scale and centering offsets for the layout space.
/// The mouse is fitted and centered exactly as in the original design.
fn fit_transform(width: f64, height: f64) -> (f64, f64, f64) {
    let scale = (width / LAYOUT_W).min(height / LAYOUT_H);
    let content_w = LAYOUT_W * scale;
    let content_h = LAYOUT_H * scale;
    (scale, (width - content_w) / 2.0, (height - content_h) / 2.0)
}

/// Maps widget pixel coordinates into the device layout space.
pub(super) fn to_layout(px: f64, py: f64, width: i32, height: i32) -> (f64, f64) {
    let (scale, offset_x, offset_y) = fit_transform(f64::from(width), f64::from(height));
    ((px - offset_x) / scale, (py - offset_y) / scale)
}

/// Paints one button's alpha mask with a flat color, aligned to the base
/// image through the raster-to-layout scale.
fn paint_button_shape(
    cr: &gtk4::cairo::Context,
    sx: f64,
    sy: f64,
    mask: &gtk4::cairo::ImageSurface,
    color: (f64, f64, f64, f64)
) {
    let pattern = gtk4::cairo::SurfacePattern::create(mask);
    let _ = cr.save();
    cr.scale(sx, sy);
    cr.set_source_rgba(color.0, color.1, color.2, color.3);
    let _ = cr.mask(&pattern);
    let _ = cr.restore();
}

/// Highlight colors: hover and selection share the accent, assignments are
/// tinted blue.
const ACCENT: (f64, f64, f64) = (0.93, 0.66, 0.46);
const ASSIGNED: (f64, f64, f64) = (0.42, 0.60, 0.80);

/// Highlights hovered, selected and assigned buttons by tinting their exact
/// SVG shapes; the selection additionally carries a contour outline.
fn draw_selection(
    st: &HudState,
    cr: &gtk4::cairo::Context,
    sx: f64,
    sy: f64,
    masks: &ButtonMasks
) {
    for button in MouseButton::ALL {
        let selected = st.selected == Some(button);
        let hovered = st.hover == Some(button);
        let assigned = st.assignments.contains_key(&button);
        if !selected && !hovered && !assigned {
            continue;
        }

        let Some(fill) = masks.fill.get(&button) else {
            continue;
        };
        let color = if selected || hovered {
            (ACCENT.0, ACCENT.1, ACCENT.2, 0.55)
        } else {
            (ASSIGNED.0, ASSIGNED.1, ASSIGNED.2, 0.50)
        };
        paint_button_shape(cr, sx, sy, fill, color);

        if selected && let Some(outline) = masks.outline.get(&button) {
            paint_button_shape(cr, sx, sy, outline, (ACCENT.0, ACCENT.1, ACCENT.2, 1.0));
        }
    }
}

/// Tints the exact SVG shape of every physically pressed button.
fn draw_pressed_shapes(
    st: &HudState,
    cr: &gtk4::cairo::Context,
    sx: f64,
    sy: f64,
    masks: &HashMap<MouseButton, gtk4::cairo::ImageSurface>
) {
    for button in MouseButton::ALL {
        let bit = button.live_bit();
        if (st.live.buttons | st.live.flash) & bit == 0 {
            continue;
        }
        let Some(mask) = masks.get(&button) else {
            continue;
        };
        paint_button_shape(cr, sx, sy, mask, (ACCENT.0, ACCENT.1, ACCENT.2, 0.9));
    }
}

/// Draws chevrons near the wheel showing scroll direction and velocity.
#[allow(
    clippy::suboptimal_flops,
    reason = "plain arithmetic keeps gauge geometry readable"
)]
fn draw_wheel_indicator(
    st: &HudState,
    cr: &gtk4::cairo::Context,
    scale: f64,
    offset_x: f64,
    offset_y: f64
) {
    let intensity = (st.live.wheel_speed / WHEEL_FULL_SCALE).min(1.0);
    if intensity <= 0.01 {
        return;
    }

    let Some(&(ax, ay)) = button_anchors().get(&MouseButton::Wheel) else {
        return;
    };
    let (ax, ay) = (offset_x + ax * scale, offset_y + ay * scale);
    let dir = st.live.wheel_dir;
    let vertical = dir != 0;
    let (ux, uy) = if vertical {
        (0.0, f64::from(dir))
    } else {
        (f64::from(st.live.hwheel_dir), 0.0)
    };

    cr.set_source_rgba(0.93, 0.66, 0.46, 0.4 + 0.6 * intensity);
    cr.set_line_width(1.2);
    for step in 1..=3 {
        let dist = (3.0 + f64::from(step) * 2.2) * scale;
        let cx = ax + ux * dist + if vertical { 6.5 * scale } else { 0.0 };
        let cy = ay + uy * dist + if vertical { 0.0 } else { -5.0 * scale };
        let half = 1.6 * scale;
        let (px, py) = if vertical {
            (half, half * 0.6 * f64::from(dir))
        } else {
            (half * 0.6 * f64::from(st.live.hwheel_dir), half)
        };
        cr.move_to(
            cx - (if vertical { half } else { 0.0 }),
            cy - (if vertical { 0.0 } else { half })
        );
        cr.line_to(cx + px * 0.4, cy + py * 0.4);
        cr.line_to(
            cx + (if vertical { half } else { 0.0 }),
            cy + (if vertical { 0.0 } else { half })
        );
        let _ = cr.stroke();
    }
}

/// Draws one leader line per button from its diagram anchor to the matching
/// row of the Buttons page, so the diagram and the settings list read as one
/// surface.
///
/// The overlay spans the whole window: anchors are projected through the
/// diagram transform into window space, rows through their own allocations.
/// Lines land on the left edge of each row, clamped to the scroll viewport so
/// a row scrolled out of view still keeps its line pointing the right way.
/// The focused button (hover or selection) draws bright and thick.
pub(super) fn attach_leaders_draw(
    state: &Rc<RefCell<HudState>>,
    leaders: &gtk4::DrawingArea,
    drawing: &gtk4::DrawingArea,
    window: &ApplicationWindow
) {
    let state_draw = Rc::clone(state);
    let diagram = drawing.clone();
    let window = window.clone();
    leaders.set_draw_func(move |_, cr, _, _| {
        let st = state_draw.borrow();
        let Some(bounds) = diagram.compute_bounds(&window) else {
            return;
        };
        let (scale, offset_x, offset_y) =
            fit_transform(f64::from(diagram.width()), f64::from(diagram.height()));
        let base_x = f64::from(bounds.x()) + offset_x;
        let base_y = f64::from(bounds.y()) + offset_y;
        let Some(rows) = ROWS_REGISTRY.with(|registry| registry.borrow().as_ref().map(Rc::clone))
        else {
            return;
        };
        let rows = rows.borrow();
        let anchors = button_anchors();

        for button in MouseButton::ALL {
            let Some(&(ax, ay)) = anchors.get(&button) else {
                continue;
            };
            let Some(tile) = rows.get(&button) else {
                continue;
            };
            let ax = ax.mul_add(scale, base_x);
            let ay = ay.mul_add(scale, base_y);
            let Some((tx, ty)) = row_target(&tile.child, &window) else {
                continue;
            };
            let focused = st.selected == Some(button) || st.hover == Some(button);
            if focused {
                cr.set_source_rgba(0.93, 0.66, 0.46, 0.95);
                cr.set_line_width(2.0);
            } else {
                cr.set_source_rgba(0.62, 0.65, 0.70, 0.40);
                cr.set_line_width(1.0);
            }
            let mid_x = f64::midpoint(ax, tx);
            cr.move_to(ax, ay);
            cr.curve_to(mid_x, ay, mid_x, ty, tx, ty);
            let _ = cr.stroke();

            cr.new_path();
            cr.arc(ax, ay, 2.2, 0.0, std::f64::consts::TAU);
            let _ = cr.fill();
            cr.new_path();
            let tip = if focused { 3.0 } else { 2.0 };
            cr.arc(tx, ty, tip, 0.0, std::f64::consts::TAU);
            let _ = cr.fill();
        }
    });
}

/// Window-space landing point of a button tile's leader line: just left of
/// the tile, vertically centred and clamped to its scrolled viewport. `None`
/// when the tile is not on screen, for example while another page is open.
fn row_target(tile: &impl IsA<gtk4::Widget>, window: &ApplicationWindow) -> Option<(f64, f64)> {
    if !tile.is_mapped() {
        return None;
    }
    let bounds = tile.compute_bounds(window)?;
    let x = f64::from(bounds.x()) - 4.0;
    let mut y = f64::from(bounds.y()) + f64::from(bounds.height()) / 2.0;
    if let Some(scroll) = tile.ancestor(gtk4::ScrolledWindow::static_type())
        && let Some(area) = scroll.compute_bounds(window)
    {
        let top = f64::from(area.y()) + 8.0;
        let bottom = (f64::from(area.y()) + f64::from(area.height()) - 8.0).max(top);
        y = y.clamp(top, bottom);
    }
    Some((x, y))
}

/// Anchor points of every button, parsed once from the layout SVG.
fn button_anchors() -> &'static HashMap<MouseButton, (f64, f64)> {
    static ANCHORS: OnceLock<HashMap<MouseButton, (f64, f64)>> = OnceLock::new();
    ANCHORS.get_or_init(|| {
        MouseButton::ALL
            .iter()
            .filter_map(|button| {
                parse_anchor(DEVICE_SVG, button.anchor_id()).map(|point| (*button, point))
            })
            .collect()
    })
}

/// Extracts `(cx, cy)` of the anchor circle with the given id.
fn parse_anchor(svg: &str, anchor_id: &str) -> Option<(f64, f64)> {
    let tail = &svg[svg.find(anchor_id)?..];
    Some((
        parse_attr_number(tail, "cx")?,
        parse_attr_number(tail, "cy")?
    ))
}

fn parse_attr_number(text: &str, attribute: &str) -> Option<f64> {
    let tail = &text[text.find(attribute)? + attribute.len()..];
    let tail = &tail[tail.find('"')? + 1..];
    tail[..tail.find('"')?].parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_button_has_an_svg_anchor() {
        assert_eq!(button_anchors().len(), MouseButton::ALL.len());
    }

    #[test]
    fn gesture_anchor_sits_on_the_rotated_tab() {
        let (x, y) = *button_anchors()
            .get(&MouseButton::Gesture)
            .expect("gesture anchor present");
        assert!(x < 12.0 && (60.0..70.0).contains(&y));
    }

    #[test]
    fn parse_anchor_reads_circle_coordinates() {
        let svg = r#"<circle id="button0-anchor" cx="34.382" cy="38.154" r="0.8"/>"#;
        assert_eq!(parse_anchor(svg, "button0-anchor"), Some((34.382, 38.154)));
        assert_eq!(parse_anchor(svg, "missing-anchor"), None);
    }

    #[test]
    fn element_svg_extracts_self_closing_shapes() {
        let svg = device_element_svg("button4").expect("button4 present");
        assert!(svg.contains("id=\"button4\""));
        assert!(svg.starts_with("<svg"));
        assert!(svg.contains("translate(0,-179.26022)"));
    }

    #[test]
    fn element_svg_rejects_unknown_ids() {
        assert!(device_element_svg("button99").is_none());
    }

    #[test]
    fn styled_element_svg_injects_attributes_before_close() {
        let svg = device_element_svg_styled("button4", "fill=\"none\" stroke=\"#000000\"")
            .expect("button4 present");
        assert!(svg.contains("fill=\"none\" stroke=\"#000000\"/>"));
        assert!(svg.contains("id=\"button4\""));
    }

    #[test]
    fn styled_element_svg_rejects_unknown_ids() {
        assert!(device_element_svg_styled("button99", "fill=\"none\"").is_none());
    }

    #[test]
    fn button_masks_cover_every_button() {
        let masks = build_button_masks();
        assert_eq!(masks.fill.len(), MouseButton::ALL.len());
        assert_eq!(masks.outline.len(), MouseButton::ALL.len());
    }
}
