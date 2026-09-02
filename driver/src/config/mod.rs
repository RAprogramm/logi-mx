// SPDX-FileCopyrightText: 2025 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

pub mod schema;

use std::path::{Path, PathBuf};

use masterror::prelude::*;
pub use schema::*;
use tracing::{debug, info};

use crate::error::Result;

const DEFAULT_CONFIG_NAME: &str = "logi-mx.toml";

/// Resolves the configuration file location.
///
/// Honours `XDG_CONFIG_HOME` first, then falls back to `$HOME/.config`.
///
/// # Returns
///
/// Absolute path to `logi-mx.toml` inside the user configuration directory.
///
/// # Errors
///
/// Returns [`masterror::AppError`] when neither `XDG_CONFIG_HOME` nor `HOME`
/// is set, making the configuration directory undeterminable.
///
/// # Examples
///
/// ```no_run
/// use logi_mx_driver::config::config_path;
///
/// let path = config_path()?;
/// println!("config located at {}", path.display());
/// # Ok::<(), masterror::AppError>(())
/// ```
pub fn config_path() -> Result<PathBuf> {
    config_path_from_env(|key| std::env::var(key))
}

/// Resolves the configuration path using a supplied environment reader.
///
/// Keeps [`config_path`] testable without mutating process state.
fn config_path_from_env<F>(env_fn: F) -> Result<PathBuf>
where
    F: Fn(&str) -> std::result::Result<String, std::env::VarError>
{
    if let Ok(config_home) = env_fn("XDG_CONFIG_HOME") {
        return Ok(PathBuf::from(config_home).join(DEFAULT_CONFIG_NAME));
    }

    if let Ok(home) = env_fn("HOME") {
        return Ok(PathBuf::from(home)
            .join(".config")
            .join(DEFAULT_CONFIG_NAME));
    }

    Err(AppError::internal("Cannot determine config directory"))
}

/// Loads configuration from the default location.
///
/// When the file does not exist yet, a default [`Config`] is created and
/// persisted so first run is self-initialising.
///
/// # Returns
///
/// Parsed configuration, or the freshly written default.
///
/// # Errors
///
/// Returns [`masterror::AppError`] when the path cannot be determined
/// (see [`config_path`]), the default cannot be persisted, or the
/// existing file is unreadable or malformed.
///
/// # Examples
///
/// ```no_run
/// use logi_mx_driver::config::load_config;
///
/// let config = load_config()?;
/// println!("{} device(s) configured", config.devices.len());
/// # Ok::<(), masterror::AppError>(())
/// ```
pub fn load_config() -> Result<Config> {
    let path = config_path()?;

    if !path.exists() {
        info!("Config file not found, creating default: {:?}", path);
        let config = Config::default();
        save_config(&config)?;
        return Ok(config);
    }

    load_config_from_path(&path)
}

/// Parses a configuration file from an explicit path.
///
/// # Arguments
///
/// * `path` - TOML file location to read.
///
/// # Returns
///
/// Parsed [`Config`] on success.
///
/// # Errors
///
/// Returns [`masterror::AppError`] when the file cannot be read, or when
/// its content is not valid TOML matching the [`Config`] schema.
///
/// # Examples
///
/// ```no_run
/// use std::path::Path;
///
/// use logi_mx_driver::config::load_config_from_path;
///
/// let config = load_config_from_path(Path::new("/tmp/logi-mx.toml"))?;
/// # Ok::<(), masterror::AppError>(())
/// ```
pub fn load_config_from_path(path: &Path) -> Result<Config> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| AppError::internal("Failed to read config file").with_source(e))?;

    let config: Config = toml::from_str(&content)
        .map_err(|e| AppError::bad_request("Invalid config format").with_source(e))?;

    debug!("Loaded config from {:?}", path);
    Ok(config)
}

/// Persists configuration to the default location.
///
/// Creates the target directory when missing and writes pretty-printed TOML.
///
/// # Arguments
///
/// * `config` - Configuration to serialize.
///
/// # Errors
///
/// Returns [`masterror::AppError`] when the path cannot be determined, the
/// directory cannot be created, serialization fails, or the file cannot be
/// written.
///
/// # Examples
///
/// ```no_run
/// use logi_mx_driver::config::{Config, save_config};
///
/// save_config(&Config::default())?;
/// # Ok::<(), masterror::AppError>(())
/// ```
pub fn save_config(config: &Config) -> Result<()> {
    let path = config_path()?;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| AppError::internal("Failed to create config directory").with_source(e))?;
    }

    let content = toml::to_string_pretty(config)
        .map_err(|e| AppError::internal("Failed to serialize config").with_source(e))?;

    std::fs::write(&path, content)
        .map_err(|e| AppError::internal("Failed to write config file").with_source(e))?;

    info!("Saved config to {:?}", path);
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::env;

    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert!(!config.devices.is_empty());
    }

    #[test]
    fn test_config_serialization() {
        let config = Config::default();
        let toml_str = toml::to_string(&config).unwrap();
        let deserialized: Config = toml::from_str(&toml_str).unwrap();
        assert_eq!(config.devices.len(), deserialized.devices.len());
    }

    #[test]
    fn test_config_path_with_xdg() {
        // Mock environment with XDG_CONFIG_HOME set
        let mock_env = |var: &str| {
            if var == "XDG_CONFIG_HOME" {
                Ok("/tmp/test_xdg".to_string())
            } else {
                Err(env::VarError::NotPresent)
            }
        };
        let path = config_path_from_env(mock_env).unwrap();
        assert_eq!(path, PathBuf::from("/tmp/test_xdg/logi-mx.toml"));
    }

    #[test]
    fn test_config_path_with_home() {
        // Mock environment with only HOME set
        let mock_env = |var: &str| {
            if var == "HOME" {
                Ok("/tmp/test_home".to_string())
            } else {
                Err(env::VarError::NotPresent)
            }
        };
        let path = config_path_from_env(mock_env).unwrap();
        assert_eq!(path, PathBuf::from("/tmp/test_home/.config/logi-mx.toml"));
    }

    #[test]
    fn test_save_and_load_config() {
        let temp_dir = std::env::temp_dir().join("logi-mx-test");
        let config_path = temp_dir.join("test_config.toml");

        let mut config = Config::default();
        config.devices[0].dpi = 2400;

        std::fs::create_dir_all(&temp_dir).unwrap();

        let content = toml::to_string_pretty(&config).unwrap();
        std::fs::write(&config_path, content).unwrap();

        let loaded = load_config_from_path(&config_path).unwrap();
        assert_eq!(loaded.devices[0].dpi, 2400);

        std::fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn test_load_config_invalid_format() {
        let temp_path = std::env::temp_dir().join("invalid_logi_mx.toml");
        std::fs::write(&temp_path, "invalid toml {{{").unwrap();

        let result = load_config_from_path(&temp_path);
        assert!(result.is_err());

        std::fs::remove_file(&temp_path).ok();
    }

    #[test]
    fn test_config_path_no_env() {
        // Mock environment with no variables set
        let mock_env = |_: &str| Err(env::VarError::NotPresent);
        let result = config_path_from_env(mock_env);
        assert!(result.is_err());
    }
}
