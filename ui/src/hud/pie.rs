// SPDX-FileCopyrightText: 2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Pie page: a live preview of the command ring plus its tuning.
//!
//! The ring shows exactly as many equal wedges as there are installed
//! slots, slot 0 at twelve o'clock. Clicking a wedge selects its slot and
//! the slot editor binds a shell command or a picked Hyprland keybind,
//! clears the slot, adds or removes wedges. The scope bar switches between
//! the global ring and a per-application profile, so a game can carry its
//! own wheel while the desktop keeps the default one.

mod preview;
mod slots;
mod store;
mod tuning;

use std::{
    cell::{Cell, RefCell},
    rc::Rc
};

use gtk4::{Button, DrawingArea, DropDown, GestureClick};
use libadwaita::{ActionRow, PreferencesGroup, prelude::*};
use logi_mx_daemon::ipc::{self, Response};
use preview::{WHEEL_SIZE, paint_ring, slot_at};
use slots::{SlotContext, SlotEditor, details_label, slot_summary, wire_slot_editor};
use store::{PieScope, PieStore};
use tuning::radial_menu_group;

use super::{page_body, page_scroll, report, toasts::Toasts};

/// Shared handle of the page store.
type StoreCell = Rc<RefCell<PieStore>>;

/// Everything a scope callback needs to switch rings without racing the
/// dropdown's own change notifications.
struct ScopeHandles {
    toast:    Toasts,
    store:    StoreCell,
    selected: Rc<Cell<Option<usize>>>,
    model:    gtk4::StringList,
    picker:   DropDown,
    loading:  Rc<Cell<bool>>,
    refresh:  Rc<dyn Fn()>
}

/// Builds the interactive wheel: equal wedges painted from the store,
/// redrawn whenever the caller queues a repaint.
fn preview_area(store: &StoreCell, selected: &Rc<Cell<Option<usize>>>) -> DrawingArea {
    let area = DrawingArea::new();
    area.set_size_request(WHEEL_SIZE, WHEEL_SIZE);
    area.set_halign(gtk4::Align::Center);
    area.set_margin_top(8);
    area.set_margin_bottom(8);
    let paint_store = Rc::clone(store);
    let paint_selected = Rc::clone(selected);
    area.set_draw_func(move |_area, cr, width, height| {
        paint_ring(
            cr,
            f64::from(width),
            f64::from(height),
            &paint_store.borrow().visible_ring(),
            paint_selected.get(),
            "No slots yet"
        );
    });
    area
}

/// Builds the Pie page: intro, scope bar, interactive wheel preview, slot
/// editor and the radial menu tuning group.
pub(super) fn pie_page(toast: &Toasts) -> gtk4::ScrolledWindow {
    let scroll = page_scroll();
    let body = page_body();

    let intro = PreferencesGroup::new();
    intro.set_title("Radial menu");
    intro.set_description(Some(
        "Hold the gesture button: a thin ring charges around the cursor, then the menu blooms \
         in with one equal wedge per installed slot, the first at twelve o'clock. Point at a \
         wedge or swipe toward it, release to fire it; a swipe that commits before the ring \
         opens is an expert mark and fires the swipe binding instead. A quick tap keeps the \
         click binding, a double click opens the menu at once."
    ));

    let store: StoreCell = Rc::new(RefCell::new(PieStore::default()));
    let selected: Rc<Cell<Option<usize>>> = Rc::new(Cell::new(None));

    let area = preview_area(&store, &selected);
    let details = details_label();
    let editor = SlotEditor::new();

    let model = gtk4::StringList::new(&["Global"]);
    let picker = DropDown::new(Some(model.clone()), None::<gtk4::Expression>);
    picker.set_valign(gtk4::Align::Center);
    let loading = Rc::new(Cell::new(false));

    let refresh: Rc<dyn Fn()> = {
        let store = Rc::clone(&store);
        let selected = Rc::clone(&selected);
        let details = details.clone();
        let editor = editor.clone();
        let area = area.clone();
        let model = model.clone();
        let picker = picker.clone();
        let loading = Rc::clone(&loading);
        Rc::new(move || {
            let guard = store.borrow();
            let apps = guard.scope_apps();
            let extra = model.n_items().saturating_sub(1);
            let refs: Vec<&str> = apps.iter().map(String::as_str).collect();
            model.splice(1, extra, &refs);
            let expected = match &guard.scope {
                PieScope::Global => 0,
                PieScope::App(app) => {
                    u32::try_from(apps.iter().position(|a| a == app).map_or(0, |p| p + 1))
                        .unwrap_or(0)
                }
            };
            if picker.selected() != expected {
                loading.set(true);
                picker.set_selected(expected);
                loading.set(false);
            }
            let ring = guard.visible_ring();
            drop(guard);
            details.set_text(&slot_summary(&ring, selected.get()));
            editor.inspect(&ring, selected.get());
            area.queue_draw();
        })
    };

    wire_slot_editor(&SlotContext {
        toast:    toast.clone(),
        store:    Rc::clone(&store),
        selected: Rc::clone(&selected),
        repaint:  {
            let area = area.clone();
            Rc::new(move || area.queue_draw()) as Rc<dyn Fn()>
        },
        details:  Rc::clone(&refresh),
        editor:   editor.clone()
    });

    let click = GestureClick::new();
    let click_store = Rc::clone(&store);
    let click_selected = Rc::clone(&selected);
    let click_refresh = Rc::clone(&refresh);
    click.connect_pressed(move |_gesture, _n_press, x, y| {
        let count = click_store.borrow().visible_ring().len();
        click_selected.set(slot_at(x, y, f64::from(WHEEL_SIZE), count));
        click_refresh();
    });
    area.add_controller(click);

    let scope_group = scope_bar(
        toast, &store, &selected, &model, &picker, &loading, &refresh
    );

    let (tuning, widgets) = radial_menu_group(toast, &store.borrow().config.clone());

    body.append(&intro);
    body.append(&scope_group);
    body.append(&area);
    body.append(&details);
    body.append(&editor.group);
    body.append(&tuning);
    scroll.set_child(Some(&body));

    let reload_store = Rc::clone(&store);
    let reload_refresh = Rc::clone(&refresh);
    scroll.connect_map(move |_| {
        reload_pie(&reload_store, &widgets, &reload_refresh);
    });
    scroll
}

/// Builds the ring scope bar: the editing dropdown, the focused-app button
/// and the profile removal button.
#[allow(clippy::too_many_arguments, reason = "one bundle of scope widgets")]
fn scope_bar(
    toast: &Toasts,
    store: &StoreCell,
    selected: &Rc<Cell<Option<usize>>>,
    model: &gtk4::StringList,
    picker: &DropDown,
    loading: &Rc<Cell<bool>>,
    refresh: &Rc<dyn Fn()>
) -> PreferencesGroup {
    let group = PreferencesGroup::new();
    group.set_title("Ring scope");
    group.set_description(Some(
        "Edit the global ring or a per-application override keyed by the window class"
    ));

    let pick_row = ActionRow::new();
    pick_row.set_title("Editing");
    pick_row.add_suffix(picker);

    let focused_row = ActionRow::new();
    focused_row.set_title("Use focused app");
    focused_row.set_subtitle("Switch to the focused window class as the ring scope");
    let focused_btn = Button::with_label("Focused");
    focused_btn.set_valign(gtk4::Align::Center);
    focused_row.add_suffix(&focused_btn);
    focused_row.set_activatable_widget(Some(&focused_btn));

    let remove_row = ActionRow::new();
    remove_row.set_title("Remove app ring");
    remove_row.set_subtitle("Deletes the selected profile and returns to the global ring");
    let remove_btn = Button::with_label("Remove");
    remove_btn.add_css_class("destructive-action");
    remove_btn.set_valign(gtk4::Align::Center);
    remove_row.add_suffix(&remove_btn);
    remove_row.set_activatable_widget(Some(&remove_btn));

    group.add(&pick_row);
    group.add(&focused_row);
    group.add(&remove_row);

    let handles = ScopeHandles {
        toast:    toast.clone(),
        store:    Rc::clone(store),
        selected: Rc::clone(selected),
        model:    model.clone(),
        picker:   picker.clone(),
        loading:  Rc::clone(loading),
        refresh:  Rc::clone(refresh)
    };
    wire_scope_picker(&handles);
    wire_scope_buttons(handles, &focused_btn, &remove_btn);
    group
}

/// Switches the store scope whenever the user moves the dropdown.
fn wire_scope_picker(handles: &ScopeHandles) {
    let ScopeHandles {
        store,
        selected,
        model,
        picker,
        loading,
        refresh,
        ..
    } = handles;
    let store = Rc::clone(store);
    let selected = Rc::clone(selected);
    let model = model.clone();
    let picker = picker.clone();
    let loading = Rc::clone(loading);
    let refresh = Rc::clone(refresh);
    picker.connect_selected_notify(move |pick| {
        if loading.get() {
            return;
        }
        let index = pick.selected();
        let scope = if index == 0 {
            PieScope::Global
        } else {
            model
                .string(index)
                .map_or(PieScope::Global, |app| PieScope::App(app.to_string()))
        };
        store.borrow_mut().scope = scope;
        selected.set(None);
        refresh();
    });
}

/// Wires the "Focused" and "Remove" controls of the scope bar.
fn wire_scope_buttons(handles: ScopeHandles, focused_btn: &Button, remove_btn: &Button) {
    let ScopeHandles {
        toast,
        store,
        selected,
        model,
        picker,
        loading,
        refresh
    } = handles;

    let store_focus = Rc::clone(&store);
    let selected_focus = Rc::clone(&selected);
    let model_focus = model.clone();
    let picker_focus = picker.clone();
    let loading_focus = Rc::clone(&loading);
    let refresh_focus = Rc::clone(&refresh);
    let toast_focus = toast.clone();
    focused_btn.connect_clicked(
        move |_| match ipc::try_request(&ipc::Request::GetFocusedApp) {
            Some(Response::FocusedApp {
                app_id: Some(app)
            }) => {
                store_focus.borrow_mut().scope = PieScope::App(app.clone());
                selected_focus.set(None);
                let apps = store_focus.borrow().scope_apps();
                let extra = model_focus.n_items().saturating_sub(1);
                let refs: Vec<&str> = apps.iter().map(String::as_str).collect();
                model_focus.splice(1, extra, &refs);
                let index = apps.iter().position(|a| *a == app).map_or(1, |p| p + 1);
                loading_focus.set(true);
                picker_focus.set_selected(u32::try_from(index).unwrap_or(1));
                loading_focus.set(false);
                refresh_focus();
            }
            _ => toast_focus.show("The desktop reported no focused window")
        }
    );

    let toast_remove = toast;
    remove_btn.connect_clicked(move |_| {
        let app = match store.borrow().scope.clone() {
            PieScope::App(app) => app,
            PieScope::Global => {
                toast_remove.show("The global ring cannot be removed");
                return;
            }
        };
        let response = ipc::try_request(&ipc::Request::RemoveAppPieProfile {
            app_id: app.clone()
        });
        if matches!(response, Some(Response::Ok)) {
            store.borrow_mut().profiles.remove(&app);
            store.borrow_mut().scope = PieScope::Global;
            selected.set(None);
            let apps = store.borrow().scope_apps();
            let extra = model.n_items().saturating_sub(1);
            let refs: Vec<&str> = apps.iter().map(String::as_str).collect();
            model.splice(1, extra, &refs);
            loading.set(true);
            picker.set_selected(0);
            loading.set(false);
            refresh();
        }
        report(response, &toast_remove, "App ring removed");
    });
}

/// Reloads the rings and the tuning fields from the daemon.
///
/// Runs when the page is mapped rather than at build time: the HUD opens on
/// another tab, so the page skips the round-trip until the user looks at it,
/// and every later visit shows edits made by the CLI or another window.
fn reload_pie(store: &StoreCell, widgets: &tuning::PieWidgets, refresh: &Rc<dyn Fn()>) {
    let Some(Response::PieSettings {
        config,
        profiles
    }) = ipc::try_request(&ipc::Request::GetPie)
    else {
        return;
    };
    widgets.enable.set_active(config.enabled);
    widgets
        .radius
        .set_text(&config.activation_radius.to_string());
    widgets.dead.set_text(&config.dead_zone.to_string());
    {
        let mut guard = store.borrow_mut();
        guard.config = config;
        guard.profiles = profiles;
    }
    refresh();
}
