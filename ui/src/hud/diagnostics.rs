// SPDX-FileCopyrightText: 2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Diagnostics page: the journal logs of every logi-mx component in one
//! themed view. The daemon and the overlay run as user services, so their
//! `tracing` output lands in the systemd journal; this page queries
//! `journalctl` on a background thread, decodes the JSON records, and
//! paints them into a selectable, copyable text view grouped by date.
//! Filters (component, verbosity, free-text search) apply client-side to
//! the fetched window, and a follow timer refetches while it is on.

use std::{
    cell::{Cell, RefCell},
    process::Command,
    rc::{Rc, Weak},
    sync::{Arc, Mutex},
    thread,
    time::Duration
};

use gtk4::{
    Box as GtkBox, Button, DropDown, Entry, Label, Orientation, ScrolledWindow, Switch,
    TextBuffer, TextTag, TextView, WrapMode, glib
};
use libadwaita::{ActionRow, PreferencesGroup, prelude::*};
use serde_json::Value;

use super::toasts::Toasts;

/// Journal units whose logs the page shows, with their short names.
const UNITS: [(&str, &str); 2] = [
    ("logi-mx-daemon.service", "daemon"),
    ("logi-mx-overlay.service", "overlay")
];
/// Newest journal records a single fetch reads.
const JOURNAL_LINES: &str = "5000";
/// Refetch period while follow mode is on, in milliseconds.
const FOLLOW_TICK_MS: u64 = 3000;

/// One decoded journal record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogEntry {
    /// Wall-clock time in microseconds since the Unix epoch.
    pub timestamp_us: i64,
    /// Short component name, e.g. `daemon`.
    pub unit:         String,
    /// syslog priority: 3 error, 4 warning, 6 info, 7 debug.
    pub priority:     u8,
    /// Log line text.
    pub message:      String
}

/// Decodes one `journalctl -o json` line.
///
/// # Arguments
///
/// * `line` - A single JSON record as emitted by `journalctl -o json`.
///
/// # Returns
///
/// The entry when the line is a JSON object carrying a numeric timestamp,
/// priority and text message; `None` for anything else, including
/// non-UTF-8 messages journalctl encodes as byte arrays.
#[must_use]
pub fn parse_journal_line(line: &str) -> Option<LogEntry> {
    let value: Value = serde_json::from_str(line).ok()?;
    let timestamp_us = value
        .get("__REALTIME_TIMESTAMP")
        .and_then(Value::as_str)?
        .parse()
        .ok()?;
    let priority = value
        .get("PRIORITY")
        .and_then(Value::as_str)?
        .parse::<u8>()
        .ok()?;
    let message = value.get("MESSAGE")?.as_str()?.to_string();
    let identifier = value
        .get("SYSLOG_IDENTIFIER")
        .and_then(Value::as_str)
        .unwrap_or("logi-mx");
    Some(LogEntry {
        timestamp_us,
        unit: unit_label(identifier),
        priority,
        message
    })
}

/// Strips the project prefix from a syslog identifier.
///
/// # Arguments
///
/// * `identifier` - The `SYSLOG_IDENTIFIER` journal field.
///
/// # Returns
///
/// The remainder after `logi-mx-`, or the identifier unchanged when the
/// prefix is absent.
#[must_use]
pub fn unit_label(identifier: &str) -> String {
    identifier
        .strip_prefix("logi-mx-")
        .map_or_else(|| identifier.to_string(), str::to_string)
}

/// Formats a microsecond timestamp as the local `YYYY-MM-DD` date.
///
/// # Arguments
///
/// * `timestamp_us` - Microseconds since the Unix epoch.
///
/// # Returns
///
/// The formatted date, or an empty string for timestamps `GLib` cannot
/// represent.
#[must_use]
pub fn date_of(timestamp_us: i64) -> String {
    let Ok(when) = glib::DateTime::from_unix_local(timestamp_us.div_euclid(1_000_000)) else {
        return String::new();
    };
    when.format("%Y-%m-%d")
        .map_or_else(|_| String::new(), |t| t.to_string())
}

/// Formats a microsecond timestamp as the local `HH:MM:SS` clock.
///
/// # Arguments
///
/// * `timestamp_us` - Microseconds since the Unix epoch.
///
/// # Returns
///
/// The formatted time, or an empty string for unrepresentable timestamps.
#[must_use]
pub fn clock_of(timestamp_us: i64) -> String {
    let Ok(when) = glib::DateTime::from_unix_local(timestamp_us.div_euclid(1_000_000)) else {
        return String::new();
    };
    when.format("%H:%M:%S")
        .map_or_else(|_| String::new(), |t| t.to_string())
}

/// Which fetched entries the view should paint.
#[derive(Debug, Clone)]
pub struct LogFilter {
    /// Show records from the daemon unit.
    pub daemon:   bool,
    /// Show records from the overlay unit.
    pub overlay:  bool,
    /// Paint records at this syslog priority or more urgent.
    pub max_prio: u8,
    /// Case-insensitive substring every painted line must contain.
    pub query:    String
}

impl Default for LogFilter {
    fn default() -> Self {
        Self {
            daemon:   true,
            overlay:  true,
            max_prio: 6,
            query:    String::new()
        }
    }
}

impl LogFilter {
    /// Whether `entry` passes every active filter.
    ///
    /// # Arguments
    ///
    /// * `entry` - A decoded journal record.
    ///
    /// # Returns
    ///
    /// `true` when the unit switch is on, the priority is urgent enough
    /// and the query (lower-cased) occurs in the unit or the message.
    #[must_use]
    pub fn visible(&self, entry: &LogEntry) -> bool {
        let unit_allowed = match entry.unit.as_str() {
            "daemon" => self.daemon,
            "overlay" => self.overlay,
            _ => true
        };
        if !unit_allowed || entry.priority > self.max_prio {
            return false;
        }
        if self.query.is_empty() {
            return true;
        }
        let needle = self.query.to_lowercase();
        entry.message.to_lowercase().contains(&needle)
            || entry.unit.to_lowercase().contains(&needle)
    }
}

/// Reads the logi-mx journal window and decodes every record.
///
/// # Returns
///
/// The decoded entries in chronological order, or the `journalctl`
/// failure to surface in the view.
fn fetch_journal() -> Result<Vec<LogEntry>, String> {
    let mut command = Command::new("journalctl");
    command.args(["--user", "--no-pager", "-o", "json", "-n", JOURNAL_LINES]);
    for (unit, _) in UNITS {
        command.args(["-u", unit]);
    }
    let output = command
        .output()
        .map_err(|e| format!("journalctl unavailable: {e}"))?;
    if !output.status.success() {
        return Err("journalctl reported an error".to_string());
    }
    let text = String::from_utf8_lossy(&output.stdout);
    Ok(text.lines().filter_map(parse_journal_line).collect())
}

thread_local! {
    /// The live diagnostics page, while one is built.
    static DIAG_VIEW: RefCell<Option<Weak<DiagView>>> = const { RefCell::new(None) };
    /// Main-context hook applying journal records fetched by the loader
    /// thread; cleared when the HUD closes.
    static DIAG_HOOK: RefCell<Option<Rc<dyn Fn()>>> = const { RefCell::new(None) };
}

/// Clears the diagnostics hook; called when the HUD window closes.
pub(super) fn clear_diag_hook() {
    DIAG_HOOK.with(|cell| *cell.borrow_mut() = None);
    DIAG_VIEW.with(|cell| *cell.borrow_mut() = None);
}

/// Outcome slot a fetch thread parks for the main-context hook.
type JournalSlot = Arc<Mutex<Option<Result<Vec<LogEntry>, String>>>>;

/// Mutable contents of the view: fetched records and active filters.
#[derive(Default)]
struct DiagState {
    entries: Vec<LogEntry>,
    filter:  LogFilter
}

/// Shared state of one open diagnostics page.
struct DiagView {
    buffer:     TextBuffer,
    view:       TextView,
    reload:     Button,
    state:      RefCell<DiagState>,
    loading:    Cell<bool>,
    slot:       JournalSlot,
    tags:       DiagTags,
    follow_src: RefCell<Option<glib::SourceId>>
}

/// Colour tags of the log view: error, warning, info, debug, date header.
struct DiagTags {
    error: TextTag,
    warn:  TextTag,
    info:  TextTag,
    debug: TextTag,
    date:  TextTag
}

impl DiagView {
    /// Repaints the text view from the fetched entries under the filter,
    /// grouping lines under local date headers and scrolling to the end.
    fn repaint(&self) {
        self.buffer.set_text("");
        let state = self.state.borrow();
        let mut current_date = String::new();
        for entry in state
            .entries
            .iter()
            .filter(|entry| state.filter.visible(entry))
        {
            let date = date_of(entry.timestamp_us);
            if date != current_date {
                current_date.clone_from(&date);
                let mut end = self.buffer.end_iter();
                self.buffer.insert_with_tags(
                    &mut end,
                    &format!("──── {date} ────\n"),
                    &[&self.tags.date]
                );
            }
            let tag = match entry.priority {
                0..=3 => &self.tags.error,
                4 => &self.tags.warn,
                5..=6 => &self.tags.info,
                _ => &self.tags.debug
            };
            let line = format!(
                "{} {:<8} {}\n",
                clock_of(entry.timestamp_us),
                entry.unit,
                entry.message
            );
            let mut end = self.buffer.end_iter();
            self.buffer.insert_with_tags(&mut end, &line, &[tag]);
        }
        let end = self.buffer.end_iter();
        let mark = self.buffer.mark("diagnostics-end").unwrap_or_else(|| {
            self.buffer
                .create_mark(Some("diagnostics-end"), &end, false)
        });
        self.buffer.move_mark(&mark, &end);
        self.view.scroll_mark_onscreen(&mark);
    }
}

/// Registers a foreground-colour tag on the buffer's tag table.
///
/// # Arguments
///
/// * `buffer` - The log text buffer owning the tag table.
/// * `name` - Tag name used when painting lines.
/// * `colour` - CSS colour string for the tag's foreground.
fn register_tag(buffer: &TextBuffer, name: &str, colour: &str) -> TextTag {
    let tag = TextTag::new(Some(name));
    tag.set_foreground(Some(colour));
    buffer.tag_table().add(&tag);
    tag
}

/// Marks the page busy and starts a background journal fetch; the reload
/// button stays insensitive until the hook applies the outcome.
///
/// # Arguments
///
/// * `diag` - The live diagnostics page.
fn start_fetch(diag: &Rc<DiagView>) {
    if diag.loading.replace(true) {
        return;
    }
    diag.reload.set_sensitive(false);
    let slot_thread = Arc::clone(&diag.slot);
    let context = glib::MainContext::default();
    let spawned = thread::Builder::new()
        .name("journal-loader".to_string())
        .spawn(move || {
            let outcome = fetch_journal();
            if let Ok(mut guard) = slot_thread.lock() {
                *guard = Some(outcome);
            }
            context.invoke(|| {
                DIAG_HOOK.with(|cell| {
                    if let Some(hook) = cell.borrow().as_ref() {
                        hook();
                    }
                });
            });
        });
    if let Err(e) = spawned {
        diag.loading.set(false);
        diag.reload.set_sensitive(true);
        diag.buffer
            .set_text(&format!("Failed to spawn the journal loader: {e}"));
    }
}

/// Widgets of the filter group, kept so the handlers can read them.
struct FilterWidgets {
    group:   PreferencesGroup,
    daemon:  Switch,
    overlay: Switch,
    prio:    DropDown,
    search:  Entry
}

/// Builds the Filters group: unit switches, verbosity and search field.
fn build_filter_group() -> FilterWidgets {
    let group = PreferencesGroup::new();
    group.set_title("Filters");

    let daemon_row = ActionRow::new();
    daemon_row.set_title("Daemon log");
    let daemon = Switch::new();
    daemon.set_valign(gtk4::Align::Center);
    daemon.set_active(true);
    daemon_row.add_suffix(&daemon);
    daemon_row.set_activatable_widget(Some(&daemon));

    let overlay_row = ActionRow::new();
    overlay_row.set_title("Overlay log");
    let overlay = Switch::new();
    overlay.set_valign(gtk4::Align::Center);
    overlay.set_active(true);
    overlay_row.add_suffix(&overlay);
    overlay_row.set_activatable_widget(Some(&overlay));

    let prio_row = ActionRow::new();
    prio_row.set_title("Verbosity");
    let prio =
        DropDown::from_strings(&["Errors", "Warnings and errors", "Info and above", "Debug"]);
    prio.set_selected(2);
    prio.set_valign(gtk4::Align::Center);
    prio_row.add_suffix(&prio);

    let search_row = ActionRow::new();
    search_row.set_title("Search");
    let search = Entry::new();
    search.set_placeholder_text(Some("filter lines"));
    search.set_valign(gtk4::Align::Center);
    search_row.add_suffix(&search);

    group.add(&daemon_row);
    group.add(&overlay_row);
    group.add(&prio_row);
    group.add(&search_row);
    FilterWidgets {
        group,
        daemon,
        overlay,
        prio,
        search
    }
}

/// Connects the filter widgets to the page state: every change recomputes
/// the filter and repaints from the already fetched records.
///
/// # Arguments
///
/// * `diag` - The live diagnostics page.
/// * `widgets` - The filter widgets to observe.
fn wire_filters(diag: &Rc<DiagView>, widgets: &FilterWidgets) {
    let apply = {
        let diag = Rc::clone(diag);
        let daemon = widgets.daemon.clone();
        let overlay = widgets.overlay.clone();
        let prio = widgets.prio.clone();
        let search = widgets.search.clone();
        Rc::new(move || {
            {
                let mut state = diag.state.borrow_mut();
                state.filter.daemon = daemon.is_active();
                state.filter.overlay = overlay.is_active();
                state.filter.max_prio = match prio.selected() {
                    0 => 3,
                    1 => 4,
                    3 => 7,
                    _ => 6
                };
                state.filter.query = search.text().trim().to_string();
            }
            diag.repaint();
        })
    };
    let apply_daemon = Rc::clone(&apply);
    widgets.daemon.connect_state_set(move |_, _| {
        let apply = Rc::clone(&apply_daemon);
        glib::idle_add_local_once(move || apply());
        glib::Propagation::Proceed
    });
    let apply_overlay = Rc::clone(&apply);
    widgets.overlay.connect_state_set(move |_, _| {
        let apply = Rc::clone(&apply_overlay);
        glib::idle_add_local_once(move || apply());
        glib::Propagation::Proceed
    });
    let apply_prio = Rc::clone(&apply);
    widgets.prio.connect_selected_notify(move |_| apply_prio());
    let diag_search = Rc::clone(diag);
    let search = widgets.search.clone();
    widgets.search.connect_changed(move |_| {
        diag_search.state.borrow_mut().filter.query = search.text().trim().to_string();
        diag_search.repaint();
    });
}

/// Builds the themed log text view and its colour tags.
fn build_log_view() -> (TextView, TextBuffer, DiagTags) {
    let view = TextView::new();
    view.set_editable(false);
    view.set_cursor_visible(true);
    view.set_wrap_mode(WrapMode::Char);
    view.set_monospace(true);
    view.add_css_class("diag-log");
    view.set_vexpand(true);
    view.set_size_request(-1, 420);
    let buffer = view.buffer();
    let tags = DiagTags {
        error: register_tag(&buffer, "diag-err", "#ff6b6b"),
        warn:  register_tag(&buffer, "diag-warn", "#e3b341"),
        info:  register_tag(&buffer, "diag-info", "#c9d1d9"),
        debug: register_tag(&buffer, "diag-debug", "#8b949e"),
        date:  register_tag(&buffer, "diag-date", "#58a6ff")
    };
    (view, buffer, tags)
}

/// Connects the follow switch: while on, a timer refetches every few
/// seconds until the page is dropped; turning it off removes the timer.
///
/// # Arguments
///
/// * `diag` - The live diagnostics page.
/// * `follow` - The switch toggling follow mode.
fn wire_follow(diag: &Rc<DiagView>, follow: &Switch) {
    let diag_follow = Rc::clone(diag);
    let weak_follow = Rc::downgrade(diag);
    follow.connect_state_set(move |_, active| {
        if active {
            start_fetch(&diag_follow);
            let weak = weak_follow.clone();
            let source =
                glib::timeout_add_local(Duration::from_millis(FOLLOW_TICK_MS), move || {
                    let Some(diag) = weak.upgrade() else {
                        return glib::ControlFlow::Break;
                    };
                    if !diag.loading.get() {
                        start_fetch(&diag);
                    }
                    glib::ControlFlow::Continue
                });
            *diag_follow.follow_src.borrow_mut() = Some(source);
        } else if let Some(source) = diag_follow.follow_src.borrow_mut().take() {
            source.remove();
        }
        glib::Propagation::Proceed
    });
}

/// Builds the Diagnostics page: component and verbosity filters, a search
/// field, the themed journal view and the reload / follow controls.
///
/// # Arguments
///
/// * `toast` - Toast stack for load failures.
pub(super) fn diagnostics_page(toast: &Toasts) -> ScrolledWindow {
    let scroll = ScrolledWindow::builder()
        .vexpand(true)
        .hexpand(true)
        .build();
    scroll.add_css_class("hud-page");
    let body = GtkBox::new(Orientation::Vertical, 10);
    body.set_margin_top(4);

    let intro = PreferencesGroup::new();
    intro.set_title("Diagnostics");
    intro.set_description(Some(
        "Journal logs of the daemon and the overlay. Select text and press Ctrl+C to copy."
    ));
    let filters = build_filter_group();

    let (view, buffer, tags) = build_log_view();

    let reload = Button::with_label("Reload");
    let diag = Rc::new(DiagView {
        buffer: buffer.clone(),
        view: view.clone(),
        reload: reload.clone(),
        state: RefCell::new(DiagState::default()),
        loading: Cell::new(false),
        slot: Arc::new(Mutex::new(None)),
        tags,
        follow_src: RefCell::new(None)
    });
    DIAG_VIEW.with(|cell| *cell.borrow_mut() = Some(Rc::downgrade(&diag)));

    let hook_diag = Rc::downgrade(&diag);
    let toast_hook = toast.clone();
    let hook: Rc<dyn Fn()> = Rc::new(move || {
        let Some(diag) = hook_diag.upgrade() else {
            return;
        };
        let Some(outcome) = diag.slot.lock().ok().and_then(|mut guard| guard.take()) else {
            return;
        };
        diag.loading.set(false);
        diag.reload.set_sensitive(true);
        match outcome {
            Ok(entries) => {
                diag.state.borrow_mut().entries = entries;
                diag.repaint();
            }
            Err(message) => {
                diag.buffer.set_text(&message);
                toast_hook.show("Journal unavailable");
            }
        }
    });
    DIAG_HOOK.with(|cell| *cell.borrow_mut() = Some(hook));
    wire_filters(&diag, &filters);

    let log_scroll = ScrolledWindow::builder().child(&view).vexpand(true).build();
    log_scroll.add_css_class("hud-page");

    let controls = GtkBox::new(Orientation::Horizontal, 8);
    let copy = Button::with_label("Copy all");
    let follow_label = Label::new(Some("Follow"));
    let follow = Switch::new();
    follow.set_valign(gtk4::Align::Center);
    controls.append(&reload);
    controls.append(&copy);
    controls.append(&follow_label);
    controls.append(&follow);
    controls.set_halign(gtk4::Align::End);

    let diag_reload = Rc::clone(&diag);
    reload.connect_clicked(move |_| start_fetch(&diag_reload));

    let diag_copy = Rc::clone(&diag);
    copy.connect_clicked(move |button| {
        let text = diag_copy.buffer.text(
            &diag_copy.buffer.start_iter(),
            &diag_copy.buffer.end_iter(),
            false
        );
        button.clipboard().set_text(&text);
    });

    wire_follow(&diag, &follow);

    body.append(&intro);
    body.append(&filters.group);
    body.append(&log_scroll);
    body.append(&controls);
    scroll.set_child(Some(&body));

    buffer.set_text("Loading journal…");
    start_fetch(&diag);
    scroll
}

#[cfg(test)]
#[path = "diagnostics_tests.rs"]
mod tests;
