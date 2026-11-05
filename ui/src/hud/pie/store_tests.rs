// SPDX-FileCopyrightText: 2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Unit tests for the pie page store: scope fallbacks, request routing and
//! local mirroring of successful writes.

use logi_mx_driver::{
    config::{PieProfile, PieSlot},
    prelude::Action
};

use super::{PieScope, PieStore};

fn key_slot(key: &str) -> PieSlot {
    PieSlot {
        action: Some(Action::Keypress {
            keys: vec![format!("KEY_{key}")]
        }),
        label:  None
    }
}

fn store_with_global(count: usize) -> PieStore {
    let mut store = PieStore::default();
    store.config.sectors = (0..count).map(|_| key_slot("A")).collect();
    store
}

#[test]
fn global_scope_shows_the_global_ring() {
    let store = store_with_global(3);
    assert_eq!(store.visible_ring().len(), 3);
    assert!(matches!(
        store.set_ring_request(&[]),
        logi_mx_daemon::ipc::Request::SetPieSectors { .. }
    ));
}

#[test]
fn app_scope_shows_its_own_profile_or_nothing() {
    let mut store = store_with_global(4);
    store.scope = PieScope::App("firefox".to_string());
    assert!(
        store.visible_ring().is_empty(),
        "an app without a profile edits nothing, not the global ring"
    );
    store.profiles.insert(
        "firefox".to_string(),
        PieProfile {
            sectors: (0..2).map(|_| key_slot("F")).collect()
        }
    );
    assert_eq!(store.visible_ring().len(), 2);
    assert!(matches!(
        store.set_ring_request(&[]),
        logi_mx_daemon::ipc::Request::SetAppPieSectors { .. }
    ));
}

#[test]
fn apply_ring_mirrors_writes_per_scope() {
    let mut store = store_with_global(2);
    store.apply_ring((0..5).map(|_| key_slot("B")).collect());
    assert_eq!(
        store.config.sectors.len(),
        5,
        "global writes land in config"
    );

    store.scope = PieScope::App("minecraft".to_string());
    store.apply_ring((0..6).map(|_| key_slot("M")).collect());
    assert_eq!(store.profiles["minecraft"].sectors.len(), 6);
    assert_eq!(
        store.config.sectors.len(),
        5,
        "the global ring stays intact"
    );

    store.apply_ring(Vec::new());
    assert!(
        !store.profiles.contains_key("minecraft"),
        "an empty app ring removes the profile, matching the daemon"
    );
}

#[test]
fn apps_are_sorted_for_the_scope_dropdown() {
    let mut store = PieStore::default();
    for app in ["zoom", "alacritty", "firefox"] {
        store.profiles.insert(
            app.to_string(),
            PieProfile {
                sectors: vec![key_slot("A"), key_slot("B")]
            }
        );
    }
    assert_eq!(store.apps(), vec!["alacritty", "firefox", "zoom"]);
}
