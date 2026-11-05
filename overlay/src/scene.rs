// SPDX-FileCopyrightText: 2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Pure renderer state: monitor mapping, sector geometry and the scene the
//! draw callbacks paint.
//!
//! The daemon streams the press anchor in Hyprland global coordinates, which
//! are logical pixels, and GDK reports monitor geometry in the same logical
//! space. This module maps the anchor onto the monitor that contains it and
//! keeps the small state machine the overlay paints: nothing here touches
//! GTK, sockets or the clock, so the whole mapping is unit-testable headless.

use logi_mx_daemon::pie::{PieEvent, labels::PieSector};

/// Logical geometry of one monitor, in Hyprland global coordinates.
///
/// `x` and `y` are the monitor's origin in the global logical layout,
/// `width` and `height` its logical size and `scale` the integer scale
/// factor (at least 1) GDK reports for it, kept for painting hints.
///
/// # Examples
///
/// ```
/// use logi_mx_overlay::scene::MonitorGeom;
///
/// let monitor = MonitorGeom {
///     x:      0,
///     y:      0,
///     width:  1920,
///     height: 1080,
///     scale:  2
/// };
/// assert!(monitor.contains((100, 200)));
/// assert!(!monitor.contains((1920, 0)));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MonitorGeom {
    /// Global logical x of the monitor's top-left corner.
    pub x:      i32,
    /// Global logical y of the monitor's top-left corner.
    pub y:      i32,
    /// Logical width in pixels.
    pub width:  i32,
    /// Logical height in pixels.
    pub height: i32,
    /// Integer scale factor, at least 1.
    pub scale:  i32
}

impl MonitorGeom {
    /// Reports whether a global logical point falls inside the monitor.
    ///
    /// The rectangle is half-open: the right and bottom edges belong to the
    /// neighbouring monitor, so overlapping edges never pick two monitors.
    ///
    /// # Arguments
    ///
    /// * `point` - Global logical `(x, y)` coordinate.
    ///
    /// # Returns
    ///
    /// `true` when the point is inside this monitor's rectangle.
    #[must_use]
    pub const fn contains(&self, point: (i32, i32)) -> bool {
        point.0 >= self.x
            && point.0 < self.x.saturating_add(self.width)
            && point.1 >= self.y
            && point.1 < self.y.saturating_add(self.height)
    }
}

/// Finds the monitor whose logical geometry contains a global anchor.
///
/// # Arguments
///
/// * `anchor` - Press point in Hyprland global logical coordinates.
/// * `monitors` - Logical geometries of all monitors, in window order.
///
/// # Returns
///
/// The index into `monitors`, or `None` when the anchor is outside every
/// monitor (stale layout, disconnected screen).
///
/// # Examples
///
/// ```
/// use logi_mx_overlay::scene::{MonitorGeom, anchor_monitor};
///
/// let monitors = [
///     MonitorGeom {
///         x:      0,
///         y:      0,
///         width:  1920,
///         height: 1080,
///         scale:  2
///     },
///     MonitorGeom {
///         x:      1920,
///         y:      0,
///         width:  2560,
///         height: 1440,
///         scale:  1
///     }
/// ];
/// assert_eq!(anchor_monitor((100, 20), &monitors), Some(0));
/// assert_eq!(anchor_monitor((2000, 20), &monitors), Some(1));
/// assert_eq!(anchor_monitor((-1, 20), &monitors), None);
/// ```
#[must_use]
pub fn anchor_monitor(anchor: (i32, i32), monitors: &[MonitorGeom]) -> Option<usize> {
    monitors.iter().position(|monitor| monitor.contains(anchor))
}

/// Converts a global logical anchor into monitor-local logical coordinates.
///
/// The daemon's anchor and GDK's monitor origins share Hyprland's logical
/// space, so the conversion is a plain subtraction of the monitor origin.
///
/// # Arguments
///
/// * `anchor` - Press point in Hyprland global logical coordinates.
/// * `monitor` - The monitor that contains the anchor.
///
/// # Returns
///
/// The anchor in logical pixels relative to the monitor's top-left corner.
///
/// # Examples
///
/// ```
/// use logi_mx_overlay::scene::{MonitorGeom, logical_anchor};
///
/// let monitor = MonitorGeom {
///     x:      0,
///     y:      0,
///     width:  1920,
///     height: 1080,
///     scale:  2
/// };
/// assert_eq!(logical_anchor((100, 200), &monitor), (100.0, 200.0));
/// ```
#[must_use]
pub fn logical_anchor(anchor: (i32, i32), monitor: &MonitorGeom) -> (f64, f64) {
    (
        f64::from(anchor.0.saturating_sub(monitor.x)),
        f64::from(anchor.1.saturating_sub(monitor.y))
    )
}

/// Cuts a sector label to at most `max_chars` characters, appending `…`
/// when anything was dropped.
///
/// # Arguments
///
/// * `label` - Sector label streamed by the daemon.
/// * `max_chars` - Maximum number of characters in the result.
///
/// # Returns
///
/// The label unchanged when it fits, a truncated copy with a trailing
/// ellipsis otherwise, or an empty string when `max_chars` is zero.
///
/// # Examples
///
/// ```
/// use logi_mx_overlay::scene::truncate_label;
///
/// assert_eq!(truncate_label("DownRight", 12), "DownRight");
/// assert_eq!(truncate_label("DownRight", 6), "DownR…");
/// assert_eq!(truncate_label("Up", 0), "");
/// ```
#[must_use]
pub fn truncate_label(label: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    if label.chars().count() <= max_chars {
        return label.to_string();
    }
    let mut truncated: String = label.chars().take(max_chars - 1).collect();
    truncated.push('…');
    truncated
}

/// Everything the draw callbacks paint, folded from the daemon event stream.
///
/// The overlay is a passive renderer: [`Self::apply`] mirrors the daemon
/// state machine (`Charge` → `Open`? → `Highlight`* → `Close`) and nothing
/// else can make the ring appear.
///
/// # Examples
///
/// ```
/// use logi_mx_daemon::pie::PieEvent;
/// use logi_mx_overlay::scene::{MonitorGeom, PieScene};
///
/// let monitors = [MonitorGeom {
///     x:      0,
///     y:      0,
///     width:  1920,
///     height: 1080,
///     scale:  1
/// }];
/// let mut scene = PieScene::default();
/// assert!(!scene.is_active());
/// scene.apply(
///     &PieEvent::Close {
///         committed: None
///     },
///     &monitors
/// );
/// assert!(!scene.is_active(), "a Close without a charge stays inert");
/// ```
#[derive(Debug, Clone)]
pub struct PieScene {
    /// Whether a charge ring or a pie is currently painted on some monitor.
    pub active:    bool,
    /// Whether the thin hold-gate ring is charging instead of the menu.
    pub charging:  bool,
    /// Index of the monitor carrying the anchor.
    pub monitor:   usize,
    /// Anchor in logical pixels relative to that monitor.
    pub anchor:    (f64, f64),
    /// Sectors advertised when the pie opened, in wedge order.
    pub sectors:   Vec<PieSector>,
    /// Selected wedge index, `None` while nothing is highlighted.
    pub highlight: Option<usize>,
    /// Exact cursor aim in radians, last streamed by the daemon.
    pub aim:       Option<f64>,
    /// Scale factor of the monitor carrying the pie, for crisp icon decode.
    pub scale:     i32
}

impl Default for PieScene {
    fn default() -> Self {
        Self {
            active:    false,
            charging:  false,
            monitor:   0,
            anchor:    (0.0, 0.0),
            sectors:   Vec::new(),
            highlight: None,
            aim:       None,
            scale:     1
        }
    }
}

impl PieScene {
    /// Whether an open event describes the currently active menu.
    #[must_use]
    pub fn is_same_open(&self, event: &PieEvent, monitors: &[MonitorGeom]) -> bool {
        let PieEvent::Open {
            anchor,
            sectors
        } = event
        else {
            return false;
        };
        self.active
            && !self.charging
            && anchor_monitor(*anchor, monitors).is_some_and(|index| {
                self.monitor == index
                    && self.anchor == logical_anchor(*anchor, &monitors[index])
                    && self.sectors == *sectors
            })
    }

    /// Folds one daemon event into the scene.
    ///
    /// `Charge` activates the thin ring on the monitor containing the
    /// anchor; `Open` replaces the charge with the full menu and resets the
    /// highlight for a new menu. Repeated identical opens preserve it.
    /// An anchor outside every monitor keeps the scene inactive.
    /// `Highlight` only moves the highlight while a menu is open, and
    /// `Close` always clears everything.
    ///
    /// # Arguments
    ///
    /// * `event` - Event decoded from the daemon stream.
    /// * `monitors` - Logical geometries used to map the anchor.
    pub fn apply(&mut self, event: &PieEvent, monitors: &[MonitorGeom]) {
        if self.is_same_open(event, monitors) {
            return;
        }
        match event {
            PieEvent::Charge {
                anchor
            } => match anchor_monitor(*anchor, monitors) {
                Some(index) => {
                    self.reset();
                    self.active = true;
                    self.charging = true;
                    self.monitor = index;
                    self.anchor = logical_anchor(*anchor, &monitors[index]);
                    self.scale = monitors[index].scale.max(1);
                }
                None => self.reset()
            },
            PieEvent::Open {
                anchor,
                sectors
            } => match anchor_monitor(*anchor, monitors) {
                Some(index) => {
                    self.reset();
                    self.active = true;
                    self.monitor = index;
                    self.anchor = logical_anchor(*anchor, &monitors[index]);
                    self.scale = monitors[index].scale.max(1);
                    self.sectors.clone_from(sectors);
                }
                None => self.reset()
            },
            PieEvent::Highlight {
                index
            } => {
                if self.active && !self.charging {
                    self.highlight = index.map(usize::from);
                }
            }
            PieEvent::Aim {
                angle
            } => {
                if self.active && !self.charging {
                    self.aim = Some(*angle);
                }
            }
            PieEvent::Close {
                ..
            } => self.reset()
        }
    }

    /// Clears the scene back to the idle state.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Whether anything should be painted at all.
    #[must_use]
    pub const fn is_active(&self) -> bool {
        self.active
    }
}

#[cfg(test)]
#[path = "scene_tests.rs"]
mod scene_tests;
