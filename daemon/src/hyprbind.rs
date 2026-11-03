// SPDX-FileCopyrightText: 2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Hyprland keyboard bindings reader for the keybind picker.
//!
//! Hyprland answers the plain-text `j/binds` request on its request socket
//! (the same instance directory the focus watcher follows) with the active
//! bindings as a JSON array. This module turns every replayable
//! keyboard binding into a `KEY_*` chord the gesture and button runtimes can
//! inject, so a mouse gesture can follow a keybind the user already defined
//! instead of capturing a new chord by hand.
//!
//! Bindings that cannot be replayed faithfully are skipped: mouse controls,
//! submap switches, chords carrying the CAPS Lock bit, and chords carrying
//! modifier bits the mapping does not cover (`NumLock`, Mod3, Mod5, high
//! bits).

use std::{
    io::{Read, Write},
    os::unix::net::UnixStream,
    time::Duration
};

use masterror::AppError;
use serde::{Deserialize, Serialize};

use crate::focus;

/// Result alias for Hyprland binding operations failing with [`AppError`].
type Result<T> = std::result::Result<T, AppError>;

/// Read and write timeout for the request socket exchange.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(1);

/// Plain-text request asking Hyprland for its bindings as JSON, the same
/// payload `hyprctl binds -j` sends: the `j/` prefix selects JSON output.
const BINDINGS_REQUEST: &[u8] = b"j/binds";

/// Maximum number of dispatcher argument characters shown in a label.
const ARG_DISPLAY_LIMIT: usize = 40;

/// XKB modifier bits Hyprland reports, mapped onto evdev modifier keys.
/// Bit 0 is Shift, 2 Control, 3 Mod1 (Alt) and 6 Mod4 (Super); any other
/// bit (CAPS Lock, `NumLock`, Mod3, Mod5) has no faithful injector key.
const MODIFIER_KEYS: [(u32, &str); 4] = [
    (1, "KEY_LEFTSHIFT"),
    (4, "KEY_LEFTCTRL"),
    (8, "KEY_LEFTALT"),
    (64, "KEY_LEFTMETA")
];

/// Human names of the same bits, in label order.
const MODIFIER_NAMES: [(u32, &str); 4] = [(1, "SHIFT"), (4, "CTRL"), (8, "ALT"), (64, "SUPER")];

/// One Hyprland keyboard binding converted into a replayable key chord.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeybindEntry {
    /// `KEY_*` chord ready for `Action::Keypress`: modifiers first, then the
    /// binding's own keys in report order.
    pub keys:  Vec<String>,
    /// Human description, e.g. `SUPER + D — exec: rofi`.
    pub label: String
}

/// One raw element of the `j/binds` JSON array.
///
/// The shape matches Hyprland 0.57: one `key` per binding, a `mouse` flag
/// for pointer bindings, and a `description` that Lua configurations fill in
/// while legacy dispatchers leave empty.
#[derive(Debug, Deserialize)]
struct RawBinding {
    #[serde(default)]
    key:         String,
    #[serde(default)]
    modmask:     u32,
    #[serde(default)]
    dispatcher:  String,
    #[serde(default)]
    arg:         String,
    #[serde(default)]
    submap:      String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    mouse:       bool,
    #[serde(default)]
    catch_all:   bool
}

/// Parses the `j/binds` JSON array into replayable entries.
///
/// Malformed entries and anything that cannot be replayed as a keyboard
/// chord (mouse bindings, submap switches, CAPS or unmapped modifier bits,
/// unknown key names) is skipped; invalid JSON yields an empty list.
///
/// # Arguments
///
/// * `json` - Raw JSON text received from the Hyprland request socket.
///
/// # Returns
///
/// One [`KeybindEntry`] per replayable binding, in report order.
///
/// # Examples
///
/// ```
/// use logi_mx_daemon::hyprbind::parse_bindings;
///
/// let json = r#"[{"key":"D","modmask":64,"dispatcher":"exec","arg":"rofi"}]"#;
/// let entries = parse_bindings(json);
/// assert_eq!(entries.len(), 1);
/// assert_eq!(entries[0].keys, vec!["KEY_LEFTMETA", "KEY_D"]);
/// ```
#[must_use]
pub fn parse_bindings(json: &str) -> Vec<KeybindEntry> {
    let Ok(raw) = serde_json::from_str::<Vec<RawBinding>>(json) else {
        return Vec::new();
    };
    raw.iter().filter_map(entry_from).collect()
}

/// Maps a Hyprland modmask onto evdev modifier key names, modifiers in
/// shift, control, alt, super order.
///
/// # Arguments
///
/// * `modmask` - XKB modifier mask reported by Hyprland.
///
/// # Returns
///
/// The modifier `KEY_*` names, possibly empty for an unmodified binding, or
/// `None` when the mask carries the CAPS bit or any unmapped bit, because
/// such a chord cannot be replayed faithfully through uinput.
///
/// # Examples
///
/// ```
/// use logi_mx_daemon::hyprbind::modmask_to_keys;
///
/// assert_eq!(modmask_to_keys(64), Some(vec!["KEY_LEFTMETA".to_string()]));
/// assert_eq!(modmask_to_keys(2), None);
/// ```
#[must_use]
pub fn modmask_to_keys(modmask: u32) -> Option<Vec<String>> {
    let mapped: u32 = MODIFIER_KEYS.iter().fold(0, |acc, (bit, _)| acc | bit);
    if modmask & !mapped != 0 {
        return None;
    }
    Some(
        MODIFIER_KEYS
            .iter()
            .filter(|(bit, _)| modmask & bit != 0)
            .map(|(_, key)| (*key).to_string())
            .collect()
    )
}

/// Converts one Hyprland key name into a `KEY_*` evdev name.
///
/// Accepts single letters and digits (`D` → `KEY_D`, `1` → `KEY_1`) and the
/// named keys Hyprland configs commonly bind: function keys, navigation and
/// punctuation keysyms. Matching is case-insensitive.
///
/// # Arguments
///
/// * `name` - Key name as reported in the `keys` array of a binding.
///
/// # Returns
///
/// The evdev `KEY_*` name, or `None` for mouse buttons and unknown names.
///
/// # Examples
///
/// ```
/// use logi_mx_daemon::hyprbind::hypr_key_to_keycode;
///
/// assert_eq!(hypr_key_to_keycode("d"), Some("KEY_D".to_string()));
/// assert_eq!(hypr_key_to_keycode("mouse:276"), None);
/// ```
#[must_use]
pub fn hypr_key_to_keycode(name: &str) -> Option<String> {
    let trimmed = name.trim();
    let mut chars = trimmed.chars();
    if let (Some(ch), None) = (chars.next(), chars.next()) {
        if ch.is_ascii_alphabetic() {
            return Some(format!("KEY_{}", ch.to_ascii_uppercase()));
        }
        if ch.is_ascii_digit() {
            return Some(format!("KEY_{ch}"));
        }
    }
    let key = match trimmed.to_lowercase().as_str() {
        "f1" => "KEY_F1",
        "f2" => "KEY_F2",
        "f3" => "KEY_F3",
        "f4" => "KEY_F4",
        "f5" => "KEY_F5",
        "f6" => "KEY_F6",
        "f7" => "KEY_F7",
        "f8" => "KEY_F8",
        "f9" => "KEY_F9",
        "f10" => "KEY_F10",
        "f11" => "KEY_F11",
        "f12" => "KEY_F12",
        "return" => "KEY_ENTER",
        "tab" => "KEY_TAB",
        "space" => "KEY_SPACE",
        "backspace" => "KEY_BACKSPACE",
        "delete" => "KEY_DELETE",
        "escape" => "KEY_ESC",
        "home" => "KEY_HOME",
        "end" => "KEY_END",
        "page_up" => "KEY_PAGEUP",
        "page_down" => "KEY_PAGEDOWN",
        "left" => "KEY_LEFT",
        "right" => "KEY_RIGHT",
        "up" => "KEY_UP",
        "down" => "KEY_DOWN",
        "comma" => "KEY_COMMA",
        "period" => "KEY_DOT",
        "slash" => "KEY_SLASH",
        "semicolon" => "KEY_SEMICOLON",
        "apostrophe" => "KEY_APOSTROPHE",
        "grave" => "KEY_GRAVE",
        "minus" => "KEY_MINUS",
        "equal" => "KEY_EQUAL",
        "bracketleft" => "KEY_LEFTBRACE",
        "bracketright" => "KEY_RIGHTBRACE",
        "backslash" => "KEY_BACKSLASH",
        _ => return None
    };
    Some(key.to_string())
}

/// Fetches the active Hyprland keyboard bindings through the request socket.
///
/// Discovers the live instance directory the same way the focus watcher
/// does, sends `j/binds` and parses the reply.
///
/// # Returns
///
/// The replayable bindings, in Hyprland report order.
///
/// # Errors
///
/// Returns [`AppError`] when no Hyprland instance directory is found, the
/// request socket is unreachable, or the compositor does not answer within
/// the timeout.
pub fn fetch_keybinds() -> Result<Vec<KeybindEntry>> {
    let dir = focus::instance_dir()
        .ok_or_else(|| AppError::not_found("No running Hyprland instance was found"))?;
    let socket = dir.join(".socket.sock");
    let mut stream = UnixStream::connect(&socket).map_err(|e| {
        AppError::service(format!(
            "Failed to connect to the Hyprland request socket at {}",
            socket.display()
        ))
        .with_source(e)
    })?;
    stream
        .set_read_timeout(Some(REQUEST_TIMEOUT))
        .and_then(|()| stream.set_write_timeout(Some(REQUEST_TIMEOUT)))
        .map_err(|e| {
            AppError::service("Failed to configure the Hyprland request socket").with_source(e)
        })?;
    stream
        .write_all(BINDINGS_REQUEST)
        .and_then(|()| stream.flush())
        .map_err(|e| {
            AppError::service("Failed to query Hyprland for its bindings").with_source(e)
        })?;
    let mut reply = String::new();
    if let Err(e) = stream.read_to_string(&mut reply) {
        if matches!(
            e.kind(),
            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
        ) {
            return Err(
                AppError::timeout("Hyprland did not answer the bindings query in time")
                    .with_source(e)
            );
        }
        return Err(
            AppError::service("Failed to read the Hyprland bindings reply").with_source(e)
        );
    }
    Ok(parse_bindings(&reply))
}

/// Converts one raw binding into an entry, rejecting the replayable-impossible
/// shapes: empty or catch-all keys, mouse bindings, submap switches, mouse
/// dispatchers, unmappable modifiers and unknown key names.
fn entry_from(raw: &RawBinding) -> Option<KeybindEntry> {
    if raw.key.trim().is_empty()
        || raw.catch_all
        || raw.mouse
        || !raw.submap.is_empty()
        || raw.dispatcher.starts_with("mouse")
        || raw.dispatcher == "submap"
    {
        return None;
    }
    let mut keys = modmask_to_keys(raw.modmask)?;
    keys.push(hypr_key_to_keycode(&raw.key)?);
    Some(KeybindEntry {
        keys,
        label: label_for(raw)
    })
}

/// Builds the human label: the chord, then the most meaningful detail the
/// binding carries.
fn label_for(raw: &RawBinding) -> String {
    let mut chord: Vec<&str> = MODIFIER_NAMES
        .iter()
        .filter(|(bit, _)| raw.modmask & bit != 0)
        .map(|(_, name)| *name)
        .collect();
    chord.push(raw.key.trim());
    let chord = chord.join(" + ");
    match detail_for(raw) {
        Some(detail) => format!("{chord} — {detail}"),
        None => chord
    }
}

/// The trailing detail for a binding label: the configured description when
/// present, else the dispatcher and a truncated argument. Lua configurations
/// report the dispatcher as an opaque function address, so there only the
/// argument can carry meaning.
fn detail_for(raw: &RawBinding) -> Option<String> {
    let description = raw.description.trim();
    if !description.is_empty() {
        return Some(description.to_string());
    }
    let arg = raw.arg.trim();
    if raw.dispatcher.starts_with("function:") {
        return (!arg.is_empty()).then(|| truncate(arg));
    }
    if raw.dispatcher.is_empty() {
        return None;
    }
    if arg.is_empty() {
        Some(raw.dispatcher.clone())
    } else {
        Some(format!("{}: {}", raw.dispatcher, truncate(arg)))
    }
}

/// Shortens text to [`ARG_DISPLAY_LIMIT`] characters, marking the cut.
fn truncate(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= ARG_DISPLAY_LIMIT {
        return text.to_string();
    }
    let mut head: String = chars[..ARG_DISPLAY_LIMIT].iter().collect();
    head.push('…');
    head
}

#[cfg(test)]
mod tests {
    use logi_mx_driver::keys::key_code;

    use super::*;

    const FIXTURE: &str = r#"[
        {"key":"D","modmask":64,"dispatcher":"exec","arg":"rofi","submap":"","description":"","mouse":false},
        {"key":"T","modmask":5,"dispatcher":"exec","arg":"firefox --new-tab https://example.com/a/path/long/enough/to/be/truncated/by/the/label","submap":"","description":"","mouse":false},
        {"key":"mouse:276","modmask":64,"dispatcher":"mouselook","arg":"lock","submap":"","description":"","mouse":true},
        {"key":"M","modmask":64,"dispatcher":"submap","arg":"gaming","submap":"","description":"","mouse":false},
        {"key":"G","modmask":64,"dispatcher":"exec","arg":"games","submap":"gaming","description":"","mouse":false},
        {"key":"C","modmask":2,"dispatcher":"exec","arg":"caps","submap":"","description":"","mouse":false},
        {"key":"mouse:277","modmask":8,"dispatcher":"exec","arg":"x","submap":"","description":"","mouse":true},
        {"key":"NOSUCHKEY","modmask":64,"dispatcher":"exec","arg":"x","submap":"","description":"","mouse":false},
        {"key":"F1","modmask":0,"dispatcher":"exec","arg":"tty1","submap":"","description":"Virtual console 1","mouse":false},
        {"key":"T","modmask":64,"dispatcher":"function: 0x558ce96d6e10","arg":"6","submap":"","description":"[Launcher|Apps] terminal emulator","mouse":false},
        {"key":"","modmask":64,"dispatcher":"exec","arg":"x","submap":"","description":"","mouse":false,"catch_all":true}
    ]"#;

    #[test]
    fn parse_bindings_keeps_replayable_entries_and_skips_the_rest() {
        let entries = parse_bindings(FIXTURE);
        assert_eq!(entries.len(), 4);
        assert_eq!(entries[0].label, "SUPER + D — exec: rofi");
        assert!(
            entries[1]
                .label
                .starts_with("SHIFT + CTRL + T — exec: firefox --new-tab ")
        );
        assert!(entries[1].label.ends_with('…'));
        assert_eq!(entries[2].label, "F1 — Virtual console 1");
        assert_eq!(
            entries[3].label,
            "SUPER + T — [Launcher|Apps] terminal emulator"
        );
        assert_eq!(
            entries[0].keys,
            vec!["KEY_LEFTMETA".to_string(), "KEY_D".to_string()]
        );
        assert_eq!(
            entries[1].keys,
            vec![
                "KEY_LEFTSHIFT".to_string(),
                "KEY_LEFTCTRL".to_string(),
                "KEY_T".to_string()
            ]
        );
        assert_eq!(entries[2].keys, vec!["KEY_F1".to_string()]);
    }

    #[test]
    fn parse_bindings_survives_invalid_json() {
        assert_eq!(parse_bindings("not json"), Vec::new());
        assert_eq!(parse_bindings("{}"), Vec::new());
        assert_eq!(parse_bindings(""), Vec::new());
    }

    #[test]
    fn parse_bindings_accepts_unmodified_and_missing_fields() {
        let entries = parse_bindings(r#"[{"key":"Print"},{"key":"F5","modmask":0}]"#);
        assert_eq!(entries.len(), 1, "Print is not a replayable key name");
        assert_eq!(entries[0].keys, vec!["KEY_F5".to_string()]);
    }

    #[test]
    fn modmask_to_keys_maps_the_supported_bits() {
        assert_eq!(modmask_to_keys(0), Some(Vec::new()));
        assert_eq!(modmask_to_keys(1), Some(vec!["KEY_LEFTSHIFT".to_string()]));
        assert_eq!(modmask_to_keys(4), Some(vec!["KEY_LEFTCTRL".to_string()]));
        assert_eq!(modmask_to_keys(8), Some(vec!["KEY_LEFTALT".to_string()]));
        assert_eq!(modmask_to_keys(64), Some(vec!["KEY_LEFTMETA".to_string()]));
        assert_eq!(
            modmask_to_keys(5),
            Some(vec![
                "KEY_LEFTSHIFT".to_string(),
                "KEY_LEFTCTRL".to_string()
            ])
        );
        assert_eq!(
            modmask_to_keys(76),
            Some(vec![
                "KEY_LEFTCTRL".to_string(),
                "KEY_LEFTALT".to_string(),
                "KEY_LEFTMETA".to_string()
            ])
        );
    }

    #[test]
    fn modmask_to_keys_rejects_caps_and_unmapped_bits() {
        assert_eq!(modmask_to_keys(2), None);
        assert_eq!(modmask_to_keys(66), None);
        assert_eq!(modmask_to_keys(16), None);
        assert_eq!(modmask_to_keys(32), None);
        assert_eq!(modmask_to_keys(128), None);
        assert_eq!(modmask_to_keys(256), None);
        assert_eq!(modmask_to_keys(u32::MAX), None);
    }

    #[test]
    fn hypr_key_to_keycode_maps_letters_digits_and_named_keys() {
        assert_eq!(hypr_key_to_keycode("d"), Some("KEY_D".to_string()));
        assert_eq!(hypr_key_to_keycode("D"), Some("KEY_D".to_string()));
        assert_eq!(hypr_key_to_keycode("7"), Some("KEY_7".to_string()));
        assert_eq!(hypr_key_to_keycode("F1"), Some("KEY_F1".to_string()));
        assert_eq!(hypr_key_to_keycode("f12"), Some("KEY_F12".to_string()));
        assert_eq!(hypr_key_to_keycode("Return"), Some("KEY_ENTER".to_string()));
        assert_eq!(hypr_key_to_keycode("Tab"), Some("KEY_TAB".to_string()));
        assert_eq!(hypr_key_to_keycode("Space"), Some("KEY_SPACE".to_string()));
        assert_eq!(hypr_key_to_keycode("space"), Some("KEY_SPACE".to_string()));
        assert_eq!(
            hypr_key_to_keycode("BackSpace"),
            Some("KEY_BACKSPACE".to_string())
        );
        assert_eq!(
            hypr_key_to_keycode("Delete"),
            Some("KEY_DELETE".to_string())
        );
        assert_eq!(hypr_key_to_keycode("Escape"), Some("KEY_ESC".to_string()));
        assert_eq!(hypr_key_to_keycode("Home"), Some("KEY_HOME".to_string()));
        assert_eq!(hypr_key_to_keycode("End"), Some("KEY_END".to_string()));
        assert_eq!(
            hypr_key_to_keycode("Page_Up"),
            Some("KEY_PAGEUP".to_string())
        );
        assert_eq!(
            hypr_key_to_keycode("Page_Down"),
            Some("KEY_PAGEDOWN".to_string())
        );
        assert_eq!(hypr_key_to_keycode("left"), Some("KEY_LEFT".to_string()));
        assert_eq!(hypr_key_to_keycode("Right"), Some("KEY_RIGHT".to_string()));
        assert_eq!(hypr_key_to_keycode("up"), Some("KEY_UP".to_string()));
        assert_eq!(hypr_key_to_keycode("down"), Some("KEY_DOWN".to_string()));
        assert_eq!(hypr_key_to_keycode("comma"), Some("KEY_COMMA".to_string()));
        assert_eq!(hypr_key_to_keycode("period"), Some("KEY_DOT".to_string()));
        assert_eq!(hypr_key_to_keycode("slash"), Some("KEY_SLASH".to_string()));
        assert_eq!(
            hypr_key_to_keycode("semicolon"),
            Some("KEY_SEMICOLON".to_string())
        );
        assert_eq!(
            hypr_key_to_keycode("apostrophe"),
            Some("KEY_APOSTROPHE".to_string())
        );
        assert_eq!(hypr_key_to_keycode("grave"), Some("KEY_GRAVE".to_string()));
        assert_eq!(hypr_key_to_keycode("minus"), Some("KEY_MINUS".to_string()));
        assert_eq!(hypr_key_to_keycode("equal"), Some("KEY_EQUAL".to_string()));
        assert_eq!(
            hypr_key_to_keycode("bracketleft"),
            Some("KEY_LEFTBRACE".to_string())
        );
        assert_eq!(
            hypr_key_to_keycode("bracketright"),
            Some("KEY_RIGHTBRACE".to_string())
        );
        assert_eq!(
            hypr_key_to_keycode("backslash"),
            Some("KEY_BACKSLASH".to_string())
        );
    }

    #[test]
    fn hypr_key_to_keycode_rejects_unknown_names() {
        assert_eq!(hypr_key_to_keycode("mouse:276"), None);
        assert_eq!(hypr_key_to_keycode(""), None);
        assert_eq!(hypr_key_to_keycode("F13"), None);
        assert_eq!(hypr_key_to_keycode("A1"), None);
        assert_eq!(hypr_key_to_keycode("!"), None);
        assert_eq!(hypr_key_to_keycode("NOSUCHKEY"), None);
    }

    #[test]
    fn parsed_chords_resolve_through_the_driver_key_table() {
        for entry in parse_bindings(FIXTURE) {
            for key in &entry.keys {
                assert!(
                    key_code(key).is_some(),
                    "{key} from {:?} must resolve to an evdev code",
                    entry.label
                );
            }
        }
    }

    #[test]
    fn truncate_marks_long_text_and_keeps_short_text() {
        assert_eq!(truncate("rofi"), "rofi");
        let long = "a".repeat(ARG_DISPLAY_LIMIT + 10);
        let cut = truncate(&long);
        assert_eq!(cut.chars().count(), ARG_DISPLAY_LIMIT + 1);
        assert!(cut.ends_with('…'));
    }
}
