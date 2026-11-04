// SPDX-FileCopyrightText: 2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Pie page: a live preview of the radial menu plus its tuning. The wheel
//! mirrors what the overlay paints — one sector per bound direction,
//! labelled with the action summary. Clicking a sector selects it and opens
//! the sector editor below: bind a shell command, pick an existing Hyprland
//! keybind, or clear the sector — all on the global scope, firing on
//! release like the pie commit path does.

use std::{
    cell::{Cell, RefCell},
    rc::Rc
};

use gtk4::{Button, DrawingArea, Entry, GestureClick, Label, ScrolledWindow, Switch, cairo};
use libadwaita::{ActionRow, PreferencesGroup, prelude::*};
use logi_mx_daemon::{
    ipc::{self, Response},
    pie::{PieSector, action_summary, direction_angle}
};
use logi_mx_driver::{
    config::PieConfig,
    devices::{Action, Gesture, GestureDirection, GestureMode}
};

use super::{keybinds::open_keybind_picker, page_body, page_scroll, report, toasts::Toasts};

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

/// What the preview paints: the bound gestures and the clicked selection.
#[derive(Default)]
struct WheelModel {
    bindings: Vec<Gesture>,
    selected: Option<GestureDirection>
}

impl WheelModel {
    /// The bound sectors with their action summaries, in binding order.
    fn sectors(&self) -> Vec<PieSector> {
        self.bindings
            .iter()
            .map(|gesture| PieSector {
                direction: gesture.direction,
                label:     action_summary(&gesture.action)
            })
            .collect()
    }

    /// The summary painted for `direction`, empty when unbound.
    fn label_for(&self, direction: GestureDirection) -> String {
        self.bindings
            .iter()
            .find(|gesture| gesture.direction == direction)
            .map_or_else(String::new, |gesture| action_summary(&gesture.action))
    }

    /// The shell command bound to `direction`, empty for other actions.
    fn command_for(&self, direction: GestureDirection) -> String {
        self.bindings
            .iter()
            .find(|gesture| gesture.direction == direction)
            .and_then(|gesture| match gesture.action.as_ref() {
                Action::Command {
                    command
                } => Some(command.clone()),
                _ => None
            })
            .unwrap_or_default()
    }

    /// Inserts or replaces the binding for the gesture's direction.
    fn upsert(&mut self, gesture: Gesture) {
        match self
            .bindings
            .iter_mut()
            .find(|existing| existing.direction == gesture.direction)
        {
            Some(existing) => *existing = gesture,
            None => self.bindings.push(gesture)
        }
    }

    /// Drops the binding for `direction`.
    fn remove(&mut self, direction: GestureDirection) {
        self.bindings
            .retain(|gesture| gesture.direction != direction);
    }
}

/// Widgets of the sector editor, kept so the click handler can retarget and
/// gate them. Cheap to clone: every field is a GTK handle.
#[derive(Clone)]
struct SectorEditor {
    group:     PreferencesGroup,
    title:     ActionRow,
    command:   Entry,
    apply_btn: Button,
    pick_btn:  Button,
    clear_btn: Button
}

impl SectorEditor {
    /// Builds the editor group: a title row, the command row, the keybind
    /// picker row and the clear row, all insensitive until a sector is
    /// selected.
    fn new() -> Self {
        let group = PreferencesGroup::new();
        group.set_title("Sector editor");
        group.set_description(Some(
            "Click a sector on the wheel to bind it here; bindings fire on release"
        ));

        let title = ActionRow::new();
        title.set_title("Sector");
        title.set_subtitle("Click a sector above");

        let command_row = ActionRow::new();
        command_row.set_title("Application command");
        command_row.set_subtitle("Press Enter to bind, e.g. thunar");
        let command = Entry::new();
        command.set_width_chars(18);
        command.set_valign(gtk4::Align::Center);
        let apply_btn = Button::with_label("Bind");
        apply_btn.set_valign(gtk4::Align::Center);
        command_row.add_suffix(&apply_btn);
        command_row.add_suffix(&command);

        let pick_row = ActionRow::new();
        pick_row.set_title("Keyboard shortcut");
        pick_row.set_subtitle("Pick an existing Hyprland keybind");
        let pick_btn = Button::with_label("Pick…");
        pick_btn.set_valign(gtk4::Align::Center);
        pick_row.add_suffix(&pick_btn);
        pick_row.set_activatable_widget(Some(&pick_btn));

        let clear_row = ActionRow::new();
        clear_row.set_title("Clear sector");
        clear_row.set_subtitle("Removes this direction's binding");
        let clear_btn = Button::with_label("Clear");
        clear_btn.add_css_class("destructive-action");
        clear_btn.set_valign(gtk4::Align::Center);
        clear_row.add_suffix(&clear_btn);
        clear_row.set_activatable_widget(Some(&clear_btn));

        group.add(&title);
        group.add(&command_row);
        group.add(&pick_row);
        group.add(&clear_row);
        group.set_sensitive(false);

        Self {
            group,
            title,
            command,
            apply_btn,
            pick_btn,
            clear_btn
        }
    }

    /// Points the editor at `direction` (or disables it when `None`),
    /// prefilling the command entry when the sector holds a command.
    fn inspect(&self, model: &WheelModel, direction: Option<GestureDirection>) {
        self.group.set_sensitive(direction.is_some());
        let Some(dir) = direction else {
            self.title.set_title("Sector");
            self.title.set_subtitle("Click a sector above");
            self.command.set_text("");
            return;
        };
        let label = model.label_for(dir);
        let summary = if label.is_empty() { "unbound" } else { &label };
        self.title.set_title(&format!("{dir:?} — {summary}"));
        self.title.set_subtitle("Choose a new action below");
        self.command.set_text(&model.command_for(dir));
    }
}

/// Builds the Pie page: intro, interactive wheel preview, inspection line,
/// sector editor and the radial menu tuning group.
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

    let model: Rc<RefCell<WheelModel>> = Rc::new(RefCell::new(WheelModel::default()));
    let selected: Rc<Cell<Option<GestureDirection>>> = Rc::new(Cell::new(None));

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

    let editor = SectorEditor::new();
    wire_sector_editor(&SectorContext {
        toast:    toast.clone(),
        model:    Rc::clone(&model),
        selected: Rc::clone(&selected),
        area:     area.clone(),
        details:  details.clone(),
        editor:   editor.clone()
    });

    let click = GestureClick::new();
    let click_model = Rc::clone(&model);
    let click_selected = Rc::clone(&selected);
    let click_details = details.clone();
    let click_area = area.clone();
    let click_editor = editor.clone();
    click.connect_pressed(move |_gesture, _n_press, x, y| {
        let sectors = click_model.borrow().sectors();
        let hit = sector_at(x, y, f64::from(WHEEL_SIZE), &sectors);
        click_selected.set(hit);
        click_model.borrow_mut().selected = hit;
        let text = hit.map_or_else(
            || "Click a sector to inspect its binding".to_string(),
            |direction| {
                let label = click_model.borrow().label_for(direction);
                if label.is_empty() {
                    format!("{direction:?} — unbound")
                } else {
                    format!("{direction:?} — {label}")
                }
            }
        );
        click_details.set_text(&text);
        click_editor.inspect(&click_model.borrow(), hit);
        click_area.queue_draw();
    });
    area.add_controller(click);

    let (tuning, widgets) = radial_menu_group(toast, &PieConfig::default());

    body.append(&intro);
    body.append(&area);
    body.append(&details);
    body.append(&editor.group);
    body.append(&tuning);
    scroll.set_child(Some(&body));

    if let Some(Response::Gestures {
        gestures,
        pie,
        ..
    }) = ipc::try_request(&ipc::Request::GetGestures)
    {
        model.borrow_mut().bindings = gestures
            .into_iter()
            .filter(|gesture| gesture.direction != GestureDirection::None)
            .collect();
        area.queue_draw();
        widgets.enable.set_active(pie.enabled);
        widgets.radius.set_text(&pie.activation_radius.to_string());
        widgets.dead.set_text(&pie.dead_zone.to_string());
    }
    scroll
}

/// Everything a sector editor callback needs to act on the selection.
#[derive(Clone)]
struct SectorContext {
    toast:    Toasts,
    model:    Rc<RefCell<WheelModel>>,
    selected: Rc<Cell<Option<GestureDirection>>>,
    area:     DrawingArea,
    details:  Label,
    editor:   SectorEditor
}

/// Sends `action` for the selected sector and reflects it on the wheel.
fn bind_selected(ctx: &SectorContext, action: Option<Action>, success: &str) {
    let Some(direction) = ctx.selected.get() else {
        ctx.toast.show("Select a sector first");
        return;
    };
    let Some(action) = action else {
        ctx.toast.show("Enter a command first");
        return;
    };
    let response = ipc::try_request(&ipc::Request::SetGesture {
        direction,
        mode: GestureMode::OnRelease,
        action: Box::new(action.clone())
    });
    if matches!(response, Some(Response::Ok)) {
        ctx.model.borrow_mut().upsert(Gesture {
            direction,
            mode: GestureMode::OnRelease,
            action: Box::new(action)
        });
        let label = ctx.model.borrow().label_for(direction);
        ctx.details.set_text(&format!("{direction:?} — {label}"));
        ctx.editor.inspect(&ctx.model.borrow(), Some(direction));
        ctx.area.queue_draw();
    }
    report(response, &ctx.toast, success);
}

/// Wires the sector editor rows to the daemon and the wheel model.
fn wire_sector_editor(ctx: &SectorContext) {
    let bind_command = {
        let ctx = ctx.clone();
        move |entry: &Entry| {
            let text = entry.text().trim().to_string();
            let action = (!text.is_empty()).then_some(Action::Command {
                command: text
            });
            bind_selected(&ctx, action, "Sector bound");
        }
    };
    ctx.editor.apply_btn.connect_clicked({
        let command = ctx.editor.command.clone();
        let bind = bind_command.clone();
        move |_| bind(&command)
    });
    ctx.editor
        .command
        .connect_activate(move |entry| bind_command(entry));

    ctx.editor.pick_btn.connect_clicked({
        let ctx = ctx.clone();
        move |_| {
            let picker_toast = ctx.toast.clone();
            let picked = ctx.clone();
            open_keybind_picker(&picker_toast, move |keys| {
                bind_selected(
                    &picked,
                    Some(Action::Keypress {
                        keys
                    }),
                    "Sector bound"
                );
            });
        }
    });

    ctx.editor.clear_btn.connect_clicked({
        let ctx = ctx.clone();
        move |_| {
            let Some(direction) = ctx.selected.get() else {
                ctx.toast.show("Select a sector first");
                return;
            };
            let response = ipc::try_request(&ipc::Request::ClearGesture {
                direction
            });
            if matches!(response, Some(Response::Ok)) {
                let mut model = ctx.model.borrow_mut();
                model.remove(direction);
                model.selected = None;
                drop(model);
                ctx.selected.set(None);
                ctx.details
                    .set_text("Click a sector to inspect its binding");
                ctx.editor.inspect(&ctx.model.borrow(), None);
                ctx.area.queue_draw();
            }
            report(response, &ctx.toast, "Sector cleared");
        }
    });
}

/// Paints the preview ring: annulus segments tile the circle, the selected
/// one wears the accent, and every sector carries its action summary.
fn paint_wheel(cr: &cairo::Context, width: f64, height: f64, model: &WheelModel) {
    let centre = width.min(height) / 2.0;
    let outer = (centre - OUTER_MARGIN).max(INNER_RADIUS + 1.0);
    let sectors = model.sectors();
    if sectors.is_empty() {
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
    let Ok(count) = u32::try_from(sectors.len()) else {
        return;
    };
    let half = std::f64::consts::PI / f64::from(count);
    for sector in &sectors {
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
    paint_labels(cr, centre, outer, &sectors);
}

/// Draws one truncated action summary per sector, mid-band.
fn paint_labels(cr: &cairo::Context, centre: f64, outer: f64, sectors: &[PieSector]) {
    use gtk4::cairo::{FontSlant, FontWeight};

    cr.select_font_face(LABEL_FONT, FontSlant::Normal, FontWeight::Normal);
    cr.set_font_size(LABEL_SIZE);
    let (red, green, blue, alpha) = TEXT;
    cr.set_source_rgba(red, green, blue, alpha);
    let radius = f64::midpoint(outer, INNER_RADIUS);
    for sector in sectors {
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
