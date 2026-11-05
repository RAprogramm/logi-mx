// SPDX-FileCopyrightText: 2025-2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Tests for the backdrop mode tokens and blur fallback.

use super::*;

#[test]
fn background_mode_round_trips_through_bools_and_tokens() {
    for mode in BgMode::ALL {
        assert_eq!(BgMode::from_bools(mode.dim_on(), mode.wants_blur()), mode);
        assert_eq!(BgMode::from_token(mode.token()), mode);
    }
    assert_eq!(BgMode::from_bools(false, false), BgMode::None);
    assert_eq!(BgMode::from_bools(true, true), BgMode::BlurDim);
    assert_eq!(BgMode::from_token("garbage"), BgMode::default());
}

#[test]
fn without_blur_drops_compositor_modes() {
    assert_eq!(BgMode::Blur.without_blur(), BgMode::None);
    assert_eq!(BgMode::BlurDim.without_blur(), BgMode::Dim);
    assert_eq!(BgMode::None.without_blur(), BgMode::None);
    assert_eq!(BgMode::Dim.without_blur(), BgMode::Dim);
}
