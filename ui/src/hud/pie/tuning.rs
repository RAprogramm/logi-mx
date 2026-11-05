// SPDX-FileCopyrightText: 2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! The radial menu tuning group: the enable switch, the two radii and the
//! Apply row that sends [`ipc::Request::SetPie`].

use gtk4::{Button, Entry, Switch};
use libadwaita::{ActionRow, PreferencesGroup, prelude::*};
use logi_mx_daemon::ipc;
use logi_mx_driver::config::PieConfig;

use crate::hud::{report, toasts::Toasts};

/// Widgets of the Radial menu group, kept so the initial daemon reply can
/// fill them in after the page was built.
#[derive(Clone)]
pub struct PieWidgets {
    pub enable: Switch,
    pub radius: Entry,
    pub dead:   Entry
}

/// Builds the Radial menu group: the enable switch, the activation radius
/// and dead zone entries, and an Apply button that sends
/// [`ipc::Request::SetPie`] and toasts the outcome.
pub fn radial_menu_group(toast: &Toasts, initial: &PieConfig) -> (PreferencesGroup, PieWidgets) {
    let group = PreferencesGroup::new();
    group.set_title("Radial menu tuning");
    group.set_description(Some(
        "The cursor must travel past the activation radius before the mouse can point at a \
         wedge; the dead zone keeps the centre clear."
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
    radius_row.set_subtitle("Cursor travel in pixels that lets the mouse pick a wedge");
    let radius_entry = Entry::new();
    radius_entry.set_width_chars(6);
    radius_entry.set_valign(gtk4::Align::Center);
    radius_entry.set_text(&initial.activation_radius.to_string());
    radius_row.add_suffix(&radius_entry);

    let dead_row = ActionRow::new();
    dead_row.set_title("Dead zone");
    dead_row.set_subtitle("Radius in pixels where no wedge highlights");
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
        if dead > radius {
            toast_apply.show("Dead zone must not exceed the activation radius");
            return;
        }
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
#[must_use]
pub fn parse_pie_px(text: &str) -> Option<i32> {
    let value = text.trim().parse::<i32>().ok()?;
    (1..=4000).contains(&value).then_some(value)
}

#[cfg(test)]
#[path = "tuning_tests.rs"]
mod tests;
