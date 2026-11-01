// SPDX-FileCopyrightText: 2025-2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! One-click Hyprland integration.
//!
//! Detects a running Hyprland session, locates its configuration (Lua for
//! 0.55+, legacy hyprlang otherwise) and appends the integration block when
//! it is missing: window rules that float and center the settings window and
//! a keybind that opens the button-mapping overlay. Existing configuration
//! is never rewritten, only appended, and the marker makes the operation
//! idempotent.

use std::{fs, path::PathBuf, process::Command};

use masterror::prelude::*;

/// Marker identifying the block appended by this module.
const INTEGRATION_MARKER: &str = "logi-mx integration";

/// Marker wrapping the HUD blur rule so it can be added and removed again.
const BLUR_MARKER_BEGIN: &str = "begin logi-mx hud blur";
/// End marker of the HUD blur rule block.
const BLUR_MARKER_END: &str = "end logi-mx hud blur";

/// Lua rule blurring the desktop behind the HUD layer (Hyprland 0.55+).
const LUA_BLUR_BLOCK: &str = "-- begin logi-mx hud blur
hl.layer_rule({
    name = \"logi-mx-hud-blur\",
    match = { namespace = \"^logi-mx-hud$\" },
    blur = true,
    ignore_alpha = 0.0,
})
-- end logi-mx hud blur
";

/// Legacy hyprlang blur rule for pre-Lua configurations.
const CONF_BLUR_BLOCK: &str = "# begin logi-mx hud blur
layerrule = blur, logi-mx-hud
layerrule = ignorealpha 0.0, logi-mx-hud
# end logi-mx hud blur
";

/// Lua snippet for Hyprland 0.55 and newer.
const LUA_BLOCK: &str = "
-- logi-mx integration
hl.window_rule({
    name  = \"logi-mx-ui\",
    match = { title = \"^Logitech MX Master 3S$\" },
    float = true,
    center = true,
})

hl.bind(\"SUPER + M\", hl.dsp.exec_cmd(\"logi-mx-ui\"))
";

/// Legacy hyprlang snippet for pre-Lua configurations.
const CONF_BLOCK: &str = "
# logi-mx integration
windowrulev2 = float, title:^(Logitech MX Master 3S)$
windowrulev2 = center, title:^(Logitech MX Master 3S)$
bind = SUPER, M, exec, logi-mx-ui
";

/// Outcome of an integration run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The block was missing; the config was appended and Hyprland reloaded.
    Configured {
        /// Display path of the updated config file.
        path: String,
    },
    /// The config already contains the integration block.
    AlreadyConfigured {
        /// Display path of the config file.
        path: String,
    },
    /// The session is not running under Hyprland.
    NotHyprland,
    /// Hyprland is running but no config file was found.
    NoConfig,
}

/// Reports whether the session runs under Hyprland.
#[must_use]
pub fn is_hyprland() -> bool {
    std::env::var("HYPRLAND_INSTANCE_SIGNATURE").is_ok_and(|value| !value.is_empty())
}

/// Reports whether a config body already carries the integration block.
#[must_use]
pub fn contains_integration(content: &str) -> bool {
    content.contains(INTEGRATION_MARKER)
}

/// Ensures the integration block exists in the active Hyprland config.
///
/// Appends the block when missing and reloads Hyprland afterwards. Existing
/// configuration is preserved verbatim.
///
/// # Errors
///
/// Returns an internal error when the config cannot be read or written, or
/// when `hyprctl reload` fails.
pub fn ensure_integration() -> Result<Outcome, AppError> {
    if !is_hyprland() {
        return Ok(Outcome::NotHyprland);
    }

    let Some(dir) = config_dir() else {
        return Ok(Outcome::NoConfig);
    };

    let lua_path = dir.join("hyprland.lua");
    let conf_path = dir.join("hyprland.conf");
    let (path, block) = if lua_path.is_file() {
        (lua_path, LUA_BLOCK)
    } else if conf_path.is_file() {
        (conf_path, CONF_BLOCK)
    } else {
        return Ok(Outcome::NoConfig);
    };

    let content = fs::read_to_string(&path)
        .map_err(|e| AppError::internal("Failed to read Hyprland config").with_source(e))?;

    let display = path.display().to_string();
    if contains_integration(&content) {
        return Ok(Outcome::AlreadyConfigured { path: display });
    }

    let mut updated = content;
    if !updated.ends_with('\n') {
        updated.push('\n');
    }
    updated.push_str(block);

    fs::write(&path, updated)
        .map_err(|e| AppError::internal("Failed to write Hyprland config").with_source(e))?;

    reload_hyprland()?;
    Ok(Outcome::Configured { path: display })
}

fn config_dir() -> Option<PathBuf> {
    let base = match std::env::var("XDG_CONFIG_HOME") {
        Ok(value) => PathBuf::from(value),
        Err(_) => PathBuf::from(std::env::var("HOME").ok()?).join(".config"),
    };
    let dir = base.join("hypr");
    dir.is_dir().then_some(dir)
}

/// Resolves the active config file and whether it uses the Lua dialect.
fn config_file() -> Option<(PathBuf, bool)> {
    let dir = config_dir()?;
    let lua_path = dir.join("hyprland.lua");
    if lua_path.is_file() {
        return Some((lua_path, true));
    }
    let conf_path = dir.join("hyprland.conf");
    conf_path.is_file().then_some((conf_path, false))
}

/// Enables or disables compositor blur behind the HUD layer.
///
/// Adds the named layer rule to the active Hyprland config when enabling and
/// strips it again when disabling, reloading Hyprland on any change. The
/// operation is idempotent: a matching state leaves the config untouched.
///
/// # Errors
///
/// Returns an internal error when no Hyprland config file exists, or when
/// reading, writing or reloading fails.
pub fn set_hud_blur(enable: bool) -> Result<(), AppError> {
    let (path, lua) =
        config_file().ok_or_else(|| AppError::internal("Blur needs a Hyprland config file"))?;

    let content = fs::read_to_string(&path)
        .map_err(|e| AppError::internal("Failed to read Hyprland config").with_source(e))?;
    let present = content.contains(BLUR_MARKER_BEGIN);
    if present == enable {
        return Ok(());
    }

    let updated = if enable {
        let block = if lua { LUA_BLUR_BLOCK } else { CONF_BLUR_BLOCK };
        let mut updated = content;
        if !updated.ends_with('\n') {
            updated.push('\n');
        }
        updated.push_str(block);
        updated
    } else {
        strip_blur_block(&content)
    };

    fs::write(&path, &updated)
        .map_err(|e| AppError::internal("Failed to write Hyprland config").with_source(e))?;
    reload_hyprland()
}

/// Removes the marked blur block, including its marker lines.
fn strip_blur_block(content: &str) -> String {
    let mut kept = String::with_capacity(content.len());
    let mut skipping = false;
    for line in content.lines() {
        if line.contains(BLUR_MARKER_BEGIN) {
            skipping = true;
            continue;
        }
        if line.contains(BLUR_MARKER_END) {
            skipping = false;
            continue;
        }
        if !skipping {
            kept.push_str(line);
            kept.push('\n');
        }
    }
    kept
}

fn reload_hyprland() -> Result<(), AppError> {
    let status = Command::new("hyprctl")
        .arg("reload")
        .status()
        .map_err(|e| AppError::internal("Failed to run hyprctl").with_source(e))?;

    if !status.success() {
        return Err(AppError::internal("hyprctl reload failed"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marker_detected_in_lua_and_conf_blocks() {
        assert!(contains_integration(LUA_BLOCK));
        assert!(contains_integration(CONF_BLOCK));
    }

    #[test]
    fn plain_config_is_not_marked() {
        let content = "general {\n  gaps_in = 5\n}\nbind = SUPER, RETURN, exec, kitty\n";
        assert!(!contains_integration(content));
    }

    #[test]
    fn lua_block_declares_rule_and_bind() {
        assert!(LUA_BLOCK.contains("hl.window_rule"));
        assert!(LUA_BLOCK.contains("float = true"));
        assert!(LUA_BLOCK.contains("center = true"));
        assert!(LUA_BLOCK.contains("hl.bind"));
        assert!(LUA_BLOCK.contains("hl.bind(\"SUPER + M\", hl.dsp.exec_cmd(\"logi-mx-ui\"))"));
    }

    #[test]
    fn conf_block_declares_rules_and_bind() {
        assert!(CONF_BLOCK.contains("windowrulev2 = float"));
        assert!(CONF_BLOCK.contains("windowrulev2 = center"));
        assert!(CONF_BLOCK.contains("bind = SUPER, M, exec"));
    }

    #[test]
    fn append_preserves_existing_content() {
        let original = "hl.config({ general = { gaps_in = 4 } })";
        let mut updated = original.to_string();
        if !updated.ends_with('\n') {
            updated.push('\n');
        }
        updated.push_str(LUA_BLOCK);

        assert!(updated.starts_with("hl.config("));
        assert!(contains_integration(&updated));
    }

    #[test]
    fn blur_blocks_carry_both_markers() {
        assert!(LUA_BLUR_BLOCK.contains(BLUR_MARKER_BEGIN));
        assert!(LUA_BLUR_BLOCK.contains(BLUR_MARKER_END));
        assert!(CONF_BLUR_BLOCK.contains(BLUR_MARKER_BEGIN));
        assert!(CONF_BLUR_BLOCK.contains(BLUR_MARKER_END));
        assert!(LUA_BLUR_BLOCK.contains("blur = true"));
        assert!(CONF_BLUR_BLOCK.contains("layerrule = blur"));
    }

    #[test]
    fn strip_blur_block_removes_only_the_rule() {
        let original = "general {\n  gaps_in = 5\n}\n";
        let with_rule = format!("{original}{LUA_BLUR_BLOCK}");
        assert!(with_rule.contains("blur"));

        let stripped = strip_blur_block(&with_rule);
        assert!(!stripped.contains("blur"));
        assert!(!stripped.contains(BLUR_MARKER_BEGIN));
        assert!(stripped.contains("gaps_in = 5"));
    }

    #[test]
    fn strip_blur_block_on_clean_config_is_identity() {
        let original = "general {\n  gaps_in = 5\n}\n";
        assert_eq!(strip_blur_block(original), original);
    }
}
