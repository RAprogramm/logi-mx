// SPDX-FileCopyrightText: 2025 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

pub mod mx_master_3s;
pub mod traits;

pub use mx_master_3s::{
    MxMaster3s, ReprogControl, control_id, control_id_name, parse_battery_status,
    parse_hires_mode, parse_smartshift
};
pub use traits::*;
