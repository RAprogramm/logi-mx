// SPDX-FileCopyrightText: 2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Radial pie menu runtime for the diverted gesture button.
//!
//! Pressing and holding the gesture button charges a thin ring around the
//! cursor; once the hold gate passes, the ring blooms into the full menu at
//! the cursor. The menu divides the circle into as many equal wedges as
//! there are installed commands — between two and eight — with slot 0
//! centred at 12 o'clock and the rest running clockwise, so the layout
//! follows what the user configured instead of a hand-set count. A quick
//! tap never charges: it releases the click binding instead, and a second
//! press within the double-click window skips the charge and opens the
//! menu straight away.
//!
//! Selection is directional, never positional: the wedge under the cursor
//! vector (or under a committed swipe direction) highlights, and releasing
//! fires that slot's action. The ring itself owns the actions —
//! [`logi_mx_driver::config::PieConfig::sectors`], optionally overridden
//! per application through
//! [`logi_mx_driver::config::DeviceConfig::pie_profiles`] — so a game can
//! carry its own wheel while the desktop keeps the default one.
//!
//! Swipes and the pie are one marking-menu mechanism, not two competing
//! ones: a swipe that commits while the ring is still charging is an expert
//! mark — it fires the directional binding and cancels the menu for that
//! hold; once the ring is open, swipe commits only select wedges. One hold
//! is exactly one outcome.
//!
//! Every input is a pure function or runs against an injected cursor
//! reader and an explicit clock, so the whole state machine is testable
//! without Hyprland and without sleeping. The press point is captured
//! through the Hyprland request socket
//! ([`crate::hyprbind::fetch_cursor_pos`]) because raw-XY deltas carry no
//! absolute position. Events reach the overlay through [`PieHub`].

pub mod geometry;
pub mod hub;
pub mod labels;
pub mod ring;
pub mod runtime;

pub use hub::{PieEvent, PieHub, PieRelease};
/// Re-export of the stored pie tuning so daemon consumers import it from
/// the pie module; the schema itself lives in the driver crate because the
/// device configuration embeds it.
pub use logi_mx_driver::config::PieConfig;
pub use ring::merge_effective;
#[allow(
    unused_imports,
    reason = "the daemon binary re-exports the pie API for its own modules"
)]
pub use runtime::{DOUBLE_CLICK_WINDOW, HOLD_DELAY, PieRuntime};
