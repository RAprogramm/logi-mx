// SPDX-FileCopyrightText: 2025-2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Buttons page: the adaptive flow of button tiles, each pairing a device
//! button with the dropdown that assigns its behaviour in firmware, plus the
//! per-application software remap of the two thumb side buttons.

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc
};

use gtk4::{
    Box as GtkBox, Button, DropDown, Entry, EventControllerKey, EventControllerMotion, FlowBox,
    FlowBoxChild, Label, Orientation, ScrolledWindow, SelectionMode, glib
};
use libadwaita::{
    ActionRow, ApplicationWindow, PreferencesGroup, Toast, ToastOverlay, prelude::*
};
use logi_mx_daemon::ipc::{self, Response};
use logi_mx_driver::{
    devices::{Action, ButtonId, control_id},
    keys::key_name
};

use super::{
    HudState, MouseButton, ROWS_REGISTRY, RowMap, Tile, keybinds::open_keybind_picker,
    page_scroll, queue_leaders, report, sync_row_focus
};

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

/// Builds one button tile: a card with the button name and its assignment
/// dropdown; picking an entry writes the remap into the mouse firmware
/// instantly and reverts the choice when the daemon refuses.
fn button_tile(
    state: &Rc<RefCell<HudState>>,
    drawing: &gtk4::DrawingArea,
    toast: &ToastOverlay,
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
    let applied = Rc::new(Cell::new(0_u32));
    dropdown.connect_selected_notify(move |picker| {
        let index = picker.selected();
        if index == applied.get() {
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
            applied.set(index);
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
            toast_assign.add_toast(Toast::new(&message));
        } else {
            picker.set_selected(applied.get());
            toast_assign.add_toast(Toast::new("Daemon unavailable — change not applied"));
        }
    });

    Tile {
        child,
        dropdown
    }
}

/// Builds the button tiles: an adaptive flow of cards, each pairing the
/// button with a dropdown that assigns its behaviour straight into the mouse
/// firmware. The flow fits one or two columns to the available width.
/// Hovering a tile highlights the button on the diagram.
fn buttons_list_section(
    state: &Rc<RefCell<HudState>>,
    drawing: &gtk4::DrawingArea,
    toast: &ToastOverlay
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

/// Which remap set the thumb rows currently edit.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
enum RemapScope {
    /// The global mappings, active when no profile matches.
    #[default]
    Global,
    /// A per-application profile keyed by the Hyprland window class.
    App(String)
}

/// Everything the Buttons page knows about the stored software remaps.
#[derive(Default)]
struct ButtonStore {
    global:   HashMap<ButtonId, Action>,
    profiles: HashMap<String, HashMap<ButtonId, Action>>,
    scope:    RemapScope
}

impl ButtonStore {
    /// The mapping in force for a button in the currently selected scope.
    fn action(&self, button: ButtonId) -> Option<&Action> {
        match &self.scope {
            RemapScope::Global => self.global.get(&button),
            RemapScope::App(app) => self.profiles.get(app).and_then(|p| p.get(&button))
        }
    }

    /// The request that binds `action` to a button in the current scope.
    fn bind_request(&self, button: ButtonId, action: Action) -> ipc::Request {
        match &self.scope {
            RemapScope::Global => ipc::Request::SetButton {
                button,
                action: Box::new(action)
            },
            RemapScope::App(app_id) => ipc::Request::SetAppButton {
                app_id: app_id.clone(),
                button,
                action: Box::new(action)
            }
        }
    }

    /// The request that restores a button in the current scope.
    fn clear_request(&self, button: ButtonId) -> ipc::Request {
        match &self.scope {
            RemapScope::Global => ipc::Request::ClearButton {
                button
            },
            RemapScope::App(app_id) => ipc::Request::ClearAppButton {
                app_id: app_id.clone(),
                button
            }
        }
    }

    /// Mirrors a successful bind locally so scope reloads stay accurate.
    fn record(&mut self, button: ButtonId, action: Action) {
        match &mut self.scope {
            RemapScope::Global => {
                self.global.insert(button, action);
            }
            RemapScope::App(app) => {
                self.profiles
                    .entry(app.clone())
                    .or_default()
                    .insert(button, action);
            }
        }
    }

    /// Mirrors a successful clear locally.
    fn forget(&mut self, button: ButtonId) {
        match &mut self.scope {
            RemapScope::Global => {
                self.global.remove(&button);
            }
            RemapScope::App(app) => {
                if let Some(profile) = self.profiles.get_mut(app) {
                    profile.remove(&button);
                    if profile.is_empty() {
                        self.profiles.remove(app);
                    }
                }
            }
        }
    }
}

/// Shared handle to the button remap store.
type StoreCell = Rc<RefCell<ButtonStore>>;

/// Pending key capture: the button being bound and the evdev codes pressed
/// so far, in press order.
type RemapCapture = Rc<RefCell<Option<(ButtonId, Vec<u16>)>>>;

/// Shared registry of the built remap rows.
type RemapTiles = Rc<RefCell<HashMap<ButtonId, RemapTile>>>;

/// Thumb buttons offered for software remapping, with display labels.
const REMAP_BUTTONS: [(ButtonId, &str); 2] =
    [(ButtonId::Back, "Back"), (ButtonId::Forward, "Forward")];

/// Action kinds offered on every remap row.
const REMAP_KINDS: [&str; 3] = ["Native", "Keys", "Command"];

/// One editable software remap: button, current action and its widgets.
#[derive(Clone)]
struct RemapTile {
    child:   FlowBoxChild,
    command: Rc<RefCell<String>>,
    binding: GtkBox,
    entry:   Entry,
    kind:    DropDown,
    capture: Button,
    hypr:    Button
}

impl RemapTile {
    /// Reflects a bound key sequence as monospace chips.
    fn set_keys(&self, keys: &[String]) {
        while let Some(child) = self.binding.first_child() {
            self.binding.remove(&child);
        }
        if keys.is_empty() {
            let placeholder = Label::new(Some("Native"));
            placeholder.add_css_class("dim-label");
            placeholder.add_css_class("caption");
            self.binding.append(&placeholder);
        } else {
            for (index, key) in keys.iter().enumerate() {
                if index > 0 {
                    let plus = Label::new(Some("+"));
                    plus.add_css_class("hud-chip-sep");
                    self.binding.append(&plus);
                }
                let chip = Label::new(Some(&pretty_key(key)));
                chip.add_css_class("hud-chip");
                self.binding.append(&chip);
            }
        }
        self.binding.set_tooltip_text(Some(&keys.join(" + ")));
        self.kind.set_selected(1);
        self.show_command(false);
    }

    /// Reflects a bound shell command and switches the row to command mode.
    fn set_command(&self, command: String) {
        self.entry.set_text(&command);
        *self.command.borrow_mut() = command;
        self.kind.set_selected(2);
        self.show_command(true);
    }

    /// Reflects the native (unmapped) state.
    fn set_native(&self) {
        self.set_keys(&[]);
        self.kind.set_selected(0);
    }

    /// Switches the editable control between key chips and command entry.
    fn show_command(&self, command: bool) {
        self.binding.set_visible(!command);
        self.capture.set_visible(!command);
        self.hypr.set_visible(!command);
        self.entry.set_visible(command);
    }

    /// Reflects a stored action, or native when there is none.
    fn apply(&self, action: Option<&Action>) {
        match action {
            Some(Action::Keypress {
                keys
            }) => self.set_keys(keys),
            Some(Action::Command {
                command
            }) => self.set_command(command.clone()),
            _ => self.set_native()
        }
    }
}

/// Human form of one evdev key name, e.g. `KEY_LEFTCTRL` → `Left Ctrl`.
fn pretty_key(name: &str) -> String {
    name.trim_start_matches("KEY_")
        .split('_')
        .map(|word| {
            let lower = word.to_lowercase();
            let mut chars = lower.chars();
            let head: String = chars
                .next()
                .map_or(String::new(), |c| c.to_uppercase().collect());
            head + chars.as_str()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Disarms a pending capture and restores the row button.
fn disarm_capture(capture: &RemapCapture, tiles: &RemapTiles) {
    if let Some((button, _)) = capture.borrow_mut().take()
        && let Some(tile) = tiles.borrow().get(&button)
    {
        tile.capture.set_label("Capture");
        tile.capture.remove_css_class("suggested");
    }
}

/// Profile application ids in dropdown order.
fn scope_apps(store: &ButtonStore) -> Vec<String> {
    let mut apps: Vec<String> = store.profiles.keys().cloned().collect();
    apps.sort();
    apps
}

/// Refills the scope dropdown after a profile was added or removed: index 0
/// stays "Global", the rest are the profile apps in display order.
fn refresh_scope_model(model: &gtk4::StringList, store: &ButtonStore) {
    let extra = model.n_items().saturating_sub(1);
    let apps = scope_apps(store);
    let refs: Vec<&str> = apps.iter().map(String::as_str).collect();
    model.splice(1, extra, &refs);
}

/// Reloads every row from the store mappings of the current scope; the
/// loading guard suppresses the IPC writes the programmatic picker updates
/// would otherwise trigger.
fn apply_scope(tiles: &RemapTiles, store: &StoreCell, loading: &Cell<bool>) {
    loading.set(true);
    for (button, tile) in tiles.borrow().iter() {
        tile.apply(store.borrow().action(*button));
    }
    loading.set(false);
}

/// Creates the profile for `app` when missing and switches the rows to it.
fn select_app_scope(
    app: &str,
    store: &StoreCell,
    model: &gtk4::StringList,
    picker: &DropDown,
    tiles: &RemapTiles,
    loading: &Cell<bool>
) {
    let created = {
        let mut guard = store.borrow_mut();
        let created = !guard.profiles.contains_key(app);
        guard.profiles.entry(app.to_string()).or_default();
        guard.scope = RemapScope::App(app.to_string());
        created
    };
    if created {
        refresh_scope_model(model, &store.borrow());
    }
    let index = scope_apps(&store.borrow())
        .iter()
        .position(|candidate| candidate == app)
        .and_then(|pos| u32::try_from(pos).ok())
        .map_or(0, |pos| 1 + pos);
    loading.set(true);
    picker.set_selected(index);
    loading.set(false);
    apply_scope(tiles, store, loading);
}

/// Arms the key capture for `button`, disarming any other pending capture.
fn arm_capture(button: ButtonId, capture: &RemapCapture, tiles: &RemapTiles) {
    disarm_capture(capture, tiles);
    *capture.borrow_mut() = Some((button, Vec::new()));
    if let Some(tile) = tiles.borrow().get(&button) {
        tile.capture.set_label("Press keys…");
        tile.capture.add_css_class("suggested");
    }
}

/// Wires the kind picker: Native clears through the daemon, Keys arms the
/// capture, Command reveals the shell entry.
fn wire_remap_kind(
    kind: &DropDown,
    button: ButtonId,
    toast: &ToastOverlay,
    capture: &RemapCapture,
    tiles: &RemapTiles,
    store: &StoreCell,
    loading: &Rc<Cell<bool>>
) {
    let tiles_pick = Rc::clone(tiles);
    let store_pick = Rc::clone(store);
    let toast_pick = toast.clone();
    let loading_pick = Rc::clone(loading);
    let capture_pick = Rc::clone(capture);
    kind.connect_selected_notify(move |picker| {
        if loading_pick.get() {
            return;
        }
        match picker.selected() {
            0 => {
                let request = store_pick.borrow().clear_request(button);
                let response = ipc::try_request(&request);
                if matches!(response, Some(Response::Ok)) {
                    store_pick.borrow_mut().forget(button);
                    if let Some(tile) = tiles_pick.borrow().get(&button) {
                        loading_pick.set(true);
                        tile.set_native();
                        loading_pick.set(false);
                    }
                }
                report(response, &toast_pick, "Native behaviour restored");
            }
            1 => arm_capture(button, &capture_pick, &tiles_pick),
            _ => {
                if let Some(tile) = tiles_pick.borrow().get(&button) {
                    tile.show_command(true);
                }
            }
        }
    });
}

/// Wires the shell entry: Enter binds the command in the store's scope.
fn wire_remap_command(
    entry: &Entry,
    button: ButtonId,
    toast: &ToastOverlay,
    tiles: &RemapTiles,
    store: &StoreCell
) {
    let toast_cmd = toast.clone();
    let tiles_cmd = Rc::clone(tiles);
    let store_cmd = Rc::clone(store);
    entry.connect_activate(move |field| {
        let text = field.text().trim().to_string();
        if text.is_empty() {
            return;
        }
        let action = Action::Command {
            command: text.clone()
        };
        let request = store_cmd.borrow().bind_request(button, action.clone());
        let response = ipc::try_request(&request);
        if matches!(response, Some(Response::Ok)) {
            store_cmd.borrow_mut().record(button, action);
            if let Some(tile) = tiles_cmd.borrow().get(&button) {
                tile.set_command(text);
            }
        }
        report(response, &toast_cmd, "Button bound");
    });
}

/// Wires the trash button: restores the native behaviour in the scope.
fn wire_remap_clear(
    clear_btn: &Button,
    button: ButtonId,
    toast: &ToastOverlay,
    tiles: &RemapTiles,
    store: &StoreCell,
    loading: &Rc<Cell<bool>>
) {
    let toast_reset = toast.clone();
    let tiles_reset = Rc::clone(tiles);
    let store_reset = Rc::clone(store);
    let loading_reset = Rc::clone(loading);
    clear_btn.connect_clicked(move |_| {
        let request = store_reset.borrow().clear_request(button);
        let response = ipc::try_request(&request);
        if matches!(response, Some(Response::Ok)) {
            store_reset.borrow_mut().forget(button);
            if let Some(tile) = tiles_reset.borrow().get(&button) {
                loading_reset.set(true);
                *tile.command.borrow_mut() = String::new();
                tile.set_native();
                loading_reset.set(false);
            }
        }
        report(response, &toast_reset, "Native behaviour restored");
    });
}

/// Builds one remap row: button name, current binding, kind picker, command
/// entry, capture and clear controls.
#[allow(
    clippy::too_many_arguments,
    reason = "tile builders receive the full set of shared wiring targets"
)]
fn remap_tile(
    button: ButtonId,
    label: &str,
    toast: &ToastOverlay,
    window: &ApplicationWindow,
    capture: &RemapCapture,
    tiles: &RemapTiles,
    store: &StoreCell,
    loading: &Rc<Cell<bool>>
) -> RemapTile {
    let child = FlowBoxChild::new();
    child.add_css_class("hud-tile");
    child.set_hexpand(true);
    let column = GtkBox::new(Orientation::Vertical, 8);

    let head = GtkBox::new(Orientation::Horizontal, 8);
    let heading = Label::new(Some(label));
    heading.add_css_class("title-4");
    heading.set_xalign(0.0);
    let binding = GtkBox::new(Orientation::Horizontal, 4);
    binding.set_halign(gtk4::Align::End);
    binding.set_hexpand(true);
    let entry = Entry::new();
    entry.add_css_class("hud-command");
    entry.set_placeholder_text(Some("shell command, Enter applies"));
    entry.set_hexpand(true);
    entry.set_valign(gtk4::Align::Center);
    entry.set_visible(false);
    head.append(&heading);
    head.append(&binding);
    head.append(&entry);

    let controls = GtkBox::new(Orientation::Horizontal, 8);
    let kind = DropDown::from_strings(&REMAP_KINDS);
    kind.set_valign(gtk4::Align::Center);
    kind.set_tooltip_text(Some("What the button should do"));
    let spring = Label::new(None);
    spring.set_hexpand(true);
    let hypr_btn = Button::with_label("Hyprland…");
    hypr_btn.set_tooltip_text(Some("Pick an existing Hyprland keybind"));
    let capture_btn = Button::with_label("Capture");
    let clear_btn = Button::from_icon_name("user-trash-symbolic");
    clear_btn.add_css_class("flat");
    clear_btn.set_tooltip_text(Some("Restore native behaviour"));
    controls.append(&kind);
    controls.append(&spring);
    controls.append(&hypr_btn);
    controls.append(&capture_btn);
    controls.append(&clear_btn);

    column.append(&head);
    column.append(&controls);
    child.set_child(Some(&column));

    wire_remap_kind(&kind, button, toast, capture, tiles, store, loading);
    wire_remap_command(&entry, button, toast, tiles, store);
    wire_remap_clear(&clear_btn, button, toast, tiles, store, loading);

    let capture_arm = Rc::clone(capture);
    let tiles_arm = Rc::clone(tiles);
    capture_btn.connect_clicked(move |_| arm_capture(button, &capture_arm, &tiles_arm));

    let tiles_hypr = Rc::clone(tiles);
    let store_hypr = Rc::clone(store);
    let loading_hypr = Rc::clone(loading);
    let toast_hypr = toast.clone();
    let window_hypr = window.clone();
    hypr_btn.connect_clicked(move |_| {
        let tiles_cb = Rc::clone(&tiles_hypr);
        let store_cb = Rc::clone(&store_hypr);
        let loading_cb = Rc::clone(&loading_hypr);
        let toast_pick = toast_hypr.clone();
        let toast_cb = toast_hypr.clone();
        open_keybind_picker(&window_hypr, &toast_pick, move |keys| {
            commit_remap_keys(button, &tiles_cb, &store_cb, &loading_cb, &toast_cb, &keys);
        });
    });

    let row = RemapTile {
        child,
        command: Rc::new(RefCell::new(String::new())),
        binding,
        entry,
        kind,
        capture: capture_btn,
        hypr: hypr_btn
    };
    row.set_native();
    row
}

/// Builds the remap scope bar: the dropdown switching between the global
/// mappings and per-application overrides, plus controls to add a profile by
/// window class, adopt the focused window's class, or remove the profile.
fn remap_scope_bar(
    toast: &ToastOverlay,
    store: &StoreCell,
    tiles: &RemapTiles,
    loading: &Rc<Cell<bool>>
) -> (PreferencesGroup, gtk4::StringList) {
    let group = PreferencesGroup::new();
    group.set_title("Per-app remap");
    group.set_description(Some(
        "Override the thumb buttons per application, keyed by the Hyprland window class"
    ));

    let model = gtk4::StringList::new(&["Global"]);
    let picker = DropDown::new(Some(model.clone()), None::<gtk4::Expression>);
    picker.set_valign(gtk4::Align::Center);
    let pick_row = ActionRow::new();
    pick_row.set_title("Editing");
    pick_row.add_suffix(&picker);
    group.add(&pick_row);

    let edit_row = ActionRow::new();
    edit_row.set_title("Add application");
    edit_row.set_subtitle("Hyprland window class of the override profile");
    let controls = GtkBox::new(Orientation::Horizontal, 8);
    controls.set_valign(gtk4::Align::Center);
    let app_entry = Entry::new();
    app_entry.set_placeholder_text(Some("e.g. firefox"));
    app_entry.set_width_chars(14);
    let add_btn = Button::with_label("Add");
    let focused_btn = Button::with_label("Use focused");
    let remove_btn = Button::with_label("Remove");
    controls.append(&app_entry);
    controls.append(&add_btn);
    controls.append(&focused_btn);
    controls.append(&remove_btn);
    edit_row.add_suffix(&controls);
    group.add(&edit_row);

    let store_pick = Rc::clone(store);
    let tiles_pick = Rc::clone(tiles);
    let model_pick = model.clone();
    let loading_pick = Rc::clone(loading);
    picker.connect_selected_notify(move |pick| {
        if loading_pick.get() {
            return;
        }
        let index = pick.selected();
        let scope = if index == 0 {
            RemapScope::Global
        } else {
            let app = model_pick.string(index).map_or(String::new(), String::from);
            RemapScope::App(app)
        };
        store_pick.borrow_mut().scope = scope;
        apply_scope(&tiles_pick, &store_pick, loading_pick.as_ref());
    });

    let on_add: Rc<dyn Fn(String)> = {
        let store_add = Rc::clone(store);
        let tiles_add = Rc::clone(tiles);
        let model_add = model.clone();
        let picker_add = picker.clone();
        let loading_add = Rc::clone(loading);
        Rc::new(move |raw: String| {
            let app = raw.trim();
            if app.is_empty() {
                return;
            }
            select_app_scope(
                app,
                &store_add,
                &model_add,
                &picker_add,
                &tiles_add,
                loading_add.as_ref()
            );
        })
    };
    let entry_add = Rc::clone(&on_add);
    app_entry.connect_activate(move |field| entry_add(field.text().to_string()));
    let btn_add = Rc::clone(&on_add);
    add_btn.connect_clicked(move |_| btn_add(app_entry.text().to_string()));

    wire_scope_profile_buttons(ScopeHandles {
        toast,
        store,
        tiles,
        loading,
        model: &model,
        picker,
        focused_btn: &focused_btn,
        remove_btn: &remove_btn
    });

    (group, model)
}

/// Shared handles the scope bar profile controls mutate.
struct ScopeHandles<'a> {
    toast:       &'a ToastOverlay,
    store:       &'a StoreCell,
    tiles:       &'a RemapTiles,
    loading:     &'a Rc<Cell<bool>>,
    model:       &'a gtk4::StringList,
    picker:      DropDown,
    focused_btn: &'a Button,
    remove_btn:  &'a Button
}

/// Wires the "Use focused" and "Remove" controls of the scope bar: the first
/// adopts the focused window's class as the active profile, the second drops
/// the current profile and returns to the global mappings.
fn wire_scope_profile_buttons(handles: ScopeHandles<'_>) {
    let ScopeHandles {
        toast,
        store,
        tiles,
        loading,
        model,
        picker,
        focused_btn,
        remove_btn
    } = handles;
    let store_focus = Rc::clone(store);
    let tiles_focus = Rc::clone(tiles);
    let model_focus = model.clone();
    let picker_focus = picker.clone();
    let loading_focus = Rc::clone(loading);
    let toast_focus = toast.clone();
    focused_btn.connect_clicked(
        move |_| match ipc::try_request(&ipc::Request::GetFocusedApp) {
            Some(Response::FocusedApp {
                app_id: Some(app)
            }) => select_app_scope(
                &app,
                &store_focus,
                &model_focus,
                &picker_focus,
                &tiles_focus,
                loading_focus.as_ref()
            ),
            _ => toast_focus.add_toast(Toast::new("The desktop reported no focused window"))
        }
    );

    let store_remove = Rc::clone(store);
    let tiles_remove = Rc::clone(tiles);
    let model_remove = model.clone();
    let picker_remove = picker;
    let loading_remove = Rc::clone(loading);
    let toast_remove = toast.clone();
    remove_btn.connect_clicked(move |_| {
        let app = match store_remove.borrow().scope.clone() {
            RemapScope::App(app) => app,
            RemapScope::Global => {
                toast_remove.add_toast(Toast::new("The global mappings cannot be removed"));
                return;
            }
        };
        let response = ipc::try_request(&ipc::Request::RemoveAppButtonProfile {
            app_id: app.clone()
        });
        if matches!(response, Some(Response::Ok)) {
            {
                let mut guard = store_remove.borrow_mut();
                guard.profiles.remove(&app);
                guard.scope = RemapScope::Global;
            }
            refresh_scope_model(&model_remove, &store_remove.borrow());
            loading_remove.set(true);
            picker_remove.set_selected(0);
            loading_remove.set(false);
            apply_scope(&tiles_remove, &store_remove, loading_remove.as_ref());
        }
        report(response, &toast_remove, "Profile removed");
    });
}

/// Binds a finished key chord to `button` through the daemon and reflects
/// it on the row; shared by the window key capture and the Hyprland keybind
/// picker. The loading guard keeps the programmatic kind switch from
/// re-arming the capture.
fn commit_remap_keys(
    button: ButtonId,
    tiles: &RemapTiles,
    store: &StoreCell,
    loading: &Cell<bool>,
    toast: &ToastOverlay,
    names: &[String]
) {
    let Some(tile) = tiles.borrow().get(&button).cloned() else {
        return;
    };
    let action = Action::Keypress {
        keys: names.to_vec()
    };
    let request = store.borrow().bind_request(button, action.clone());
    let response = ipc::try_request(&request);
    if matches!(response, Some(Response::Ok)) {
        store.borrow_mut().record(button, action);
        loading.set(true);
        tile.set_keys(names);
        loading.set(false);
    }
    report(response, toast, "Button bound");
}

/// Installs the window-level capture listener for button binding.
///
/// Runs in the capture phase so chords never reach the diagram shortcuts or
/// the Escape-to-close controller while a binding is armed.
fn attach_remap_capture(
    window: &ApplicationWindow,
    capture: &RemapCapture,
    tiles: &RemapTiles,
    store: &StoreCell,
    loading: &Rc<Cell<bool>>,
    toast: &ToastOverlay
) {
    let controller = EventControllerKey::new();
    controller.set_propagation_phase(gtk4::PropagationPhase::Capture);

    let capture_press = Rc::clone(capture);
    let tiles_press = Rc::clone(tiles);
    controller.connect_key_pressed(move |_controller, key, code, _state| {
        if capture_press.borrow().is_none() {
            return glib::Propagation::Proceed;
        }
        if key == gtk4::gdk::Key::Escape {
            disarm_capture(&capture_press, &tiles_press);
            return glib::Propagation::Stop;
        }
        if let Some(evdev) = code.checked_sub(8).and_then(|raw| u16::try_from(raw).ok())
            && key_name(evdev).is_some()
            && let Some((_, pressed)) = capture_press.borrow_mut().as_mut()
            && !pressed.contains(&evdev)
        {
            pressed.push(evdev);
        }
        glib::Propagation::Stop
    });

    let capture_release = Rc::clone(capture);
    let tiles_release = Rc::clone(tiles);
    let store_release = Rc::clone(store);
    let loading_release = Rc::clone(loading);
    let toast_release = toast.clone();
    controller.connect_key_released(move |_controller, _key, _code, _state| {
        let Some((button, pressed)) = capture_release.borrow().clone() else {
            return;
        };
        if pressed.is_empty() {
            return;
        }
        disarm_capture(&capture_release, &tiles_release);
        let names: Vec<String> = pressed
            .iter()
            .filter_map(|code| key_name(*code))
            .map(str::to_string)
            .collect();
        commit_remap_keys(
            button,
            &tiles_release,
            &store_release,
            loading_release.as_ref(),
            &toast_release,
            &names
        );
    });

    window.add_controller(controller);
}

/// Builds the thumb remap section: the scope bar and one row per thumb side
/// button, editing the daemon-side software remap rather than the firmware
/// assignment above it.
fn thumb_remap_section(toast: &ToastOverlay, window: &ApplicationWindow) -> GtkBox {
    let section = GtkBox::new(Orientation::Vertical, 8);
    let store: StoreCell = Rc::new(RefCell::new(ButtonStore::default()));
    let tiles: RemapTiles = Rc::new(RefCell::new(HashMap::new()));
    let capture: RemapCapture = Rc::new(RefCell::new(None));
    let loading: Rc<Cell<bool>> = Rc::new(Cell::new(false));
    let (scope_group, scope_model) = remap_scope_bar(toast, &store, &tiles, &loading);

    let flow = FlowBox::new();
    flow.add_css_class("hud-tiles");
    flow.set_selection_mode(SelectionMode::None);
    flow.set_min_children_per_line(1);
    flow.set_max_children_per_line(1);
    flow.set_row_spacing(12);
    for (button, label) in REMAP_BUTTONS {
        let tile = remap_tile(
            button, label, toast, window, &capture, &tiles, &store, &loading
        );
        tiles.borrow_mut().insert(button, tile.clone());
        flow.append(&tile.child);
    }

    section.append(&scope_group);
    section.append(&flow);

    if let Some(Response::ButtonsConfig {
        buttons,
        profiles
    }) = ipc::try_request(&ipc::Request::GetButtons)
    {
        {
            let mut guard = store.borrow_mut();
            guard.global = buttons;
            guard.profiles = profiles;
        }
        refresh_scope_model(&scope_model, &store.borrow());
        apply_scope(&tiles, &store, &loading);
    }

    attach_remap_capture(window, &capture, &tiles, &store, &loading, toast);
    section
}

/// Creates the assignment page: adaptive button tiles that highlight the
/// diagram, each carrying its own assignment dropdown, followed by the
/// per-application software remap of the two thumb side buttons.
pub(super) fn buttons_page(
    state: &Rc<RefCell<HudState>>,
    drawing: &gtk4::DrawingArea,
    window: &ApplicationWindow,
    toast: &ToastOverlay
) -> ScrolledWindow {
    let scroll = page_scroll();
    scroll
        .vadjustment()
        .connect_value_changed(move |_| queue_leaders());
    let box_ = GtkBox::new(Orientation::Vertical, 0);
    let (buttons_section, _rows) = buttons_list_section(state, drawing, toast);
    box_.append(&buttons_section);
    box_.append(&thumb_remap_section(toast, window));
    scroll.set_child(Some(&box_));
    scroll
}
