// SPDX-FileCopyrightText: 2025-2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Live input telemetry: the socket pump thread and the visual state it
//! feeds — pressed-button flashes, motion trails and wheel velocity.

use std::{
    cell::RefCell,
    io::BufReader,
    rc::Rc,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU32, Ordering}
    },
    thread,
    time::Duration
};

use gtk4::glib;
use libadwaita::prelude::*;
use logi_mx_daemon::ipc::{self, LiveFrame, Response};

const LIVE_EMA_ALPHA: f64 = 0.3;
/// Interval between idle decays of the live gauges.
const LIVE_DECAY_INTERVAL: Duration = Duration::from_millis(250);
/// Delay before the live subscription reconnects after a disconnect.
const LIVE_RETRY_DELAY: Duration = Duration::from_secs(1);
/// Read timeout of the live socket, bounding how long a stop request waits.
const LIVE_POLL_TIMEOUT: Duration = Duration::from_millis(500);

/// Smoothed live input picture shown on the diagram.
#[derive(Default, Debug)]
pub(super) struct LiveVisual {
    pub(super) buttons:     u32,
    /// Buttons pressed between two HUD frames; keeps brief taps visible.
    pub(super) flash:       u32,
    pub(super) wheel_speed: f64,
    pub(super) wheel_dir:   i32,
    pub(super) hwheel_dir:  i32,
    pub(super) extra_keys:  Vec<u16>,
    pub(super) connected:   bool
}

impl LiveVisual {
    /// Folds one telemetry frame into the smoothed gauges.
    fn apply(&mut self, frame: &LiveFrame) {
        self.buttons = frame.buttons;
        self.extra_keys.clone_from(&frame.extra_keys);
        self.connected = true;

        let dt = f64::from(frame.dt_ms.max(1)) / 1000.0;
        let wheel_units = (frame.wheel.abs() * 120).max(frame.wheel_hi.abs());
        let instant_wheel = f64::from(wheel_units) / dt / 120.0;
        self.wheel_speed = if self.wheel_speed == 0.0 {
            instant_wheel
        } else {
            blend(self.wheel_speed, instant_wheel)
        };
        if frame.wheel_hi != 0 {
            self.wheel_dir = frame.wheel_hi.signum();
        }
        if frame.hwheel_hi != 0 {
            self.hwheel_dir = frame.hwheel_hi.signum();
        }
    }

    /// Folds one main-context live update: the newest frame, the stream state
    /// and the tap flashes collected between two wakes.
    ///
    /// A dropped stream clears the pressed-button mask so a button cannot
    /// stay tinted on a stale mask while no telemetry arrives.
    fn fold(&mut self, frame: Option<LiveFrame>, connected: bool, flashed: u32) {
        if let Some(frame) = frame {
            self.apply(&frame);
        }
        self.connected = connected;
        if !connected {
            self.buttons = 0;
        }
        self.flash |= flashed;
    }

    /// Fades the velocity gauges toward zero and clears the tap flashes while
    /// the device is idle.
    ///
    /// Returns whether any gauge value actually moved.
    fn decay(&mut self) -> bool {
        let mut changed = self.flash != 0;
        self.flash = 0;
        if self.wheel_speed != 0.0 {
            self.wheel_speed *= 0.6;
            if self.wheel_speed < 0.5 {
                self.wheel_speed = 0.0;
            }
            changed = true;
        }
        changed
    }
}

/// Exponential smoothing of a new sample into a running value.
#[must_use]
fn blend(previous: f64, sample: f64) -> f64 {
    sample.mul_add(LIVE_EMA_ALPHA, previous * (1.0 - LIVE_EMA_ALPHA))
}

/// Subscribes to daemon telemetry and folds frames into the HUD state.
///
/// A background thread owns the socket and survives daemon restarts; each
/// frame wakes the main context through an invoke, where the window hook
/// applies it. Closing the HUD clears the hook and stops the thread.
pub(super) fn start_live(state: &Rc<RefCell<HudState>>, drawing: &gtk4::DrawingArea) {
    let stop = Arc::new(AtomicBool::new(false));
    let slot = Arc::new(Mutex::new(None::<LiveFrame>));
    let burst = Arc::new(AtomicU32::new(0));

    let connected = Arc::new(AtomicBool::new(false));
    let slot_hook = Arc::clone(&slot);
    let burst_hook = Arc::clone(&burst);
    let connected_hook = Arc::clone(&connected);
    let state_hook = Rc::clone(state);
    let drawing_hook = drawing.clone();
    let hook: Rc<dyn Fn()> = Rc::new(move || {
        let frame = slot_hook.lock().ok().and_then(|mut guard| guard.take());
        let flashed = burst_hook.swap(0, Ordering::Relaxed);
        let connected = connected_hook.load(Ordering::Relaxed);
        state_hook.borrow_mut().live.fold(frame, connected, flashed);
        drawing_hook.queue_draw();
    });
    LIVE_HOOK.with(|cell| *cell.borrow_mut() = Some(Rc::clone(&hook)));
    LIVE_STOP.with(|cell| *cell.borrow_mut() = Some(Arc::clone(&stop)));

    let context = glib::MainContext::default();
    let stop_thread = Arc::clone(&stop);
    let slot_thread = Arc::clone(&slot);
    let burst_thread = Arc::clone(&burst);
    let connected_thread = Arc::clone(&connected);
    let pending = Arc::new(AtomicBool::new(false));
    let wake = move || {
        let context = context.clone();
        let pending = Arc::clone(&pending);
        if pending.swap(true, Ordering::Relaxed) {
            return;
        }
        context.invoke(move || {
            pending.store(false, Ordering::Relaxed);
            LIVE_HOOK.with(|cell| {
                if let Some(hook) = cell.borrow().as_ref() {
                    hook();
                }
            });
        });
    };
    if let Err(e) = thread::Builder::new()
        .name("hud-live".to_string())
        .spawn(move || {
            while !stop_thread.load(Ordering::Relaxed) {
                let Ok(socket) = ipc::open_live_stream() else {
                    thread::sleep(LIVE_RETRY_DELAY);
                    continue;
                };
                connected_thread.store(true, Ordering::Relaxed);
                wake();
                pump_live_socket(
                    socket,
                    &slot_thread,
                    &burst_thread,
                    &connected_thread,
                    &stop_thread,
                    &wake
                );
            }
        })
    {
        tracing::warn!("Failed to spawn live subscription thread: {e}");
    }

    let state_decay = Rc::clone(state);
    let drawing_decay = drawing.clone();
    glib::timeout_add_local(LIVE_DECAY_INTERVAL, move || {
        if ACTIVE_WINDOW.with(|window| window.borrow().is_none()) {
            return glib::ControlFlow::Break;
        }
        let changed = state_decay.borrow_mut().live.decay();
        if changed {
            drawing_decay.queue_draw();
        }
        glib::ControlFlow::Continue
    });
}

/// Reads frames from one live socket into the shared slot until it drops.
///
/// Every frame's button bits are also folded into `burst`, so a press that
/// starts and ends between two main-context wakes still reaches the flash
/// mask instead of being overwritten by the newest frame.
fn pump_live_socket(
    socket: std::os::unix::net::UnixStream,
    slot: &Arc<Mutex<Option<LiveFrame>>>,
    burst: &Arc<AtomicU32>,
    connected: &Arc<AtomicBool>,
    stop: &AtomicBool,
    wake: &dyn Fn()
) {
    let _ = socket.set_read_timeout(Some(LIVE_POLL_TIMEOUT));
    let mut reader = BufReader::new(socket);
    loop {
        let mut line = String::new();
        match std::io::BufRead::read_line(&mut reader, &mut line) {
            Ok(0) => break,
            Ok(_) => {
                let parsed = serde_json::from_str::<Response>(&line);
                let Ok(Response::Live(frame)) = parsed else {
                    continue;
                };
                burst.fetch_or(frame.buttons, Ordering::Relaxed);
                if let Ok(mut guard) = slot.lock() {
                    *guard = Some(frame);
                }
                wake();
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if stop.load(Ordering::Relaxed) {
                    break;
                }
            }
            Err(_) => break
        }
    }
    connected.store(false, Ordering::Relaxed);
    wake();
}

use super::{ACTIVE_WINDOW, HudState, LIVE_HOOK, LIVE_STOP};

#[cfg(test)]
#[path = "live_tests.rs"]
mod tests;
