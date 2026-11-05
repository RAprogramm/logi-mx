// SPDX-FileCopyrightText: 2025-2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Tests for the live visual state: flashes, wheel velocity and folds.

use logi_mx_daemon::ipc::{BTN_BIT_GESTURE, BTN_BIT_LEFT, BTN_BIT_RIGHT, LiveFrame};

use super::*;

#[test]
fn motion_frame_marks_connection() {
    let mut live = LiveVisual::default();
    assert!(!live.connected);
    let frame = LiveFrame {
        dx: 30,
        dy: 40,
        dt_ms: 10,
        ..LiveFrame::default()
    };
    live.apply(&frame);

    assert!(live.connected);
    assert_eq!(live.wheel_speed, 0.0);
}

#[test]
fn live_wheel_direction_and_decay() {
    let mut live = LiveVisual::default();
    let frame = LiveFrame {
        wheel_hi: -240,
        dt_ms: 20,
        ..LiveFrame::default()
    };
    live.apply(&frame);
    assert_eq!(live.wheel_dir, -1);
    assert!(live.wheel_speed > 0.0);

    for _ in 0..30 {
        live.decay();
    }
    assert_eq!(live.wheel_speed, 0.0);
}

#[test]
fn flash_survives_release_and_decays_away() {
    let mut live = LiveVisual::default();
    let pressed = LiveFrame {
        buttons: BTN_BIT_LEFT,
        ..LiveFrame::default()
    };
    live.apply(&pressed);
    live.flash |= BTN_BIT_LEFT;

    let released = LiveFrame {
        buttons: 0,
        ..LiveFrame::default()
    };
    live.apply(&released);
    assert_eq!(live.buttons, 0);
    assert_ne!(live.flash & BTN_BIT_LEFT, 0);

    assert!(live.decay());
    assert_eq!(live.flash, 0);
    assert!(!live.decay());
}

#[test]
fn disconnect_clears_pressed_buttons() {
    let mut live = LiveVisual::default();
    let pressed = LiveFrame {
        buttons: BTN_BIT_LEFT,
        ..LiveFrame::default()
    };
    live.fold(Some(pressed), true, 0);
    assert_ne!(live.buttons & BTN_BIT_LEFT, 0);

    live.fold(None, false, 0);
    assert!(!live.connected);
    assert_eq!(live.buttons, 0);
}

#[test]
fn disconnected_frame_cannot_restore_pressed_buttons() {
    let mut live = LiveVisual::default();
    let pressed = LiveFrame {
        buttons: BTN_BIT_RIGHT,
        ..LiveFrame::default()
    };
    live.fold(Some(pressed), false, 0);

    assert!(!live.connected);
    assert_eq!(live.buttons, 0);
}

#[test]
fn connected_fold_keeps_buttons_and_flashes() {
    let mut live = LiveVisual::default();
    live.fold(None, true, BTN_BIT_GESTURE);

    assert!(live.connected);
    assert_eq!(live.flash & BTN_BIT_GESTURE, BTN_BIT_GESTURE);
}
