// SPDX-FileCopyrightText: 2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Pure renderer state: monitor mapping, octant geometry and the scene the
//! draw callbacks paint.
//!
//! The daemon streams the press anchor in Hyprland global coordinates, which
//! are physical pixels, while GTK paints in logical pixels per monitor. This
//! module converts between the two and keeps the small state machine the
//! overlay paints: nothing here touches GTK, sockets or the clock, so the
//! whole mapping is unit-testable headless.

use logi_mx_daemon::pie::{PieEvent, PieSector};
use logi_mx_driver::devices::GestureDirection;

/// Physical geometry of one monitor, in Hyprland global pixels.
///
/// `x` and `y` are the monitor's origin in the global physical layout,
/// `width` and `height` its physical size and `scale` the integer scale
/// factor (at least 1) GDK reports for it.
///
/// # Examples
///
/// ```
/// use logi_mx_overlay::scene::MonitorGeom;
///
/// let monitor = MonitorGeom {
///     x:      0,
///     y:      0,
///     width:  3840,
///     height: 2160,
///     scale:  2
/// };
/// assert!(monitor.contains((100, 200)));
/// assert!(!monitor.contains((3840, 0)));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MonitorGeom {
    /// Global physical x of the monitor's top-left corner.
    pub x:      i32,
    /// Global physical y of the monitor's top-left corner.
    pub y:      i32,
    /// Physical width in pixels.
    pub width:  i32,
    /// Physical height in pixels.
    pub height: i32,
    /// Integer scale factor, at least 1.
    pub scale:  i32
}

impl MonitorGeom {
    /// Reports whether a global physical point falls inside the monitor.
    ///
    /// The rectangle is half-open: the right and bottom edges belong to the
    /// neighbouring monitor, so overlapping edges never pick two monitors.
    ///
    /// # Arguments
    ///
    /// * `point` - Global physical `(x, y)` coordinate.
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

/// Finds the monitor whose physical geometry contains a global anchor.
///
/// # Arguments
///
/// * `anchor` - Press point in Hyprland global physical coordinates.
/// * `monitors` - Physical geometries of all monitors, in window order.
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
///         width:  3840,
///         height: 2160,
///         scale:  2
///     },
///     MonitorGeom {
///         x:      3840,
///         y:      0,
///         width:  2560,
///         height: 1440,
///         scale:  1
///     }
/// ];
/// assert_eq!(anchor_monitor((100, 20), &monitors), Some(0));
/// assert_eq!(anchor_monitor((4000, 20), &monitors), Some(1));
/// assert_eq!(anchor_monitor((-1, 20), &monitors), None);
/// ```
#[must_use]
pub fn anchor_monitor(anchor: (i32, i32), monitors: &[MonitorGeom]) -> Option<usize> {
    monitors.iter().position(|monitor| monitor.contains(anchor))
}

/// Converts a global physical anchor into monitor-local logical coordinates.
///
/// GTK paints in logical pixels, so the overlay divides the offset from the
/// monitor origin by the monitor scale factor.
///
/// # Arguments
///
/// * `anchor` - Press point in Hyprland global physical coordinates.
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
///     width:  3840,
///     height: 2160,
///     scale:  2
/// };
/// assert_eq!(logical_anchor((100, 200), &monitor), (50.0, 100.0));
/// ```
#[must_use]
pub fn logical_anchor(anchor: (i32, i32), monitor: &MonitorGeom) -> (f64, f64) {
    let scale = f64::from(monitor.scale.max(1));
    (
        f64::from(anchor.0.saturating_sub(monitor.x)) / scale,
        f64::from(anchor.1.saturating_sub(monitor.y)) / scale
    )
}

/// Full angle of one octant in radians (45 degrees).
const OCTANT: f64 = std::f64::consts::FRAC_PI_4;
/// Half an octant in radians (22.5 degrees), the cardinal half-width.
const HALF_OCTANT: f64 = std::f64::consts::FRAC_PI_8;

/// The wedge `(start, end)` angles of a direction in cairo radians, or
/// `None` for [`GestureDirection::None`].
///
/// Shared contract with the daemon: the angles reproduce the octants of
/// [`logi_mx_daemon::pie::sector_for`] exactly, in screen coordinates where
/// y grows downward — `Right` spans −22.5°..=22.5° around +x, then clockwise
/// `DownRight`, `Down`, `DownLeft`, `Left`, `UpLeft`, `Up`, `UpRight`. Cairo
/// angles use the same convention (0 along +x, increasing toward +y, which
/// is downward on screen), so wedge `i` spans `i × 45° ± 22.5°` in that
/// order. A vector on a wedge boundary belongs to the cardinal sector, which
/// only matters for painting edges and is therefore shared by both wedges.
///
/// # Arguments
///
/// * `direction` - Sector direction as advertised by the daemon.
///
/// # Returns
///
/// `(start, end)` in cairo radians with `end − start == 45°`, or `None` for
/// the non-directional click binding.
///
/// # Examples
///
/// ```
/// use logi_mx_driver::devices::GestureDirection;
/// use logi_mx_overlay::scene::wedge_for;
///
/// if let Some((start, end)) = wedge_for(GestureDirection::Right) {
///     assert!(start < 0.0 && end > 0.0);
/// }
/// assert!(wedge_for(GestureDirection::None).is_none());
/// ```
#[must_use]
pub fn wedge_for(direction: GestureDirection) -> Option<(f64, f64)> {
    let index: f64 = match direction {
        GestureDirection::Right => 0.0,
        GestureDirection::DownRight => 1.0,
        GestureDirection::Down => 2.0,
        GestureDirection::DownLeft => 3.0,
        GestureDirection::Left => 4.0,
        GestureDirection::UpLeft => 5.0,
        GestureDirection::Up => 6.0,
        GestureDirection::UpRight => 7.0,
        GestureDirection::None => return None
    };
    let middle = index * OCTANT;
    Some((middle - HALF_OCTANT, middle + HALF_OCTANT))
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
/// state machine (`Open` → `Highlight`* → `Close`) and nothing else can make
/// the ring appear.
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
/// assert!(!scene.is_active(), "a Close without an Open stays inert");
/// ```
#[derive(Debug, Default, Clone)]
pub struct PieScene {
    /// Whether a pie is currently open on some monitor.
    pub active:    bool,
    /// Index of the monitor carrying the anchor.
    pub monitor:   usize,
    /// Anchor in logical pixels relative to that monitor.
    pub anchor:    (f64, f64),
    /// Bound sectors advertised when the pie opened.
    pub sectors:   Vec<PieSector>,
    /// Sector under the cursor, `None` inside the dead zone.
    pub highlight: Option<GestureDirection>
}

impl PieScene {
    /// Folds one daemon event into the scene.
    ///
    /// `Open` activates the scene on the monitor containing the anchor and
    /// resets the highlight; an anchor outside every monitor keeps the scene
    /// inactive. `Highlight` only moves the highlight while active, and
    /// `Close` always clears everything.
    ///
    /// # Arguments
    ///
    /// * `event` - Event decoded from the daemon stream.
    /// * `monitors` - Physical geometries used to map the anchor.
    pub fn apply(&mut self, event: &PieEvent, monitors: &[MonitorGeom]) {
        match event {
            PieEvent::Open {
                anchor,
                sectors
            } => match anchor_monitor(*anchor, monitors) {
                Some(index) => {
                    self.active = true;
                    self.monitor = index;
                    self.anchor = logical_anchor(*anchor, &monitors[index]);
                    self.sectors.clone_from(sectors);
                    self.highlight = None;
                }
                None => self.reset()
            },
            PieEvent::Highlight {
                direction
            } => {
                if self.active {
                    self.highlight = *direction;
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

    /// Whether a ring should be painted at all.
    #[must_use]
    pub const fn is_active(&self) -> bool {
        self.active
    }
}

#[cfg(test)]
#[path = "scene_tests.rs"]
mod scene_tests;
