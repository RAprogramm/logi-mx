// SPDX-FileCopyrightText: 2025-2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Backdrop treatment: transparent, dimmed and compositor-blur modes with
//! their persisted preference and CSS class application.

use std::{fs, path::PathBuf};

use libadwaita::{ApplicationWindow, prelude::*};

/// Backdrop treatment behind the HUD content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum BgMode {
    /// Fully transparent backdrop.
    None,
    /// Semi-transparent dark fill.
    #[default]
    Dim,
    /// Compositor blur behind a barely tinted fill.
    Blur,
    /// Compositor blur behind a dark fill.
    BlurDim
}

impl BgMode {
    /// Every mode, in token order.
    const ALL: [Self; 4] = [Self::None, Self::Dim, Self::Blur, Self::BlurDim];

    /// The CSS class painting the window backdrop. The translucent fill is
    /// also the medium the compositor blur shows through, so blur-only mode
    /// tints the window faintly.
    pub(super) const fn window_class(self) -> Option<&'static str> {
        match self {
            Self::None => None,
            Self::Blur => Some("hud-tint"),
            Self::Dim | Self::BlurDim => Some("hud-dim")
        }
    }

    pub(super) const fn wants_blur(self) -> bool {
        matches!(self, Self::Blur | Self::BlurDim)
    }

    /// The same backdrop with compositor blur dropped, used when the runtime
    /// GTK cannot request compositor blur regions.
    pub(super) const fn without_blur(self) -> Self {
        match self {
            Self::Blur => Self::None,
            Self::BlurDim => Self::Dim,
            other => other
        }
    }

    pub(super) const fn dim_on(self) -> bool {
        matches!(self, Self::Dim | Self::BlurDim)
    }

    /// Combines the two independent backdrop checkboxes into a mode.
    pub(super) const fn from_bools(dim: bool, blur: bool) -> Self {
        match (dim, blur) {
            (false, false) => Self::None,
            (true, false) => Self::Dim,
            (false, true) => Self::Blur,
            (true, true) => Self::BlurDim
        }
    }

    pub(super) const fn token(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Dim => "dim",
            Self::Blur => "blur",
            Self::BlurDim => "blur-dim"
        }
    }

    pub(super) fn from_token(token: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|mode| mode.token() == token)
            .unwrap_or_default()
    }
}

/// Path of the persisted backdrop mode, when a config home is available.
fn background_path() -> Option<PathBuf> {
    let base = std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|_| std::env::var("HOME").map(|home| PathBuf::from(home).join(".config")))
        .ok()?;
    Some(base.join("logi-mx").join("hud-background"))
}

/// Whether the runtime GTK translates the HUD's `backdrop-filter` into a
/// real compositor blur region (the `ext-background-effect` protocol added
/// in GTK 4.24).
pub(super) fn blur_supported() -> bool {
    gtk4::check_version(4, 24, 0).is_none()
}

/// Reads the persisted backdrop mode, defaulting to [`BgMode::Dim`].
///
/// Blur modes fall back to their unblurred equivalents when the runtime GTK
/// is older than 4.24, where `backdrop-filter` paints only a faint tint.
pub(super) fn load_background() -> BgMode {
    let mode = background_path()
        .and_then(|path| fs::read_to_string(path).ok())
        .map_or_else(BgMode::default, |token| BgMode::from_token(token.trim()));
    if blur_supported() {
        mode
    } else {
        mode.without_blur()
    }
}

/// Persists the backdrop mode; failures are logged, never fatal.
pub(super) fn save_background(mode: BgMode) {
    let Some(path) = background_path() else {
        return;
    };
    if let Some(dir) = path.parent()
        && let Err(e) = fs::create_dir_all(dir)
    {
        tracing::warn!("Failed to create the config directory: {e}");
        return;
    }
    if let Err(e) = fs::write(&path, mode.token()) {
        tracing::warn!("Failed to save the background mode: {e}");
    }
}

/// Paints the window backdrop for the mode through CSS classes.
///
/// The blur class carries `backdrop-filter`, which GTK 4.24 translates into an
/// `ext-background-effect-v1` blur region for the compositor; no compositor
/// rules are involved.
pub(super) fn apply_bg_classes(window: &ApplicationWindow, mode: BgMode) {
    window.remove_css_class("hud-dim");
    window.remove_css_class("hud-tint");
    window.remove_css_class("hud-blur");
    if let Some(class) = mode.window_class() {
        window.add_css_class(class);
    }
    if mode.wants_blur() {
        window.add_css_class("hud-blur");
    }
}

#[cfg(test)]
#[path = "background_tests.rs"]
mod tests;
