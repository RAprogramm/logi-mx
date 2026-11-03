// SPDX-FileCopyrightText: 2025-2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Gestures page: the master switch, the binding-scope bar and one editable
//! tile per swipe direction, with window-level key capture for binding.

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc
};

use gtk4::{
    Box as GtkBox, Button, DropDown, Entry, EventControllerKey, FlowBox, FlowBoxChild, Label,
    Orientation, ScrolledWindow, SelectionMode, Switch, glib
};
use libadwaita::{ActionRow, ApplicationWindow, PreferencesGroup, prelude::*};
use logi_mx_daemon::ipc::{self, Response};
use logi_mx_driver::{
    config::PieConfig,
    devices::{Action, Gesture, GestureDirection, GestureMode},
    keys::key_name
};

use super::{keybinds::open_keybind_picker, page_body, page_scroll, report, toasts::Toasts};

/// Gesture tiles in display order: direction and human label. The four
/// diagonals only ever fire from the radial pie menu, but they bind through
/// the same store and IPC path as the cardinals.
const GESTURE_TILES: [(GestureDirection, &str); 9] = [
    (GestureDirection::Up, "Swipe up"),
    (GestureDirection::Down, "Swipe down"),
    (GestureDirection::Left, "Swipe left"),
    (GestureDirection::Right, "Swipe right"),
    (GestureDirection::UpLeft, "Swipe up-left"),
    (GestureDirection::UpRight, "Swipe up-right"),
    (GestureDirection::DownLeft, "Swipe down-left"),
    (GestureDirection::DownRight, "Swipe down-right"),
    (GestureDirection::None, "Click")
];

/// Fire moments offered on every gesture tile.
const GESTURE_MODES: [&str; 2] = ["On press", "On release"];

/// Action kinds offered on every gesture tile.
const GESTURE_KINDS: [&str; 2] = ["Keys", "Command"];

/// Shared registry of the built gesture tiles.
type GestureTiles = Rc<RefCell<HashMap<GestureDirection, GestureTile>>>;

/// Which binding set the tiles currently edit.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
enum BindingScope {
    /// The global bindings, active when no profile matches.
    #[default]
    Global,
    /// A per-application profile keyed by the Hyprland window class.
    App(String)
}

/// Everything the Gestures page knows about the stored bindings.
#[derive(Default)]
struct GestureStore {
    global:   Vec<Gesture>,
    profiles: HashMap<String, Vec<Gesture>>,
    scope:    BindingScope
}

impl GestureStore {
    /// The bindings visible in the currently selected scope.
    fn bindings(&self) -> &[Gesture] {
        match &self.scope {
            BindingScope::Global => &self.global,
            BindingScope::App(app) => self.profiles.get(app).map_or(&[], Vec::as_slice)
        }
    }

    /// The request that binds `action` in the current scope.
    fn bind_request(
        &self,
        direction: GestureDirection,
        mode: GestureMode,
        action: Action
    ) -> ipc::Request {
        match &self.scope {
            BindingScope::Global => ipc::Request::SetGesture {
                direction,
                mode,
                action: Box::new(action)
            },
            BindingScope::App(app_id) => ipc::Request::SetAppGesture {
                app_id: app_id.clone(),
                direction,
                mode,
                action: Box::new(action)
            }
        }
    }

    /// The request that unbinds `direction` in the current scope.
    fn clear_request(&self, direction: GestureDirection) -> ipc::Request {
        match &self.scope {
            BindingScope::Global => ipc::Request::ClearGesture {
                direction
            },
            BindingScope::App(app_id) => ipc::Request::ClearAppGesture {
                app_id: app_id.clone(),
                direction
            }
        }
    }

    /// Mirrors a successful bind locally so scope reloads stay accurate.
    fn record(&mut self, direction: GestureDirection, mode: GestureMode, action: Action) {
        let list = match &self.scope {
            BindingScope::Global => &mut self.global,
            BindingScope::App(app) => self.profiles.entry(app.clone()).or_default()
        };
        list.retain(|gesture| gesture.direction != direction);
        list.push(Gesture {
            direction,
            mode,
            action: Box::new(action)
        });
    }

    /// Mirrors a successful clear locally.
    fn forget(&mut self, direction: GestureDirection) {
        match &self.scope {
            BindingScope::Global => self.global.retain(|gesture| gesture.direction != direction),
            BindingScope::App(app) => {
                if let Some(list) = self.profiles.get_mut(app) {
                    list.retain(|gesture| gesture.direction != direction);
                    if list.is_empty() {
                        self.profiles.remove(app);
                    }
                }
            }
        }
    }
}

/// Shared handle to the gesture binding store.
type StoreCell = Rc<RefCell<GestureStore>>;

/// Pending key capture: the direction being bound and the evdev codes
/// pressed so far, in press order.
type GestureCapture = Rc<RefCell<Option<(GestureDirection, Vec<u16>)>>>;

/// One editable gesture binding: direction, action and its widgets.
#[derive(Clone)]
struct GestureTile {
    child:   FlowBoxChild,
    keys:    Rc<RefCell<Vec<String>>>,
    command: Rc<RefCell<String>>,
    binding: GtkBox,
    entry:   Entry,
    kind:    DropDown,
    mode:    DropDown,
    capture: Button,
    hypr:    Button
}

impl GestureTile {
    /// Reflects a bound key sequence on the tile as monospace chips.
    fn set_keys(&self, keys: Vec<String>) {
        while let Some(child) = self.binding.first_child() {
            self.binding.remove(&child);
        }
        if keys.is_empty() {
            let placeholder = Label::new(Some("Not bound"));
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
        self.binding.set_tooltip_text(Some(&binding_text(&keys)));
        *self.keys.borrow_mut() = keys;
        self.kind.set_selected(0);
        self.show_command(false);
    }

    /// Reflects a bound shell command and switches the tile to command mode.
    fn set_command(&self, command: String) {
        self.entry.set_text(&command);
        *self.command.borrow_mut() = command;
        self.kind.set_selected(1);
        self.show_command(true);
    }

    /// Switches the editable control between key capture and command entry.
    fn show_command(&self, command: bool) {
        self.binding.set_visible(!command);
        self.capture.set_visible(!command);
        self.hypr.set_visible(!command);
        self.entry.set_visible(command);
    }

    /// The fire moment currently selected on this tile.
    fn current_mode(&self) -> GestureMode {
        picker_mode(&self.mode)
    }

    /// The action currently edited on this tile, if any is complete.
    fn current_action(&self) -> Option<Action> {
        if self.kind.selected() == 1 {
            let command = self.command.borrow().clone();
            (!command.is_empty()).then_some(Action::Command {
                command
            })
        } else {
            let keys = self.keys.borrow().clone();
            (!keys.is_empty()).then_some(Action::Keypress {
                keys
            })
        }
    }
}

/// Renders a key sequence as one line, e.g. `Left Ctrl + Right`.
fn binding_text(keys: &[String]) -> String {
    keys.iter()
        .map(|name| pretty_key(name))
        .collect::<Vec<_>>()
        .join(" + ")
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

/// Disarms a pending capture and restores the tile button.
fn disarm_capture(capture: &GestureCapture, tiles: &GestureTiles) {
    if let Some((direction, _)) = capture.borrow_mut().take()
        && let Some(tile) = tiles.borrow().get(&direction)
    {
        tile.capture.set_label("Capture");
        tile.capture.remove_css_class("suggested");
    }
}

/// Profile application ids in dropdown order.
fn scope_apps(store: &GestureStore) -> Vec<String> {
    let mut apps: Vec<String> = store.profiles.keys().cloned().collect();
    apps.sort();
    apps
}

/// Refills the scope dropdown after a profile was added or removed: index 0
/// stays "Global", the rest are the profile apps in display order.
fn refresh_scope_model(model: &gtk4::StringList, store: &GestureStore) {
    let extra = model.n_items().saturating_sub(1);
    let apps = scope_apps(store);
    let refs: Vec<&str> = apps.iter().map(String::as_str).collect();
    model.splice(1, extra, &refs);
}

/// Reloads every tile from the store bindings of the current scope. Tile
/// updates fire the mode picker, so the shared loading guard suppresses the
/// resulting IPC writes while the tiles are being refilled.
fn apply_scope(tiles: &GestureTiles, store: &StoreCell, loading: &Cell<bool>) {
    let bindings = store.borrow().bindings().to_vec();
    loading.set(true);
    for (direction, tile) in tiles.borrow().iter() {
        tile.set_keys(Vec::new());
        tile.entry.set_text("");
        *tile.command.borrow_mut() = String::new();
        let Some(gesture) = bindings
            .iter()
            .find(|binding| binding.direction == *direction)
        else {
            continue;
        };
        tile.mode
            .set_selected(u32::from(gesture.mode != GestureMode::OnPress));
        match gesture.action.as_ref() {
            Action::Keypress {
                keys
            } => tile.set_keys(keys.clone()),
            Action::Command {
                command
            } => tile.set_command(command.clone()),
            _ => {}
        }
    }
    loading.set(false);
}

/// Creates the profile for `app` when missing and switches the tiles to it.
fn select_app_scope(
    app: &str,
    store: &StoreCell,
    model: &gtk4::StringList,
    picker: &DropDown,
    tiles: &GestureTiles,
    loading: &Cell<bool>
) {
    let created = {
        let mut guard = store.borrow_mut();
        let created = !guard.profiles.contains_key(app);
        guard.profiles.entry(app.to_string()).or_default();
        guard.scope = BindingScope::App(app.to_string());
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

/// Builds one gesture tile: direction, current binding, action kind, fire
/// moment and the capture/clear controls.
#[allow(
    clippy::too_many_arguments,
    reason = "tile builders receive the full set of shared wiring targets"
)]
fn gesture_tile(
    direction: GestureDirection,
    label: &str,
    toast: &Toasts,
    capture: &GestureCapture,
    tiles: &GestureTiles,
    store: &StoreCell,
    loading: &Rc<Cell<bool>>
) -> GestureTile {
    let child = FlowBoxChild::new();
    child.add_css_class("hud-tile");
    child.set_hexpand(true);
    let column = GtkBox::new(Orientation::Vertical, 8);

    let head = GtkBox::new(Orientation::Horizontal, 8);
    let title = Label::new(Some(label));
    title.add_css_class("title-4");
    title.set_xalign(0.0);
    let binding = GtkBox::new(Orientation::Horizontal, 4);
    binding.set_halign(gtk4::Align::End);
    binding.set_hexpand(true);
    let entry = Entry::new();
    entry.add_css_class("hud-command");
    entry.set_placeholder_text(Some("shell command, Enter applies"));
    entry.set_hexpand(true);
    entry.set_valign(gtk4::Align::Center);
    entry.set_visible(false);
    head.append(&title);
    head.append(&binding);
    head.append(&entry);

    let controls = GtkBox::new(Orientation::Horizontal, 8);
    let kind = DropDown::from_strings(&GESTURE_KINDS);
    kind.set_valign(gtk4::Align::Center);
    kind.set_tooltip_text(Some("Action kind"));
    let mode = DropDown::from_strings(&GESTURE_MODES);
    mode.set_valign(gtk4::Align::Center);
    let spring = Label::new(None);
    spring.set_hexpand(true);
    let hypr_btn = Button::with_label("Hyprland…");
    hypr_btn.set_tooltip_text(Some("Pick an existing Hyprland keybind"));
    let capture_btn = Button::with_label("Capture");
    let clear_btn = Button::from_icon_name("user-trash-symbolic");
    clear_btn.add_css_class("flat");
    clear_btn.set_tooltip_text(Some("Clear binding"));
    controls.append(&kind);
    controls.append(&mode);
    controls.append(&spring);
    controls.append(&hypr_btn);
    controls.append(&capture_btn);
    controls.append(&clear_btn);

    column.append(&head);
    column.append(&controls);
    child.set_child(Some(&column));

    let keys: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
    let command: Rc<RefCell<String>> = Rc::new(RefCell::new(String::new()));

    let tiles_kind = Rc::clone(tiles);
    kind.connect_selected_notify(move |picker| {
        if let Some(tile) = tiles_kind.borrow().get(&direction) {
            tile.show_command(picker.selected() == 1);
        }
    });
    wire_command_entry(&entry, toast, tiles, store, direction);
    wire_moment_update(&mode, toast, tiles, store, loading, direction);

    let toast_clear = toast.clone();
    let tiles_clear = Rc::clone(tiles);
    let store_clear = Rc::clone(store);
    clear_btn.connect_clicked(move |_| {
        let request = store_clear.borrow().clear_request(direction);
        let response = ipc::try_request(&request);
        if matches!(response, Some(Response::Ok)) {
            store_clear.borrow_mut().forget(direction);
            if let Some(tile) = tiles_clear.borrow().get(&direction) {
                *tile.command.borrow_mut() = String::new();
                tile.set_keys(Vec::new());
            }
        }
        report(response, &toast_clear, "Gesture cleared");
    });

    let capture_arm = Rc::clone(capture);
    let tiles_arm = Rc::clone(tiles);
    capture_btn.connect_clicked(move |_| {
        disarm_capture(&capture_arm, &tiles_arm);
        *capture_arm.borrow_mut() = Some((direction, Vec::new()));
        if let Some(tile) = tiles_arm.borrow().get(&direction) {
            tile.capture.set_label("Press keys…");
            tile.capture.add_css_class("suggested");
        }
    });

    wire_gesture_hypr(&hypr_btn, direction, toast, tiles, store);

    let gesture = GestureTile {
        child,
        keys,
        command,
        binding,
        entry,
        kind,
        mode,
        capture: capture_btn,
        hypr: hypr_btn
    };
    gesture.set_keys(Vec::new());
    gesture
}

/// Wires the Hyprland chooser button of a gesture tile: opens the picker and
/// commits the chosen chord through the shared store path.
fn wire_gesture_hypr(
    button: &Button,
    direction: GestureDirection,
    toast: &Toasts,
    tiles: &GestureTiles,
    store: &StoreCell
) {
    let tiles_hypr = Rc::clone(tiles);
    let store_hypr = Rc::clone(store);
    let toast_hypr = toast.clone();
    button.connect_clicked(move |_| {
        let tiles_cb = Rc::clone(&tiles_hypr);
        let store_cb = Rc::clone(&store_hypr);
        let toast_pick = toast_hypr.clone();
        let toast_cb = toast_hypr.clone();
        open_keybind_picker(&toast_pick, move |keys| {
            commit_gesture_keys(direction, &tiles_cb, &store_cb, &toast_cb, keys);
        });
    });
}

/// Wires the command entry of a gesture tile: Enter binds the shell command
/// for the direction at the tile's current fire moment, in the store's
/// active scope.
fn wire_command_entry(
    entry: &Entry,
    toast: &Toasts,
    tiles: &GestureTiles,
    store: &StoreCell,
    direction: GestureDirection
) {
    let toast_bind = toast.clone();
    let tiles_bind = Rc::clone(tiles);
    let store_bind = Rc::clone(store);
    entry.connect_activate(move |field| {
        let text = field.text().trim().to_string();
        if text.is_empty() {
            return;
        }
        let Some(found) = tiles_bind.borrow().get(&direction).cloned() else {
            return;
        };
        let mode = found.current_mode();
        let action = Action::Command {
            command: text.clone()
        };
        let request = store_bind
            .borrow()
            .bind_request(direction, mode, action.clone());
        let response = ipc::try_request(&request);
        if matches!(response, Some(Response::Ok)) {
            *found.command.borrow_mut() = text;
            store_bind.borrow_mut().record(direction, mode, action);
        }
        report(response, &toast_bind, "Gesture bound");
    });
}

/// Wires the fire-moment picker: changing it re-sends the action currently
/// edited on the tile with the new moment, unless the picker is only being
/// refilled by a scope switch.
fn wire_moment_update(
    mode: &DropDown,
    toast: &Toasts,
    tiles: &GestureTiles,
    store: &StoreCell,
    loading: &Rc<Cell<bool>>,
    direction: GestureDirection
) {
    let toast_pick = toast.clone();
    let tiles_pick = Rc::clone(tiles);
    let store_pick = Rc::clone(store);
    let loading_pick = Rc::clone(loading);
    mode.connect_selected_notify(move |picker| {
        if loading_pick.get() {
            return;
        }
        let Some(found) = tiles_pick.borrow().get(&direction).cloned() else {
            return;
        };
        let Some(action) = found.current_action() else {
            return;
        };
        let moment = picker_mode(picker);
        let request = store_pick
            .borrow()
            .bind_request(direction, moment, action.clone());
        let response = ipc::try_request(&request);
        if matches!(response, Some(Response::Ok)) {
            store_pick.borrow_mut().record(direction, moment, action);
        }
        report(response, &toast_pick, "Gesture updated");
    });
}

/// Maps a fire-moment picker selection to its `GestureMode`.
fn picker_mode(picker: &DropDown) -> GestureMode {
    if picker.selected() == 0 {
        GestureMode::OnPress
    } else {
        GestureMode::OnRelease
    }
}

/// Builds the scope bar: the dropdown switching between the global bindings
/// and per-application profiles, plus controls to add a profile by window
/// class, adopt the focused window's class, or remove the current profile.
fn gesture_scope_bar(
    toast: &Toasts,
    store: &StoreCell,
    tiles: &GestureTiles,
    loading: &Rc<Cell<bool>>
) -> (PreferencesGroup, gtk4::StringList) {
    let group = PreferencesGroup::new();
    group.set_title("Binding scope");
    group.set_description(Some(
        "Edit the global bindings or a per-application override keyed by the window class"
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
            BindingScope::Global
        } else {
            let app = model_pick.string(index).map_or(String::new(), String::from);
            BindingScope::App(app)
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

    wire_scope_profile_buttons(
        ScopeHandles {
            toast,
            store,
            tiles,
            loading,
            model: &model,
            picker
        },
        &focused_btn,
        &remove_btn
    );

    (group, model)
}

/// Shared handles the scope bar controls mutate.
struct ScopeHandles<'a> {
    toast:   &'a Toasts,
    store:   &'a StoreCell,
    tiles:   &'a GestureTiles,
    loading: &'a Rc<Cell<bool>>,
    model:   &'a gtk4::StringList,
    picker:  DropDown
}

/// Wires the "Use focused" and "Remove" controls of the scope bar: the first
/// adopts the focused window's class as the active profile, the second drops
/// the current profile and returns to the global bindings.
fn wire_scope_profile_buttons(
    handles: ScopeHandles<'_>,
    focused_btn: &Button,
    remove_btn: &Button
) {
    let ScopeHandles {
        toast,
        store,
        tiles,
        loading,
        model,
        picker
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
            _ => toast_focus.show("The desktop reported no focused window")
        }
    );

    let store_remove = Rc::clone(store);
    let tiles_remove = Rc::clone(tiles);
    let model_remove = model.clone();
    let loading_remove = Rc::clone(loading);
    let toast_remove = toast.clone();
    remove_btn.connect_clicked(move |_| {
        let app = match store_remove.borrow().scope.clone() {
            BindingScope::App(app) => app,
            BindingScope::Global => {
                toast_remove.show("The global bindings cannot be removed");
                return;
            }
        };
        let response = ipc::try_request(&ipc::Request::RemoveAppProfile {
            app_id: app.clone()
        });
        if matches!(response, Some(Response::Ok)) {
            {
                let mut guard = store_remove.borrow_mut();
                guard.profiles.remove(&app);
                guard.scope = BindingScope::Global;
            }
            refresh_scope_model(&model_remove, &store_remove.borrow());
            loading_remove.set(true);
            picker.set_selected(0);
            loading_remove.set(false);
            apply_scope(&tiles_remove, &store_remove, loading_remove.as_ref());
        }
        report(response, &toast_remove, "Profile removed");
    });
}

/// Parses one radial menu pixel tuning from an entry field.
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
    group.set_title("Radial menu");
    group.set_description(Some(
        "Hold the gesture button, move past the activation radius and release over a sector"
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
    radius_row.set_subtitle("Cursor travel in pixels that opens the menu");
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
            toast_apply
                .show("Activation radius and dead zone must be whole pixels between 1 and 4000");
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

/// Builds the Gestures page: a master switch, the scope bar and one tile per
/// direction.
///
/// Capture arms a window-level key listener: the next chord becomes the
/// binding the moment a key is released, Escape cancels. Every change is
/// applied and persisted by the daemon immediately.
pub(super) fn gestures_page(toast: &Toasts, window: &ApplicationWindow) -> ScrolledWindow {
    let scroll = page_scroll();
    let box_ = page_body();

    let enable_group = PreferencesGroup::new();
    enable_group.set_title("Gesture recognition");
    enable_group.set_description(Some(
        "Take over the gesture button and fire key combinations on swipes"
    ));
    let enable_row = ActionRow::new();
    enable_row.set_title("Enable gestures");
    let enable_switch = Switch::new();
    enable_switch.set_valign(gtk4::Align::Center);
    enable_row.add_suffix(&enable_switch);
    enable_row.set_activatable_widget(Some(&enable_switch));
    enable_group.add(&enable_row);

    let tiles: GestureTiles = Rc::new(RefCell::new(HashMap::new()));
    let capture: GestureCapture = Rc::new(RefCell::new(None));
    let store: StoreCell = Rc::new(RefCell::new(GestureStore::default()));
    let loading: Rc<Cell<bool>> = Rc::new(Cell::new(false));
    let (scope_group, scope_model) = gesture_scope_bar(toast, &store, &tiles, &loading);
    let (radial_group, radial_widgets) = radial_menu_group(toast, &PieConfig::default());

    let flow = FlowBox::new();
    flow.add_css_class("hud-tiles");
    flow.set_selection_mode(SelectionMode::None);
    flow.set_min_children_per_line(1);
    flow.set_max_children_per_line(1);
    flow.set_row_spacing(12);
    for (direction, label) in GESTURE_TILES {
        let tile = gesture_tile(direction, label, toast, &capture, &tiles, &store, &loading);
        tiles.borrow_mut().insert(direction, tile.clone());
        flow.append(&tile.child);
    }

    box_.append(&enable_group);
    box_.append(&scope_group);
    box_.append(&radial_group);
    box_.append(&flow);
    scroll.set_child(Some(&box_));

    if let Some(Response::Gestures {
        enabled,
        gestures,
        profiles,
        pie
    }) = ipc::try_request(&ipc::Request::GetGestures)
    {
        enable_switch.set_active(enabled);
        {
            let mut guard = store.borrow_mut();
            guard.global = gestures;
            guard.profiles = profiles;
        }
        radial_widgets.enable.set_active(pie.enabled);
        radial_widgets
            .radius
            .set_text(&pie.activation_radius.to_string());
        radial_widgets.dead.set_text(&pie.dead_zone.to_string());
        refresh_scope_model(&scope_model, &store.borrow());
        apply_scope(&tiles, &store, &loading);
    }

    let toast_enable = toast.clone();
    let enable_guard = Rc::new(Cell::new(false));
    let last_enabled = Rc::new(Cell::new(enable_switch.is_active()));
    enable_switch.connect_active_notify(move |switch| {
        if enable_guard.get() {
            return;
        }
        let enabled = switch.is_active();
        let response = ipc::try_request(&ipc::Request::SetGesturesEnabled {
            enabled
        });
        if matches!(response, Some(Response::Ok)) {
            last_enabled.set(enabled);
        } else {
            enable_guard.set(true);
            switch.set_active(last_enabled.get());
            enable_guard.set(false);
        }
        let outcome = if enabled {
            "Gestures enabled"
        } else {
            "Gestures disabled"
        };
        report(response, &toast_enable, outcome);
    });

    attach_gesture_capture(window, &capture, &tiles, &store, toast);
    scroll
}

/// Binds a finished key chord to `direction` through the daemon and
/// reflects it on the tile; shared by the window key capture and the
/// Hyprland keybind picker.
fn commit_gesture_keys(
    direction: GestureDirection,
    tiles: &GestureTiles,
    store: &StoreCell,
    toast: &Toasts,
    names: Vec<String>
) {
    let Some(tile) = tiles.borrow().get(&direction).cloned() else {
        return;
    };
    let moment = tile.current_mode();
    let action = Action::Keypress {
        keys: names.clone()
    };
    let request = store
        .borrow()
        .bind_request(direction, moment, action.clone());
    let response = ipc::try_request(&request);
    if matches!(response, Some(Response::Ok)) {
        store.borrow_mut().record(direction, moment, action);
        tile.set_keys(names);
    }
    report(response, toast, "Gesture bound");
}

/// Installs the window-level capture listener for gesture binding.
///
/// Runs in the capture phase so chords never reach the diagram shortcuts or
/// the Escape-to-close controller while a binding is armed.
fn attach_gesture_capture(
    window: &ApplicationWindow,
    capture: &GestureCapture,
    tiles: &GestureTiles,
    store: &StoreCell,
    toast: &Toasts
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
    let toast_release = toast.clone();
    controller.connect_key_released(move |_controller, _key, _code, _state| {
        let Some((direction, pressed)) = capture_release.borrow().clone() else {
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
        commit_gesture_keys(
            direction,
            &tiles_release,
            &store_release,
            &toast_release,
            names
        );
    });

    window.add_controller(controller);
}

#[cfg(test)]
mod tests {
    use super::parse_pie_px;

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
