// SPDX-FileCopyrightText: 2025 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

pub mod schema;

use std::path::{Path, PathBuf};

use masterror::prelude::*;
pub use schema::*;
use tracing::{debug, info};

use crate::error::Result;

const DEFAULT_CONFIG_NAME: &str = "logi-mx.toml";

pub fn get_config_path() -> Result<PathBuf> {
    if let Ok(config_home) = std::env::var("XDG_CONFIG_HOME") {
        return Ok(PathBuf::from(config_home).join(DEFAULT_CONFIG_NAME));
    }

    if let Ok(home) = std::env::var("HOME") {
        return Ok(PathBuf::from(home)
            .join(".config")
            .join(DEFAULT_CONFIG_NAME));
    }

    Err(AppError::internal("Cannot determine config directory"))
}

pub fn load_config() -> Result<Config> {
    let path = get_config_path()?;

    if !path.exists() {
        info!("Config file not found, creating default: {:?}", path);
        let config = Config::default();
        save_config(&config)?;
        return Ok(config);
    }

    load_config_from_path(&path)
}

pub fn load_config_from_path(path: &Path) -> Result<Config> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| AppError::internal("Failed to read config file").with_source(e))?;

    let config: Config = toml::from_str(&content)
        .map_err(|e| AppError::bad_request("Invalid config format").with_source(e))?;

    debug!("Loaded config from {:?}", path);
    Ok(config)
}

pub fn save_config(config: &Config) -> Result<()> {
    let path = get_config_path()?;

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
    fn test_get_config_path_with_xdg() {
        let orig_xdg = env::var("XDG_CONFIG_HOME").ok();
        let orig_home = env::var("HOME").ok();

        unsafe {
            env::set_var("XDG_CONFIG_HOME", "/tmp/test_xdg");
        }
        let path = get_config_path().unwrap();
        assert_eq!(path, PathBuf::from("/tmp/test_xdg/logi-mx.toml"));

        unsafe {
            if let Some(val) = orig_xdg {
                env::set_var("XDG_CONFIG_HOME", val);
            } else {
                env::remove_var("XDG_CONFIG_HOME");
            }
            if let Some(val) = orig_home {
                env::set_var("HOME", val);
            }
        }
    }

    #[test]
    fn test_get_config_path_with_home() {
        let orig_xdg = env::var("XDG_CONFIG_HOME").ok();
        let orig_home = env::var("HOME").ok();

        unsafe {
            env::remove_var("XDG_CONFIG_HOME");
            env::set_var("HOME", "/tmp/test_home");
        }
        let path = get_config_path().unwrap();
        assert_eq!(path, PathBuf::from("/tmp/test_home/.config/logi-mx.toml"));

        unsafe {
            if let Some(val) = orig_xdg {
                env::set_var("XDG_CONFIG_HOME", val);
            }
            if let Some(val) = orig_home {
                env::set_var("HOME", val);
            } else {
                env::remove_var("HOME");
            }
        }
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
        let orig_xdg = env::var("XDG_CONFIG_HOME").ok();
        let orig_home = env::var("HOME").ok();

        unsafe {
            env::remove_var("XDG_CONFIG_HOME");
            env::remove_var("HOME");
        }
        let result = get_config_path();
        assert!(result.is_err());

        unsafe {
            if let Some(val) = orig_xdg {
                env::set_var("XDG_CONFIG_HOME", val);
            }
            if let Some(val) = orig_home {
                env::set_var("HOME", val);
            }
        }
    }
}
