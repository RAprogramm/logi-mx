// SPDX-FileCopyrightText: 2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Application icons for the pie sectors.
//!
//! A sector bound to a shell command shows the icon of the program it
//! launches. The mapping follows the freedesktop desktop-entry spec through
//! GIO: every installed application is indexed once by the basename of its
//! `Exec` executable — the same index GNOME Shell builds — and the entry's
//! advertised `Icon` is kept as either a themed name or an absolute file.
//! The overlay resolves a sector command to that source and decodes it into
//! a cairo surface sized for the monitor scale, caching both so opening the
//! menu never touches the disk twice.

use std::{collections::HashMap, path::PathBuf};

use gio_unix::DesktopAppInfo;
use gtk4::{
    IconLookupFlags, IconTheme, TextDirection,
    cairo::{self, Format, ImageSurface, SurfacePattern},
    gdk_pixbuf::Pixbuf,
    gio::{
        self, Icon,
        prelude::{AppInfoExt, FileExt}
    },
    glib::{self, object::Cast}
};
use logi_mx_daemon::pie::PieSector;

/// Nominal sector icon size in logical pixels.
pub const ICON_SIZE: i32 = 34;
/// Icon size requested from the theme before scaling to the monitor scale.
const THEME_LOOKUP: i32 = 128;

/// Where a sector icon's pixels come from.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum IconSource {
    /// A named icon in the current icon theme, e.g. `org.gnome.Nautilus`.
    Named(String),
    /// An absolute icon file advertised by the desktop entry.
    File(PathBuf)
}

/// The executable name a command line starts with, without any path.
///
/// The line is split with `GLib`'s shell parser — the same unquoting GIO
/// applies to `Exec` keys — so quoted paths resolve like plain commands.
///
/// # Arguments
///
/// * `command` - Raw shell command, e.g. `/usr/bin/thunar --daemon`.
///
/// # Returns
///
/// The basename of the first token, `None` when the command is blank, the
/// token has no name (a bare `/`), or quoting is malformed.
///
/// # Examples
///
/// ```
/// use logi_mx_overlay::icons::exec_key;
///
/// assert_eq!(
///     exec_key("/usr/bin/thunar --daemon").as_deref(),
///     Some("thunar")
/// );
/// assert_eq!(exec_key("firefox/").as_deref(), Some("firefox"));
/// assert_eq!(
///     exec_key("\"/opt/my app/bin/app\" %U").as_deref(),
///     Some("app")
/// );
/// assert_eq!(exec_key("   "), None);
/// ```
#[must_use]
pub fn exec_key(command: &str) -> Option<String> {
    let argv = glib::shell_parse_argv(command).ok()?;
    let first = argv.first()?;
    std::path::Path::new(first)
        .file_name()
        .map(std::ffi::OsStr::to_os_string)
        .map(|name| name.to_string_lossy().into_owned())
}

/// The icon a desktop entry advertises, as a themed name or a file path.
///
/// # Arguments
///
/// * `icon` - The icon reported by [`gio::prelude::AppInfoExt::icon`], if any.
///
/// # Returns
///
/// [`IconSource::Named`] for a themed icon with at least one name,
/// [`IconSource::File`] for a file icon with a local path, `None` for any
/// other icon kind or an unnamed themed icon.
///
/// # Examples
///
/// ```
/// use gtk4::gio;
/// use logi_mx_overlay::icons::{IconSource, icon_source};
///
/// let icon: gio::Icon = gio::ThemedIcon::new("org.gnome.Nautilus").into();
/// assert_eq!(
///     icon_source(Some(&icon)),
///     Some(IconSource::Named("org.gnome.Nautilus".to_string()))
/// );
/// assert_eq!(icon_source(None), None);
/// ```
#[must_use]
pub fn icon_source(icon: Option<&Icon>) -> Option<IconSource> {
    if let Some(themed) = icon.and_then(|icon| icon.downcast_ref::<gio::ThemedIcon>()) {
        return themed
            .names()
            .first()
            .map(|name| IconSource::Named(name.to_string()));
    }
    icon.and_then(|icon| icon.downcast_ref::<gio::FileIcon>())
        .and_then(|file_icon| file_icon.file().path())
        .map(IconSource::File)
}

/// Converts a pixbuf into a cairo `ARgb32` surface: native-endian,
/// premultiplied BGRA words, the exact layout cairo paints from.
///
/// # Arguments
///
/// * `pixbuf` - Decoded icon image, RGB or RGBA at 8 bits per channel.
///
/// # Returns
///
/// The surface sized like the pixbuf, `None` for empty images, unsupported
/// channel counts or depths, and data shorter than its rowstride claims.
///
/// # Examples
///
/// ```
/// use gtk4::gdk_pixbuf::{Colorspace, Pixbuf};
/// use logi_mx_overlay::icons::surface_from_pixbuf;
///
/// let pixbuf = Pixbuf::new(Colorspace::Rgb, true, 8, 2, 1).unwrap();
/// pixbuf.put_pixel(0, 0, 10, 20, 30, 255);
/// pixbuf.put_pixel(1, 0, 40, 50, 60, 255);
/// let surface = surface_from_pixbuf(&pixbuf).unwrap();
/// assert_eq!((surface.width(), surface.height()), (2, 1));
/// ```
#[must_use]
pub fn surface_from_pixbuf(pixbuf: &Pixbuf) -> Option<ImageSurface> {
    let width = pixbuf.width();
    let height = pixbuf.height();
    let channels = usize::try_from(pixbuf.n_channels()).ok()?;
    if width <= 0 || height <= 0 || !(3..=4).contains(&channels) || pixbuf.bits_per_sample() != 8 {
        return None;
    }
    let rowstride = usize::try_from(pixbuf.rowstride()).ok()?;
    let pixels = pixbuf.read_pixel_bytes();
    let has_alpha = pixbuf.has_alpha();
    let rows = usize::try_from(height).ok()?;
    let cols = usize::try_from(width).ok()?;
    let mut data: Vec<u8> = Vec::with_capacity(cols.checked_mul(rows)?.checked_mul(4)?);
    for y in 0..rows {
        for x in 0..cols {
            let at = y
                .checked_mul(rowstride)?
                .checked_add(x.checked_mul(channels)?)?;
            let red = *pixels.get(at)?;
            let green = *pixels.get(at.checked_add(1)?)?;
            let blue = *pixels.get(at.checked_add(2)?)?;
            let alpha = if has_alpha {
                pixels.get(at.checked_add(3)?).copied().unwrap_or(u8::MAX)
            } else {
                u8::MAX
            };
            data.extend_from_slice(&premultiplied(red, green, blue, alpha).to_ne_bytes());
        }
    }
    let stride = width.checked_mul(4)?;
    ImageSurface::create_for_data(data, Format::ARgb32, width, height, stride).ok()
}

/// Packs a pixel into a native-endian `ARgb32` word with premultiplied colour.
fn premultiplied(red: u8, green: u8, blue: u8, alpha: u8) -> u32 {
    let scale = u16::from(alpha);
    let red = u8::try_from(u16::from(red) * scale / 255).unwrap_or(0);
    let green = u8::try_from(u16::from(green) * scale / 255).unwrap_or(0);
    let blue = u8::try_from(u16::from(blue) * scale / 255).unwrap_or(0);
    u32::from(alpha) << 24 | u32::from(red) << 16 | u32::from(green) << 8 | u32::from(blue)
}

/// Paints an icon surface centred at `(cx, cy)` logical pixels.
///
/// The surface holds `logical size × scale` device pixels; the context is
/// temporarily unscaled so the icon lands crisp on any monitor scale.
///
/// # Arguments
///
/// * `cr` - Cairo context already carrying the menu's bloom transform.
/// * `surface` - Decoded icon from [`IconCache::surface`].
/// * `cx`, `cy` - Centre in logical pixels.
/// * `scale` - Monitor scale factor of the painted surface.
pub fn paint_icon(cr: &cairo::Context, surface: &ImageSurface, cx: f64, cy: f64, scale: f64) {
    let scale = scale.max(1.0);
    let side = f64::from(surface.width()) / scale;
    let _ = cr.save();
    cr.translate(cx - side / 2.0, cy - side / 2.0);
    cr.scale(1.0 / scale, 1.0 / scale);
    let pattern = SurfacePattern::create(surface);
    let _ = cr.set_source(&pattern);
    let _ = cr.paint();
    let _ = cr.restore();
}

/// Resolved sector icons, cached across menu openings.
#[derive(Default)]
pub struct IconCache {
    apps:     HashMap<String, IconSource>,
    indexed:  bool,
    sources:  HashMap<String, Option<IconSource>>,
    surfaces: HashMap<(String, i32), Option<ImageSurface>>
}

impl IconCache {
    /// An empty cache; the application index is built on first use.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Indexes the installed applications once, up front, so the first menu
    /// opening only decodes icon files. Repeated calls are free.
    pub fn warm_index(&mut self) {
        if self.indexed {
            return;
        }
        self.indexed = true;
        for app in gio::AppInfo::all() {
            let Some(desktop) = app.downcast_ref::<DesktopAppInfo>() else {
                continue;
            };
            let Some(exec) = desktop.string(glib::KEY_FILE_DESKTOP_KEY_EXEC) else {
                continue;
            };
            let Some(name) = exec_key(&exec) else {
                continue;
            };
            if self.apps.contains_key(&name) {
                continue;
            }
            if let Some(source) = icon_source(app.icon().as_ref()) {
                self.apps.insert(name, source);
            }
        }
    }

    /// Resolves and decodes the icon of every command sector at `px` device
    /// pixels so painting only looks values up.
    ///
    /// # Arguments
    ///
    /// * `sectors` - Sectors streamed with the opening menu.
    /// * `px` - Icon side in device pixels: [`ICON_SIZE`] times the monitor
    ///   scale.
    pub fn warm(&mut self, sectors: &[PieSector], px: i32) {
        self.warm_index();
        for sector in sectors {
            let Some(command) = sector.command.as_deref().filter(|text| !text.is_empty()) else {
                continue;
            };
            let key = (command.to_string(), px);
            if self.surfaces.contains_key(&key) {
                continue;
            }
            let source = self.resolve(command);
            let surface = source.as_ref().and_then(|source| decode_icon(source, px));
            self.surfaces.insert(key, surface);
        }
    }

    /// The decoded icon surface for `command` at `px` device pixels, `None`
    /// while unknown or unresolved.
    ///
    /// # Arguments
    ///
    /// * `command` - Sector command streamed by the daemon, if any.
    /// * `px` - Icon side in device pixels the caller wants to paint.
    #[must_use]
    pub fn surface(&self, command: Option<&str>, px: i32) -> Option<&ImageSurface> {
        command
            .and_then(|command| self.surfaces.get(&(command.to_string(), px)))
            .and_then(Option::as_ref)
    }

    /// The icon source for `command`, consulting and filling the per-command
    /// cache over the application index.
    fn resolve(&mut self, command: &str) -> Option<IconSource> {
        if let Some(known) = self.sources.get(command) {
            return known.clone();
        }
        let found = exec_key(command).and_then(|key| self.apps.get(&key).cloned());
        self.sources.insert(command.to_string(), found.clone());
        found
    }
}

/// Decodes the icon file behind `source` into a square surface of side `px`.
///
/// # Arguments
///
/// * `source` - Themed name or file path from the desktop entry.
/// * `px` - Requested side in device pixels.
fn decode_icon(source: &IconSource, px: i32) -> Option<ImageSurface> {
    let path = match source {
        IconSource::File(path) => path.clone(),
        IconSource::Named(name) => {
            let theme = IconTheme::default();
            if !theme.has_icon(name) {
                return None;
            }
            let paintable = theme.lookup_icon(
                name,
                &[],
                THEME_LOOKUP,
                1,
                TextDirection::None,
                IconLookupFlags::empty()
            );
            paintable.file()?.path()?
        }
    };
    let pixbuf = Pixbuf::from_file_at_size(path, px, px).ok()?;
    surface_from_pixbuf(&pixbuf)
}

#[cfg(test)]
#[path = "icons_tests.rs"]
mod tests;
