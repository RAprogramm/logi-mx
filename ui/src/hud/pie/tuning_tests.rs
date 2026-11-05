// SPDX-FileCopyrightText: 2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Parsing tests for the pie tuning entries.

use super::parse_pie_px;

#[test]
fn pie_px_accepts_whole_positive_pixels() {
    assert_eq!(parse_pie_px("40"), Some(40));
    assert_eq!(parse_pie_px(" 12 "), Some(12));
    assert_eq!(parse_pie_px("1"), Some(1));
    assert_eq!(parse_pie_px("4000"), Some(4000));
}

#[test]
fn pie_px_rejects_garbage_and_out_of_range() {
    assert_eq!(parse_pie_px(""), None);
    assert_eq!(parse_pie_px("0"), None);
    assert_eq!(parse_pie_px("-5"), None);
    assert_eq!(parse_pie_px("4001"), None);
    assert_eq!(parse_pie_px("wide"), None);
    assert_eq!(parse_pie_px("12.5"), None);
}
