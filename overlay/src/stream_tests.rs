// SPDX-FileCopyrightText: 2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Unit tests for the pie stream line decoding: serde round-trips of every
//! event shape and the skip-everything-else contract.

use logi_mx_daemon::{
    ipc::Response,
    pie::{PieEvent, labels::PieSector}
};

use super::decode_line;

#[test]
fn open_event_round_trips_through_a_stream_line() {
    let event = PieEvent::Open {
        anchor:  (1920, 540),
        sectors: vec![
            PieSector {
                index:   0,
                label:   "Meta+E".to_string(),
                command: None,
                bound:   true
            },
            PieSector {
                index:   1,
                label:   String::new(),
                command: None,
                bound:   false
            },
        ]
    };
    let line = serde_json::to_string(&Response::Pie(event.clone())).unwrap_or_default();
    assert_eq!(decode_line(&line), Some(event));
}

#[test]
fn highlight_with_null_index_decodes() {
    let line = r#"{"Pie":{"Highlight":{"index":null}}}"#;
    assert_eq!(
        decode_line(line),
        Some(PieEvent::Highlight {
            index: None
        })
    );
}

#[test]
fn close_with_committed_index_decodes() {
    let line = serde_json::to_string(&Response::Pie(PieEvent::Close {
        committed: Some(3)
    }))
    .unwrap_or_default();
    assert_eq!(
        decode_line(&line),
        Some(PieEvent::Close {
            committed: Some(3)
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
