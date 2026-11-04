// SPDX-FileCopyrightText: 2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Pie page: a live preview of the radial menu plus its tuning. The wheel
//! mirrors what the overlay paints — one sector per bound direction,
//! labelled with the action summary — and clicking a sector inspects it.
//! Direction bindings themselves stay edited on the Gestures page.

use std::{cell::RefCell, rc::Rc};

use gtk4::{Button, DrawingArea, Entry, GestureClick, Label, ScrolledWindow, Switch, cairo};
use libadwaita::{ActionRow, PreferencesGroup, prelude::*};
use logi_mx_daemon::{
    ipc::{self, Response},
    pie::{PieSector, action_summary, direction_angle}
};
use logi_mx_driver::{config::PieConfig, devices::GestureDirection};

use super::{page_body, page_scroll, report, toasts::Toasts};

/// Wheel side length in logical pixels.
const WHEEL_SIZE: i32 = 260;
/// Radius of the transparent hole at the centre.
const INNER_RADIUS: f64 = 34.0;
/// Gap between the ring and the drawing area edge.
const OUTER_MARGIN: f64 = 8.0;
/// Sector label font and size.
const LABEL_FONT: &str = "Sans";
const LABEL_SIZE: f64 = 11.0;
/// Maximum characters painted inside a sector.
const LABEL_MAX_CHARS: usize = 10;
/// Translucent dark fill of an unselected sector.
const FILL_BASE: (f64, f64, f64, f64) = (0.07, 0.08, 0.10, 0.80);
/// Accent fill of the selected sector.
const FILL_SELECTED: (f64, f64, f64, f64) = (53.0 / 255.0, 132.0 / 255.0, 228.0 / 255.0, 0.88);
/// Ring outline colour.
const OUTLINE: (f64, f64, f64, f64) = (0.91, 0.93, 0.96, 0.30);
/// Sector label text colour.
const TEXT: (f64, f64, f64, f64) = (0.94, 0.96, 0.98, 0.95);

/// What the preview paints: the bound sectors and the clicked selection.
#[derive(Default)]
struct WheelModel {
    sectors:  Vec<PieSector>,
    selected: Option<GestureDirection>
}

/// Builds the Pie page: intro, interactive wheel preview, inspection line
/// and the radial menu tuning group.
pub(super) fn pie_page(toast: &Toasts) -> ScrolledWindow {
    let scroll = page_scroll();
    let body = page_body();

    let intro = PreferencesGroup::new();
    intro.set_title("Radial menu");
    intro.set_description(Some(
        "Hold the gesture button: a thin ring charges around the cursor, then the menu blooms in. \
         Scroll the wheel or point the mouse to select a sector, release to fire it. A quick tap \
         keeps the click binding; a double click opens the menu at once."
    ));
    let hint = ActionRow::new();
    hint.set_title("Sector actions");
    hint.set_subtitle("Bind directions on the Gestures page");
    intro.add(&hint);

    let model: Rc<RefCell<WheelModel>> = Rc::new(RefCell::new(WheelModel::default()));

    let area = DrawingArea::new();
    area.set_size_request(WHEEL_SIZE, WHEEL_SIZE);
    area.set_halign(gtk4::Align::Center);
    area.set_margin_top(8);
    area.set_margin_bottom(8);
    let paint_model = Rc::clone(&model);
    area.set_draw_func(move |_area, cr, width, height| {
        paint_wheel(
            cr,
            f64::from(width),
            f64::from(height),
            &paint_model.borrow()
        );
    });

    let details = Label::new(Some("Click a sector to inspect its binding"));
    details.add_css_class("dim-label");
    details.set_halign(gtk4::Align::Center);

    let click = GestureClick::new();
    let click_model = Rc::clone(&model);
    let click_details = details.clone();
    let click_area = area.clone();
    click.connect_pressed(move |_gesture, _n_press, x, y| {
        let mut state = click_model.borrow_mut();
        state.selected = sector_at(x, y, f64::from(WHEEL_SIZE), &state.sectors);
        let text = match state.selected {
            Some(direction) => {
                let label = state
                    .sectors
                    .iter()
                    .find(|sector| sector.direction == direction)
                    .map_or("", |sector| sector.label.as_str());
                if label.is_empty() {
                    format!("{direction:?} — bound, no summary")
                } else {
                    format!("{direction:?} — {label}")
                }
            }
            None => "Click a sector to inspect its binding".to_string()
        };
        click_details.set_text(&text);
        click_area.queue_draw();
    });
    area.add_controller(click);

    let (tuning, widgets) = radial_menu_group(toast, &PieConfig::default());

    body.append(&intro);
    body.append(&area);
    body.append(&details);
    body.append(&tuning);
    scroll.set_child(Some(&body));

    if let Some(Response::Gestures {
        gestures,
        pie,
        ..
    }) = ipc::try_request(&ipc::Request::GetGestures)
    {
        model.borrow_mut().sectors = gestures
            .iter()
            .filter(|gesture| gesture.direction != GestureDirection::None)
            .map(|gesture| PieSector {
                direction: gesture.direction,
                label:     action_summary(&gesture.action)
            })
            .collect();
        area.queue_draw();
        widgets.enable.set_active(pie.enabled);
        widgets.radius.set_text(&pie.activation_radius.to_string());
        widgets.dead.set_text(&pie.dead_zone.to_string());
    }
    scroll
}

/// Paints the preview ring: annulus segments tile the circle, the selected
/// one wears the accent, and every sector carries its action summary.
fn paint_wheel(cr: &cairo::Context, width: f64, height: f64, model: &WheelModel) {
    let centre = width.min(height) / 2.0;
    let outer = (centre - OUTER_MARGIN).max(INNER_RADIUS + 1.0);
    if model.sectors.is_empty() {
        let (red, green, blue, alpha) = OUTLINE;
        cr.set_source_rgba(red, green, blue, alpha);
        cr.set_line_width(2.0);
        cr.arc(
            centre,
            centre,
            f64::midpoint(outer, INNER_RADIUS),
            0.0,
            std::f64::consts::TAU
        );
        let _ = cr.stroke();
        return;
    }
    let Ok(count) = u32::try_from(model.sectors.len()) else {
        return;
    };
    let half = std::f64::consts::PI / f64::from(count);
    for sector in &model.sectors {
        let Some(middle) = direction_angle(sector.direction) else {
            continue;
        };
        let (red, green, blue, alpha) = if model.selected == Some(sector.direction) {
            FILL_SELECTED
        } else {
            FILL_BASE
        };
        cr.new_path();
        cr.arc(centre, centre, outer, middle - half, middle + half);
        cr.arc_negative(centre, centre, INNER_RADIUS, middle + half, middle - half);
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
    paint_labels(cr, centre, outer, model);
}

/// Draws one truncated action summary per sector, mid-band.
fn paint_labels(cr: &cairo::Context, centre: f64, outer: f64, model: &WheelModel) {
    use gtk4::cairo::{FontSlant, FontWeight};

    cr.select_font_face(LABEL_FONT, FontSlant::Normal, FontWeight::Normal);
    cr.set_font_size(LABEL_SIZE);
    let (red, green, blue, alpha) = TEXT;
    cr.set_source_rgba(red, green, blue, alpha);
    let radius = f64::midpoint(outer, INNER_RADIUS);
    for sector in &model.sectors {
        let Some(middle) = direction_angle(sector.direction) else {
            continue;
        };
        let mut label: String = sector.label.chars().take(LABEL_MAX_CHARS).collect();
        if sector.label.chars().count() > LABEL_MAX_CHARS {
            label.push('…');
        }
        if label.is_empty() {
            continue;
        }
        let Ok(extents) = cr.text_extents(&label) else {
            continue;
        };
        let x = radius.mul_add(middle.cos(), centre) - extents.x_bearing() - extents.width() / 2.0;
        let y =
            radius.mul_add(middle.sin(), centre) - extents.y_bearing() - extents.height() / 2.0;
        cr.move_to(x, y);
        let _ = cr.show_text(&label);
    }
}

/// Maps a click position to the sector under it: the bound direction whose
/// centre angle is nearest, so the preview and the overlay select the same
/// sector for the same angle.
fn sector_at(x: f64, y: f64, size: f64, sectors: &[PieSector]) -> Option<GestureDirection> {
    let centre = size / 2.0;
    let outer = centre - OUTER_MARGIN;
    let (dx, dy) = (x - centre, y - centre);
    let distance = dx.hypot(dy);
    if distance < INNER_RADIUS || distance > outer {
        return None;
    }
    let angle = dy.atan2(dx);
    sectors
        .iter()
        .filter_map(|sector| {
            let middle = direction_angle(sector.direction)?;
            let diff = (angle - middle + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU)
                - std::f64::consts::PI;
            Some((diff.abs(), sector.direction))
        })
        .min_by(|(left, _), (right, _)| left.total_cmp(right))
        .map(|(_, direction)| direction)
}

/// Parses one pie pixel entry: a whole number within `1..=4000`.
///
/// # Arguments
///
/// * `text` - Raw entry text, surrounding whitespace allowed.
///
/// # Returns
///
/// The value when it is a whole number of pixels within `1..=4000`,
/// `None` for anything else.
fn parse_pie_px(text: &str) -> Option<i32> {
    let value = text.trim().parse::<i32>().ok()?;
    (1..=4000).contains(&value).then_some(value)
}

/// Widgets of the Radial menu group, kept so the initial daemon reply can
/// fill them in after the page was built.
struct PieWidgets {
    enable: Switch,
    radius: Entry,
    dead:   Entry
}

/// Builds the Radial menu group: the enable switch, the activation radius
/// and dead zone entries, and an Apply button that sends
/// [`ipc::Request::SetPie`] and toasts the outcome.
fn radial_menu_group(toast: &Toasts, initial: &PieConfig) -> (PreferencesGroup, PieWidgets) {
    let group = PreferencesGroup::new();
    group.set_title("Radial menu tuning");
    group.set_description(Some(
        "The cursor must travel past the activation radius before the mouse can point at a \
         sector; the dead zone keeps the centre clear."
    ));

    let enable_row = ActionRow::new();
    enable_row.set_title("Enable radial menu");
    let enable_switch = Switch::new();
    enable_switch.set_valign(gtk4::Align::Center);
    enable_switch.set_active(initial.enabled);
    enable_row.add_suffix(&enable_switch);
    enable_row.set_activatable_widget(Some(&enable_switch));

    let radius_row = ActionRow::new();
    radius_row.set_title("Activation radius");
    radius_row.set_subtitle("Cursor travel in pixels that lets the mouse pick a sector");
    let radius_entry = Entry::new();
    radius_entry.set_width_chars(6);
    radius_entry.set_valign(gtk4::Align::Center);
    radius_entry.set_text(&initial.activation_radius.to_string());
    radius_row.add_suffix(&radius_entry);

    let dead_row = ActionRow::new();
    dead_row.set_title("Dead zone");
    dead_row.set_subtitle("Radius in pixels where no sector highlights");
    let dead_entry = Entry::new();
    dead_entry.set_width_chars(6);
    dead_entry.set_valign(gtk4::Align::Center);
    dead_entry.set_text(&initial.dead_zone.to_string());
    dead_row.add_suffix(&dead_entry);

    let apply_row = ActionRow::new();
    apply_row.set_title("Apply tuning");
    apply_row.set_subtitle("Sends the switch and both radii to the daemon");
    let apply_btn = Button::with_label("Apply");
    apply_row.add_suffix(&apply_btn);
    apply_row.set_activatable_widget(Some(&apply_btn));

    let toast_apply = toast.clone();
    let (btn_switch, btn_radius, btn_dead) = (
        enable_switch.clone(),
        radius_entry.clone(),
        dead_entry.clone()
    );
    apply_btn.connect_clicked(move |_| {
        let tuning = match (
            parse_pie_px(&btn_radius.text()),
            parse_pie_px(&btn_dead.text())
        ) {
            (Some(radius), Some(dead)) => Some((radius, dead)),
            _ => None
        };
        let Some((radius, dead)) = tuning else {
            toast_apply.show("Pie radius and dead zone: whole pixels, 1–4000");
            return;
        };
        let response = ipc::try_request(&ipc::Request::SetPie {
            enabled:           btn_switch.is_active(),
            activation_radius: radius,
            dead_zone:         dead
        });
        report(response, &toast_apply, "Radial menu updated");
    });

    group.add(&enable_row);
    group.add(&radius_row);
    group.add(&dead_row);
    group.add(&apply_row);
    (
        group,
        PieWidgets {
            enable: enable_switch,
            radius: radius_entry,
            dead:   dead_entry
        }
    )
}

#[cfg(test)]
mod tests {
    use logi_mx_daemon::pie::PieSector;
    use logi_mx_driver::devices::GestureDirection;

    use super::{parse_pie_px, sector_at};

    fn sectors() -> Vec<PieSector> {
        vec![
            PieSector {
                direction: GestureDirection::Up,
                label:     "Meta".to_string()
            },
            PieSector {
                direction: GestureDirection::Right,
                label:     "A".to_string()
            },
        ]
    }

    #[test]
    fn sector_at_picks_only_inside_the_ring() {
        let sectors = sectors();
        assert_eq!(
            sector_at(130.0, 20.0, 260.0, &sectors),
            Some(GestureDirection::Up)
        );
        assert_eq!(
            sector_at(240.0, 130.0, 260.0, &sectors),
            Some(GestureDirection::Right)
        );
        assert_eq!(
            sector_at(130.0, 130.0, 260.0, &sectors),
            None,
            "the centre hole selects nothing"
        );
        assert_eq!(
            sector_at(2.0, 2.0, 260.0, &sectors),
            None,
            "outside the outer edge selects nothing"
        );
    }

    #[test]
    fn pie_px_accepts_whole_positive_pixels() {
        assert_eq!(parse_pie_px("40"), Some(40));
        assert_eq!(parse_pie_px(" 12 "), Some(12));
        assert_eq!(parse_pie_px("1"), Some(1));
        assert_eq!(parse_pie_px("4000"), Some(4000));
    }

    #[test]
    fn pie_px_rejects_garbage_and_out_of_range() {
        assert_eq!(parse_pie_px(""), None);
        assert_eq!(parse_pie_px("0"), None);
        assert_eq!(parse_pie_px("-5"), None);
        assert_eq!(parse_pie_px("4001"), None);
        assert_eq!(parse_pie_px("wide"), None);
        assert_eq!(parse_pie_px("12.5"), None);
    }
}
