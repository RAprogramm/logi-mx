// SPDX-FileCopyrightText: 2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Unit tests for journal decoding, timestamp formatting and the log
//! filter, including malformed input and every filter rejection path.

use super::{LogEntry, LogFilter, clock_of, date_of, parse_journal_line, unit_label};

fn entry(unit: &str, priority: u8, message: &str) -> LogEntry {
    LogEntry {
        timestamp_us: 1_793_761_932_483_136,
        unit: unit.to_string(),
        priority,
        message: message.to_string()
    }
}

#[test]
fn parses_a_well_formed_journal_record() {
    let line = r#"{"__REALTIME_TIMESTAMP":"1793761932483136","PRIORITY":"6",
        "SYSLOG_IDENTIFIER":"logi-mx-daemon","MESSAGE":"worker started"}"#;
    let parsed = parse_journal_line(line).expect("a well-formed record decodes");
    assert_eq!(parsed.unit, "daemon");
    assert_eq!(parsed.message, "worker started");
    assert_eq!(parsed.priority, 6);
    assert_eq!(parsed.timestamp_us, 1_793_761_932_483_136);
}

#[test]
fn rejects_malformed_or_incomplete_records() {
    assert!(parse_journal_line("not json").is_none());
    assert!(parse_journal_line("{}").is_none());
    assert!(parse_journal_line(r#"{"PRIORITY":"6","MESSAGE":"no ts"}"#).is_none());
    assert!(parse_journal_line(r#"{"__REALTIME_TIMESTAMP":"x","PRIORITY":"6"}"#).is_none());
    assert!(
        parse_journal_line(r#"{"__REALTIME_TIMESTAMP":"1","PRIORITY":"6","MESSAGE":[104,105]}"#)
            .is_none(),
        "byte-array messages (non-UTF-8) are skipped"
    );
}

#[test]
fn defaults_the_identifier_when_absent() {
    let line = r#"{"__REALTIME_TIMESTAMP":"1","PRIORITY":"4","MESSAGE":"x"}"#;
    let parsed = parse_journal_line(line).expect("identifier is optional");
    assert_eq!(parsed.unit, "logi-mx");
}

#[test]
fn strips_the_project_prefix_from_identifiers() {
    assert_eq!(unit_label("logi-mx-daemon"), "daemon");
    assert_eq!(unit_label("logi-mx-ui"), "ui");
    assert_eq!(unit_label("kernel"), "kernel");
    assert_eq!(unit_label(""), "");
}

#[test]
fn formats_dates_and_clocks() {
    assert_eq!(date_of(0).len(), 10);
    assert_eq!(clock_of(0).len(), 8);
    assert_eq!(date_of(0).matches('-').count(), 2);
    assert_eq!(clock_of(0).matches(':').count(), 2);
    assert_eq!(date_of(0), date_of(1_000_000), "one second stays in a day");
}

#[test]
fn filter_passes_matching_units_and_priorities() {
    let daemon = entry("daemon", 6, "worker started");
    let overlay = entry("overlay", 3, "surface lost");
    let filter = LogFilter::default();
    assert!(filter.visible(&daemon));
    assert!(filter.visible(&overlay));

    let errors_only = LogFilter {
        max_prio: 3,
        ..Default::default()
    };
    assert!(!errors_only.visible(&daemon), "info is below the cutoff");
    assert!(errors_only.visible(&overlay));
}

#[test]
fn filter_hides_disabled_units() {
    let daemon = entry("daemon", 6, "x");
    let overlay = entry("overlay", 6, "x");
    assert!(
        !LogFilter {
            daemon: false,
            ..Default::default()
        }
        .visible(&daemon)
    );
    assert!(
        !LogFilter {
            overlay: false,
            ..Default::default()
        }
        .visible(&overlay)
    );
    assert!(
        LogFilter {
            overlay: false,
            ..Default::default()
        }
        .visible(&daemon)
    );
}

#[test]
fn filter_searches_case_insensitively_in_message_and_unit() {
    let daemon = entry("daemon", 6, "Worker started");
    assert!(
        LogFilter {
            query: "WORKER".to_string(),
            ..Default::default()
        }
        .visible(&daemon)
    );
    assert!(
        LogFilter {
            query: "DAEMON".to_string(),
            ..Default::default()
        }
        .visible(&daemon)
    );
    assert!(
        !LogFilter {
            query: "crash".to_string(),
            ..Default::default()
        }
        .visible(&daemon)
    );
}
