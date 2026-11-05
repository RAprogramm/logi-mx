// SPDX-FileCopyrightText: 2025-2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Device-bound RPC requests: read-only queries and writes executed
//! against the pooled device, then mirrored into the stored config.

use logi_mx_driver::prelude::*;

use crate::{ipc, worker::pool::DevicePool};

/// Answers read-only device requests; `Ok(None)` for every other request.
fn handle_device_read(
    device: &mut MxMaster3s,
    request: &ipc::Request
) -> std::result::Result<Option<ipc::Response>, masterror::AppError> {
    Ok(Some(match request {
        ipc::Request::Info => handle_info(device),
        ipc::Request::Battery => handle_battery(device),
        ipc::Request::Hosts => {
            let (hosts, current) = device.host_info()?;
            ipc::Response::Hosts {
                hosts,
                current
            }
        }
        ipc::Request::Buttons => {
            let controls = device.list_reprog_controls()?;
            ipc::Response::Buttons {
                controls
            }
        }
        ipc::Request::GetButtonRemap {
            control_id
        } => {
            let (_, remap) = device.control_divert(*control_id)?;
            ipc::Response::ButtonRemap {
                control_id: *control_id,
                target_cid: remap
            }
        }
        ipc::Request::Dpi => {
            let value = device.dpi()?;
            ipc::Response::Dpi {
                value
            }
        }
        ipc::Request::SmartShift => {
            let config = device.smartshift()?;
            ipc::Response::SmartShift {
                config
            }
        }
        ipc::Request::HiresScroll => {
            let config = device.hires_scroll()?;
            ipc::Response::HiresScroll {
                config
            }
        }
        _ => return Ok(None)
    }))
}

/// Executes a device-bound RPC request against the pooled device.
pub(in crate::worker) fn handle_device_request(
    manager: &mut DevicePool,
    request: &ipc::Request
) -> ipc::Response {
    match manager.with_device(
        |device| -> std::result::Result<ipc::Response, masterror::AppError> {
            if let Some(response) = handle_device_read(device, request)? {
                return Ok(response);
            }
            Ok(match request {
                ipc::Request::SetDpi {
                    value
                } => {
                    device.set_dpi(*value)?;
                    ipc::Response::Ok
                }
                ipc::Request::SetSmartShift {
                    enabled,
                    threshold
                } => {
                    let cfg = SmartShiftConfig {
                        enabled:   *enabled,
                        threshold: *threshold
                    };
                    device.set_smartshift(cfg)?;
                    ipc::Response::Ok
                }
                ipc::Request::SetHires {
                    enabled,
                    inverted
                } => {
                    let cfg = HiResScrollConfig {
                        enabled:  *enabled,
                        inverted: *inverted
                    };
                    device.set_hires_scroll(cfg)?;
                    ipc::Response::Ok
                }
                ipc::Request::SetButtonDivert {
                    control_id,
                    divert,
                    raw_xy
                } => {
                    device.set_control_divert(*control_id, *divert, *raw_xy)?;
                    ipc::Response::Ok
                }
                ipc::Request::SetButtonRemap {
                    control_id,
                    target_cid
                } => {
                    device.set_control_remap(*control_id, *target_cid)?;
                    ipc::Response::Ok
                }
                ipc::Request::SetHost {
                    host
                } => {
                    device.set_host(*host)?;
                    #[cfg(feature = "notify")]
                    crate::notify::host_switched(*host);
                    ipc::Response::Ok
                }
                ipc::Request::WatchLive | ipc::Request::WatchPie => ipc::Response::Error {
                    message: "live streaming is handled by the IPC server".to_string()
                },
                _ => ipc::Response::Error {
                    message: "request is answered before device writes".to_string()
                }
            })
        }
    ) {
        Ok(response) => {
            if matches!(response, ipc::Response::Ok) {
                manager.persist_setting(request);
            }
            response
        }
        Err(e) => ipc::Response::Error {
            message: e.to_string()
        }
    }
}

/// Reports the device name, DPI and wheel configuration, falling back to
/// defaults when a read fails.
fn handle_info(device: &mut MxMaster3s) -> ipc::Response {
    ipc::Response::Info {
        name:       device
            .device_name()
            .unwrap_or_else(|_| "MX Master 3S".to_string()),
        dpi:        device.dpi().unwrap_or(1000),
        smartshift: device.smartshift().unwrap_or_default(),
        hires:      device.hires_scroll().unwrap_or_default()
    }
}

/// Reports the battery level and charging status.
fn handle_battery(device: &mut MxMaster3s) -> ipc::Response {
    match device.battery_info() {
        Ok(info) => ipc::Response::Battery {
            level:  info.level,
            status: format!("{:?}", info.status)
        },
        Err(e) => ipc::Response::Error {
            message: e.to_string()
        }
    }
}
