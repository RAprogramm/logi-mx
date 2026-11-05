// SPDX-FileCopyrightText: 2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

use super::*;

#[test]
fn test_key_name_resolves_common_codes() {
    assert_eq!(key_name(1), Some("KEY_ESC"));
    assert_eq!(key_name(30), Some("KEY_A"));
    assert_eq!(key_name(103), Some("KEY_UP"));
    assert_eq!(key_name(125), Some("KEY_LEFTMETA"));
}

#[test]
fn test_key_name_rejects_unknown_codes() {
    assert_eq!(key_name(0), None);
    assert_eq!(key_name(768), None);
    assert_eq!(key_name(u16::MAX), None);
}

#[test]
fn test_key_code_resolves_names() {
    assert_eq!(key_code("KEY_A"), Some(30));
    assert_eq!(key_code("KEY_LEFTCTRL"), Some(29));
    assert_eq!(key_code("KEY_LEFTMETA"), Some(125));
}

#[test]
fn test_key_code_rejects_unknown_names() {
    assert_eq!(key_code("KEY_NOPE"), None);
    assert_eq!(key_code(""), None);
    assert_eq!(key_code("A"), None);
}

#[test]
fn test_roundtrip_name_code() {
    for (code, name) in KEY_NAMES {
        assert_eq!(key_code(name), Some(code), "name {name} must map back");
        assert_eq!(key_name(code), Some(name), "code {code} must map back");
    }
}

#[test]
fn test_table_is_sorted_and_unique() {
    for pair in KEY_NAMES.windows(2) {
        assert!(pair[0].0 < pair[1].0, "codes must be strictly ascending");
        assert_ne!(pair[0].1, pair[1].1, "names must be unique");
    }
}
