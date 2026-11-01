// SPDX-FileCopyrightText: 2025 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Desktop notifications over the freedesktop Notifications D-Bus spec.
//!
//! Works with any compliant notification server (dunst, mako, hydrus, GNOME
//! Shell, KDE Plasma). Delivery is best-effort: failures are logged and never
//! affect device handling.
//!
//! [`NoticePolicy`] is a pure state machine over status snapshots and an
//! injected clock, so alert logic (hysteresis, escalation, reconnect re-arm)
//! is fully deterministic and unit-testable without a display server.

use std::time::Instant;

use notify_rust::{Hint, Notification, Urgency};
use tracing::warn;

use crate::status::{
    BATTERY_CRITICAL_PERCENT, BATTERY_DISCHARGING, BATTERY_LOW_PERCENT, DeviceStatus
};

/// Application name reported to the notification server.
const APP_NAME: &str = "Logi MX";
/// Icon shown for device notices.
const DEVICE_ICON: &str = "input-mouse-symbolic";
/// Notification category hint per the spec's predefined categories.
const NOTICE_CATEGORY: &str = "device";
/// Battery status string that makes charge alerts meaningful.
const DISCHARGING: &str = BATTERY_DISCHARGING;
/// Percentage above a threshold required to re-arm an alert (hysteresis).
const BATTERY_REARM_MARGIN: u8 = 5;

/// A user-facing event derived from device status transitions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceNotice {
    /// The mouse became reachable.
    Connected,
    /// The mouse stopped responding.
    Disconnected,
    /// Charge dropped to the warning level.
    BatteryLow {
        /// Current charge percentage.
        level: u8
    },
    /// Charge dropped to the critical level.
    BatteryCritical {
        /// Current charge percentage.
        level: u8
    }
}

/// Battery alert severity with an ordered escalation (`Critical > Low`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd)]
enum Severity {
    Low,
    Critical
}

impl Severity {
    /// Maps a charge percentage to its alert severity, if any.
    const fn for_level(level: u8) -> Option<Self> {
        if level <= BATTERY_CRITICAL_PERCENT {
            Some(Self::Critical)
        } else if level <= BATTERY_LOW_PERCENT {
            Some(Self::Low)
        } else {
            None
        }
    }

    /// Charge level that clears this severity's latch.
    const fn rearm_level(self) -> u8 {
        match self {
            Self::Low => BATTERY_LOW_PERCENT + BATTERY_REARM_MARGIN,
            Self::Critical => BATTERY_CRITICAL_PERCENT + BATTERY_REARM_MARGIN
        }
    }
}

/// Decides which [`DeviceNotice`] events a status snapshot implies.
///
/// The policy keeps only transition state: connection edges, the latched
/// battery severity, and hysteresis. The clock is injected per call, so the
/// state machine is deterministic.
///
/// # Examples
///
/// ```
/// use std::time::Instant;
///
/// use logi_mx_daemon::{
///     notify::{DeviceNotice, NoticePolicy},
///     status::DeviceStatus
/// };
///
/// let mut policy = NoticePolicy::new();
/// let now = Instant::now();
///
/// let offline = DeviceStatus::default();
/// assert!(policy.on_status(&offline, now).is_empty());
///
/// let mut online = DeviceStatus::default();
/// online.connected = true;
/// online.battery_status = "Discharging".to_string();
/// online.battery_level = 15;
/// let notices = policy.on_status(&online, now);
/// assert_eq!(
///     notices,
///     vec![
///         DeviceNotice::Connected,
///         DeviceNotice::BatteryLow {
///             level: 15
///         }
///     ]
/// );
///
/// assert!(policy.on_status(&online, now).is_empty());
/// ```
#[derive(Debug, Default)]
pub struct NoticePolicy {
    was_connected: bool,
    latched:       Option<Severity>
}

impl NoticePolicy {
    /// Creates a policy in the "never connected" state.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            was_connected: false,
            latched:       None
        }
    }

    /// Updates the state machine with the latest snapshot and returns the
    /// notices that should be displayed now.
    ///
    /// # Arguments
    ///
    /// * `status` - Current shared device status snapshot.
    /// * `now` - Clock reading used for future extensions; the current rules
    ///   depend only on transitions, keeping results deterministic.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::time::Instant;
    ///
    /// use logi_mx_daemon::{
    ///     notify::{DeviceNotice, NoticePolicy},
    ///     status::DeviceStatus
    /// };
    ///
    /// let mut policy = NoticePolicy::new();
    /// let now = Instant::now();
    /// let mut status = DeviceStatus::default();
    /// status.connected = true;
    /// assert_eq!(
    ///     policy.on_status(&status, now),
    ///     vec![DeviceNotice::Connected]
    /// );
    ///
    /// status.connected = false;
    /// assert_eq!(
    ///     policy.on_status(&status, now),
    ///     vec![DeviceNotice::Disconnected]
    /// );
    /// ```
    pub fn on_status(&mut self, status: &DeviceStatus, now: Instant) -> Vec<DeviceNotice> {
        let _ = now;
        let mut notices = Vec::new();

        if status.connected && !self.was_connected {
            self.latched = None;
            notices.push(DeviceNotice::Connected);
        } else if !status.connected && self.was_connected {
            notices.push(DeviceNotice::Disconnected);
        }
        self.was_connected = status.connected;

        if status.connected && status.battery_status == DISCHARGING {
            if let Some(severity) = Severity::for_level(status.battery_level) {
                if self.latched.is_none_or(|latched| severity > latched) {
                    self.latched = Some(severity);
                    notices.push(match severity {
                        Severity::Low => DeviceNotice::BatteryLow {
                            level: status.battery_level
                        },
                        Severity::Critical => DeviceNotice::BatteryCritical {
                            level: status.battery_level
                        }
                    });
                }
            } else if self
                .latched
                .is_some_and(|latched| status.battery_level > latched.rearm_level())
            {
                self.latched = None;
            }
        }

        notices
    }
}

/// Sends a notice to the desktop notification server, logging failures.
///
/// # Examples
///
/// ```no_run
/// use logi_mx_daemon::notify::{DeviceNotice, publish};
///
/// publish(&DeviceNotice::Connected);
/// ```
pub fn publish(notice: &DeviceNotice) {
    let mut notification = base_notification();
    match notice {
        DeviceNotice::Connected => {
            notification
                .summary("MX Master 3S connected")
                .body("The mouse is ready.");
        }
        DeviceNotice::Disconnected => {
            notification
                .summary("MX Master 3S disconnected")
                .body("The mouse is no longer reachable.");
        }
        DeviceNotice::BatteryLow {
            level
        } => {
            notification
                .summary("Battery low")
                .body(&format!("MX Master 3S battery at {level}%."))
                .urgency(Urgency::Low);
        }
        DeviceNotice::BatteryCritical {
            level
        } => {
            notification
                .summary("Battery critical")
                .body(&format!(
                    "MX Master 3S battery at {level}% — connect the charger."
                ))
                .urgency(Urgency::Critical);
        }
    }

    if let Err(e) = notification.show() {
        warn!("Failed to show desktop notification: {e}");
    }
}

/// Announces an Easy-Switch host change, logging delivery failures.
///
/// # Examples
///
/// ```no_run
/// logi_mx_daemon::notify::host_switched(1);
/// ```
pub fn host_switched(host: u8) {
    let mut notification = base_notification();
    notification.summary("Switched host").body(&format!(
        "Active Easy-Switch host is {host} (zero-indexed)."
    ));

    if let Err(e) = notification.show() {
        warn!("Failed to show desktop notification: {e}");
    }
}

fn base_notification() -> Notification {
    let mut notification = Notification::new();
    notification
        .appname(APP_NAME)
        .icon(DEVICE_ICON)
        .hint(Hint::Category(NOTICE_CATEGORY.to_owned()));
    notification
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(connected: bool, level: u8, battery_status: &str) -> DeviceStatus {
        DeviceStatus {
            connected,
            battery_level: level,
            battery_status: battery_status.to_string(),
            ..DeviceStatus::default()
        }
    }

    #[test]
    fn no_notices_while_never_connected() {
        let mut policy = NoticePolicy::new();
        let now = Instant::now();

        assert_eq!(
            policy.on_status(&status(false, 5, DISCHARGING), now),
            Vec::new()
        );
        assert_eq!(
            policy.on_status(&status(false, 5, DISCHARGING), now),
            Vec::new()
        );
    }

    #[test]
    fn connect_edge_fires_once() {
        let mut policy = NoticePolicy::new();
        let now = Instant::now();

        assert_eq!(
            policy.on_status(&status(true, 80, DISCHARGING), now),
            vec![DeviceNotice::Connected]
        );
        assert_eq!(
            policy.on_status(&status(true, 80, DISCHARGING), now),
            Vec::new()
        );
    }

    #[test]
    fn disconnect_edge_fires_once() {
        let mut policy = NoticePolicy::new();
        let now = Instant::now();

        policy.on_status(&status(true, 80, DISCHARGING), now);
        assert_eq!(
            policy.on_status(&status(false, 80, DISCHARGING), now),
            vec![DeviceNotice::Disconnected]
        );
        assert_eq!(
            policy.on_status(&status(false, 80, DISCHARGING), now),
            Vec::new()
        );
    }

    #[test]
    fn battery_low_fires_once_at_threshold() {
        let mut policy = NoticePolicy::new();
        let now = Instant::now();

        policy.on_status(&status(true, 21, DISCHARGING), now);
        assert_eq!(
            policy.on_status(&status(true, 20, DISCHARGING), now),
            vec![DeviceNotice::BatteryLow {
                level: 20
            }]
        );
        assert_eq!(
            policy.on_status(&status(true, 19, DISCHARGING), now),
            Vec::new()
        );
    }

    #[test]
    fn critical_escalates_over_low() {
        let mut policy = NoticePolicy::new();
        let now = Instant::now();

        policy.on_status(&status(true, 20, DISCHARGING), now);
        assert_eq!(
            policy.on_status(&status(true, 10, DISCHARGING), now),
            vec![DeviceNotice::BatteryCritical {
                level: 10
            }]
        );
        assert_eq!(
            policy.on_status(&status(true, 9, DISCHARGING), now),
            Vec::new()
        );
    }

    #[test]
    fn charging_suppresses_battery_alerts() {
        let mut policy = NoticePolicy::new();
        let now = Instant::now();

        assert_eq!(
            policy.on_status(&status(true, 5, "Charging"), now),
            vec![DeviceNotice::Connected]
        );
        assert_eq!(
            policy.on_status(&status(true, 5, "Charging"), now),
            Vec::new()
        );
    }

    #[test]
    fn unknown_battery_status_suppresses_alerts() {
        let mut policy = NoticePolicy::new();
        let now = Instant::now();

        assert_eq!(
            policy.on_status(&status(true, 0, "Unknown"), now),
            vec![DeviceNotice::Connected]
        );
        assert_eq!(
            policy.on_status(&status(true, 0, "Unknown"), now),
            Vec::new()
        );
    }

    #[test]
    fn hysteresis_requires_margin_before_rearm() {
        let mut policy = NoticePolicy::new();
        let now = Instant::now();

        policy.on_status(&status(true, 20, DISCHARGING), now);
        assert_eq!(
            policy.on_status(&status(true, 24, DISCHARGING), now),
            Vec::new()
        );
        assert_eq!(
            policy.on_status(&status(true, 25, DISCHARGING), now),
            Vec::new()
        );
        assert_eq!(
            policy.on_status(&status(true, 26, DISCHARGING), now),
            Vec::new()
        );

        assert_eq!(
            policy.on_status(&status(true, 20, DISCHARGING), now),
            vec![DeviceNotice::BatteryLow {
                level: 20
            }]
        );
    }

    #[test]
    fn reconnect_rearms_battery_alerts() {
        let mut policy = NoticePolicy::new();
        let now = Instant::now();

        policy.on_status(&status(true, 15, DISCHARGING), now);
        policy.on_status(&status(false, 15, DISCHARGING), now);
        assert_eq!(
            policy.on_status(&status(true, 15, DISCHARGING), now),
            vec![
                DeviceNotice::Connected,
                DeviceNotice::BatteryLow {
                    level: 15
                }
            ]
        );
    }

    #[test]
    fn severity_ordering() {
        assert!(Severity::Critical > Severity::Low);
        assert_eq!(Severity::for_level(0), Some(Severity::Critical));
        assert_eq!(Severity::for_level(10), Some(Severity::Critical));
        assert_eq!(Severity::for_level(11), Some(Severity::Low));
        assert_eq!(Severity::for_level(20), Some(Severity::Low));
        assert_eq!(Severity::for_level(21), None);
        assert_eq!(Severity::Low.rearm_level(), 25);
        assert_eq!(Severity::Critical.rearm_level(), 15);
    }
}
