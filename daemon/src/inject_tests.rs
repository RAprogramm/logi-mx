// SPDX-FileCopyrightText: 2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

use super::*;

fn seq(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| (*name).to_string()).collect()
}

#[test]
fn test_resolve_sequence_preserves_order() {
    assert_eq!(
        resolve_sequence(&seq(&["KEY_LEFTCTRL", "KEY_C"])).unwrap(),
        vec![29, 46]
    );
    assert_eq!(resolve_sequence(&seq(&["KEY_UP"])).unwrap(), vec![103]);
}

#[test]
fn test_resolve_sequence_rejects_unknown_name() {
    let error = resolve_sequence(&seq(&["KEY_LEFTCTRL", "KEY_NOPE"])).unwrap_err();
    assert!(
        error.to_string().contains("KEY_NOPE"),
        "name must be reported"
    );
}

#[test]
fn test_resolve_sequence_rejects_empty_list() {
    assert!(resolve_sequence(&[]).is_err());
}

#[test]
fn test_injector_starts_without_touching_uinput() {
    let injector = Injector::default();
    assert!(injector.device.is_none());
}

#[test]
fn test_press_rejects_bad_names_before_touching_device() {
    let mut injector = Injector::default();
    assert!(injector.press_sequence(&seq(&["KEY_NOPE"])).is_err());
    assert!(injector.press_sequence(&[]).is_err());
    assert!(
        injector.device.is_none(),
        "a validation error must not create the virtual device"
    );
}
