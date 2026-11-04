// SPDX-FileCopyrightText: 2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Unit tests for the pie stream line decoding: serde round-trips of every
//! event shape and the skip-everything-else contract.

use logi_mx_daemon::{
    ipc::Response,
    pie::{PieEvent, PieSector}
};
use logi_mx_driver::devices::GestureDirection;

use super::decode_line;

#[test]
fn open_event_round_trips_through_a_stream_line() {
    let event = PieEvent::Open {
        anchor:  (1920, 540),
        sectors: vec![
            PieSector {
                direction: GestureDirection::Up,
                label:     "Up".to_string(),
                command:   None
            },
            PieSector {
                direction: GestureDirection::UpLeft,
                label:     "UpLeft".to_string(),
                command:   None
            },
        ]
    };
    let line = serde_json::to_string(&Response::Pie(event.clone())).unwrap_or_default();
    assert_eq!(decode_line(&line), Some(event));
}

#[test]
fn highlight_with_null_direction_decodes() {
    let line = r#"{"Pie":{"Highlight":{"direction":null}}}"#;
    assert_eq!(
        decode_line(line),
        Some(PieEvent::Highlight {
            direction: None
        })
    );
}

#[test]
fn close_with_committed_direction_decodes() {
    let line = serde_json::to_string(&Response::Pie(PieEvent::Close {
        committed: Some(GestureDirection::DownRight)
    }))
    .unwrap_or_default();
    assert_eq!(
        decode_line(&line),
        Some(PieEvent::Close {
            committed: Some(GestureDirection::DownRight)
        })
    );
}

#[test]
fn malformed_and_foreign_lines_are_skipped() {
    assert_eq!(decode_line("not json"), None);
    assert_eq!(decode_line(""), None);
    assert_eq!(decode_line(r#"{"Pie":{}}"#), None);
    assert_eq!(decode_line("\"Pong\""), None);
    assert_eq!(decode_line("\"LiveReady\""), None);
}
