// SPDX-FileCopyrightText: 2025-2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Firmware button tiles: each device button paired with the dropdown that
//! assigns its behaviour in the device firmware, plus the background loader
//! that paints the stored remap index on every tile.

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
    sync::{Arc, Mutex},
    thread
};

use gtk4::{
    Box as GtkBox, DropDown, EventControllerMotion, FlowBox, FlowBoxChild, Label, Orientation,
    SelectionMode, glib
};
use libadwaita::prelude::*;
use logi_mx_daemon::ipc::{self, Response};
use logi_mx_driver::devices::control_id;

use super::{HudState, MouseButton, ROWS_REGISTRY, RowMap, Tile, queue_leaders, sync_row_focus};
use crate::hud::toasts::Toasts;

/// Native remap targets offered on the assignment dropdowns: label and CID.
const REMAP_TARGETS: [(&str, u16); 7] = [
    ("Left", control_id::LEFT_BUTTON),
    ("Right", control_id::RIGHT_BUTTON),
    ("Middle click", control_id::MIDDLE_BUTTON),
    ("Back", control_id::BACK_BUTTON),
    ("Forward", control_id::FORWARD_BUTTON),
    ("Gesture", control_id::GESTURE_BUTTON),
    ("DPI Shift", control_id::SMARTSHIFT_TOGGLE)
];

/// Dropdown choices for a tile: index 0 restores the device-native behaviour,
/// every later entry maps onto the matching `REMAP_TARGETS` element.
const ASSIGNMENT_CHOICES: [&str; 8] = [
    "Native",
    "Left",
    "Right",
    "Middle click",
    "Back",
    "Forward",
    "Gesture",
    "DPI Shift"
];

/// Smallest height (in pixels) one tile row needs to stay comfortable; the
/// flow drops to two columns only when the panel cannot fit every row at this
/// height in a single column.
const TILE_ROW_MIN_HEIGHT: i32 = 48;

/// Dropdown index matching the firmware remap stored for `control_id`.
///
/// `Some(0)` means the firmware confirmed the control as native, `Some(n)`
/// a known remap; `None` means the read failed or the stored target is not
/// one this UI can name. Keeping the unknown case apart from Native matters:
/// a tile that never saw its real remap must still send a restore when the
/// user picks Native, or a firmware remap read failure would hide a mapping
/// that stays active.
///
/// This is device I/O — a round-trip that can take a second when the mouse
/// has fallen asleep — so callers keep it off the main thread.
///
/// # Arguments
///
/// * `control_id` - Firmware CID of the reprogrammable button.
///
/// # Returns
///
/// `Some` dropdown index into [`ASSIGNMENT_CHOICES`] when the firmware
/// answered, `None` when the read failed or the target is unknown.
fn remap_index(control_id: u16) -> Option<u32> {
    let response = ipc::try_request(&ipc::Request::GetButtonRemap {
        control_id
    })?;
    let Response::ButtonRemap {
        target_cid, ..
    } = response
    else {
        return None;
    };
    if target_cid == 0 {
        return Some(0);
    }
    REMAP_TARGETS
        .iter()
        .position(|(_, cid)| *cid == target_cid)
        .and_then(|position| u32::try_from(position).ok())
        .map(|position| position + 1)
}

/// `applied` marker for a tile whose firmware remap was never read: picking
/// any entry, Native included, must reach the daemon.
const UNKNOWN_APPLIED: u32 = u32::MAX;

thread_local! {
    /// Main-context hook that applies remap results delivered by the loader
    /// thread; cleared when the HUD closes so a late reply finds nothing.
    static REMAP_HOOK: RefCell<Option<Rc<dyn Fn()>>> = const { RefCell::new(None) };
}

/// Clears the remap loader hook; called when the HUD window closes.
pub(in crate::hud) fn clear_remap_hook() {
    REMAP_HOOK.with(|cell| *cell.borrow_mut() = None);
}

/// Shared slot for remap results delivered by the loader thread.
type RemapSlot = Arc<Mutex<Option<Vec<(u16, Option<u32>)>>>>;

/// Reads every tile's firmware remap off the main thread and paints the
/// dropdowns once the first frame is up.
///
/// The window must not wait on device I/O: the first read after the mouse
/// sleeps costs about a second. A worker thread collects the plain
/// `(control_id, outcome)` pairs, parks them in a shared slot and wakes the
/// main context, where the hook applies each confirmed index only while the
/// user has not touched that dropdown yet; a failed read leaves the tile
/// unknown, so choosing Native still sends the restore.
///
/// # Arguments
///
/// * `tiles` - Built button tiles keyed by button.
pub(super) fn spawn_remap_loader(tiles: &RowMap) {
    let plan: Vec<(u16, DropDown, Rc<Cell<u32>>)> = tiles
        .borrow()
        .iter()
        .map(|(button, tile)| {
            (
                button.cid(),
                tile.dropdown.clone(),
                Rc::clone(&tile.applied)
            )
        })
        .collect();
    let slot: RemapSlot = Arc::new(Mutex::new(None));
    let cids: Vec<u16> = plan.iter().map(|(cid, ..)| *cid).collect();
    let slot_thread = Arc::clone(&slot);
    let slot_hook = Arc::clone(&slot);
    let hook: Rc<dyn Fn()> = Rc::new(move || {
        let Some(results) = slot_hook.lock().ok().and_then(|mut guard| guard.take()) else {
            return;
        };
        for (control_id, outcome) in results {
            for (cid, dropdown, applied) in &plan {
                if *cid == control_id
                    && applied.get() == UNKNOWN_APPLIED
                    && dropdown.selected() == 0
                    && let Some(index) = outcome
                {
                    applied.set(index);
                    dropdown.set_selected(index);
                }
            }
        }
    });
    REMAP_HOOK.with(|cell| *cell.borrow_mut() = Some(hook));
    let context = glib::MainContext::default();
    if let Err(e) = thread::Builder::new()
        .name("remap-loader".to_string())
        .spawn(move || {
            let results: Vec<(u16, Option<u32>)> = cids
                .into_iter()
                .map(|control_id| (control_id, remap_index(control_id)))
                .collect();
            if let Ok(mut guard) = slot_thread.lock() {
                *guard = Some(results);
            }
            context.invoke(|| {
                REMAP_HOOK.with(|cell| {
                    if let Some(hook) = cell.borrow().as_ref() {
                        hook();
                    }
                });
            });
        })
    {
        tracing::warn!("Failed to spawn the remap loader thread: {e}");
    }
}

/// Builds one button tile: a card with the button name and its assignment
/// dropdown; picking an entry writes the remap into the mouse firmware
/// instantly and reverts the choice when the daemon refuses.
fn button_tile(
    state: &Rc<RefCell<HudState>>,
    drawing: &gtk4::DrawingArea,
    toast: &Toasts,
    rows: &RowMap,
    button: MouseButton
) -> Tile {
    let child = FlowBoxChild::new();
    child.add_css_class("hud-tile");
    child.set_hexpand(true);
    let inner = GtkBox::new(Orientation::Horizontal, 12);
    let name = Label::new(Some(button.label()));
    name.add_css_class("title");
    name.set_xalign(0.0);
    name.set_hexpand(true);
    name.set_valign(gtk4::Align::Center);
    let dropdown = DropDown::from_strings(&ASSIGNMENT_CHOICES);
    dropdown.add_css_class("hud-assign");
    dropdown.set_valign(gtk4::Align::Center);
    inner.append(&name);
    inner.append(&dropdown);
    child.set_child(Some(&inner));

    let state_row = Rc::clone(state);
    let drawing_row = drawing.clone();
    child.connect_activate(move |_| {
        let mut st = state_row.borrow_mut();
        st.selected = Some(button);
        drawing_row.queue_draw();
        sync_row_focus(&st);
    });

    let state_enter = Rc::clone(state);
    let drawing_enter = drawing.clone();
    let motion = EventControllerMotion::new();
    motion.connect_motion(move |_, _, _| {
        let mut st = state_enter.borrow_mut();
        if st.hover != Some(button) {
            st.hover = Some(button);
            drawing_enter.queue_draw();
            sync_row_focus(&st);
        }
    });
    let state_leave = Rc::clone(state);
    let drawing_leave = drawing.clone();
    motion.connect_leave(move |_| {
        let mut st = state_leave.borrow_mut();
        if st.hover == Some(button) {
            st.hover = None;
            drawing_leave.queue_draw();
            sync_row_focus(&st);
        }
    });
    child.add_controller(motion);
    child.connect_map(|_| queue_leaders());

    let state_assign = Rc::clone(state);
    let rows_assign = Rc::clone(rows);
    let drawing_assign = drawing.clone();
    let toast_assign = toast.clone();
    let applied = Rc::new(Cell::new(UNKNOWN_APPLIED));
    let applied_notify = Rc::clone(&applied);
    dropdown.connect_selected_notify(move |picker| {
        let index = picker.selected();
        if index == applied_notify.get() {
            return;
        }
        let target_cid = if index == 0 {
            0
        } else {
            REMAP_TARGETS[(index - 1) as usize].1
        };
        let response = ipc::try_request(&ipc::Request::SetButtonRemap {
            control_id: button.cid(),
            target_cid
        });
        if matches!(response, Some(Response::Ok)) {
            applied_notify.set(index);
            let label =
                (target_cid != 0).then(|| REMAP_TARGETS[(index - 1) as usize].0.to_string());
            set_assignment(
                &state_assign,
                &rows_assign,
                &drawing_assign,
                button,
                label.clone()
            );
            let message = label.map_or_else(
                || format!("{} restored to native", button.label()),
                |target| format!("{} now acts as {target}", button.label())
            );
            toast_assign.show(&message);
        } else {
            let revert = applied_notify.get();
            picker.set_selected(if revert == UNKNOWN_APPLIED { 0 } else { revert });
            toast_assign.show("Daemon unavailable — change not applied");
        }
    });

    Tile {
        child,
        dropdown,
        applied
    }
}

/// Builds the button tiles: an adaptive flow of cards, each pairing the
/// button with a dropdown that assigns its behaviour straight into the mouse
/// firmware. The flow fits one or two columns to the available width.
/// Hovering a tile highlights the button on the diagram.
pub(super) fn buttons_list_section(
    state: &Rc<RefCell<HudState>>,
    drawing: &gtk4::DrawingArea,
    toast: &Toasts
) -> (GtkBox, RowMap) {
    let section = GtkBox::new(Orientation::Vertical, 8);
    section.set_vexpand(true);
    let title = Label::new(Some("Buttons"));
    title.add_css_class("title-4");
    title.set_xalign(0.0);
    let description = Label::new(Some(
        "Pick what each button should do; the mouse remembers it on its own"
    ));
    description.add_css_class("dim-label");
    description.add_css_class("caption");
    description.set_xalign(0.0);

    let flow = FlowBox::new();
    flow.add_css_class("hud-tiles");
    flow.set_selection_mode(SelectionMode::None);
    flow.set_homogeneous(false);
    flow.set_min_children_per_line(1);
    flow.set_max_children_per_line(1);
    flow.set_hexpand(true);
    flow.set_column_spacing(12);
    flow.set_row_spacing(8);

    flow.connect_notify_local(Some("height"), move |flow, _| {
        let rows = i32::try_from(MouseButton::ALL.len()).unwrap_or(7);
        let fits_one_column = flow.height() >= TILE_ROW_MIN_HEIGHT * rows;
        let wanted = if fits_one_column { 1 } else { 2 };
        if flow.max_children_per_line() != wanted {
            flow.set_max_children_per_line(wanted);
        }
    });

    let rows: RowMap = Rc::new(RefCell::new(HashMap::new()));
    for button in MouseButton::ALL {
        let card = button_tile(state, drawing, toast, &rows, button);
        rows.borrow_mut().insert(button, card.clone());
        flow.append(&card.child);
    }
    ROWS_REGISTRY.with(|cell| *cell.borrow_mut() = Some(Rc::clone(&rows)));

    section.append(&title);
    section.append(&description);
    section.append(&flow);
    (section, rows)
}

/// Stores an assignment for a button and reflects it in the tile dropdown and
/// the diagram leader label; `None` restores the native behaviour.
fn set_assignment(
    state: &Rc<RefCell<HudState>>,
    rows: &RowMap,
    drawing: &gtk4::DrawingArea,
    button: MouseButton,
    action: Option<String>
) {
    let index = action.as_ref().map_or(0, |label| {
        REMAP_TARGETS
            .iter()
            .position(|(target, _)| target == label)
            .and_then(|pos| u32::try_from(pos + 1).ok())
            .unwrap_or(0)
    });
    match action {
        None => {
            state.borrow_mut().assignments.remove(&button);
        }
        Some(action) => {
            state.borrow_mut().assignments.insert(button, action);
        }
    }
    if let Some(tile) = rows.borrow().get(&button) {
        tile.dropdown.set_selected(index);
    }
    drawing.queue_draw();
    queue_leaders();
}
