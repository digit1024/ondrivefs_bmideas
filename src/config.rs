// Configuration management for OneDrive sync will go here.

use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct Settings {
    /// List of OneDrive folders to sync
    pub sync_folders: Vec<String>,
    // Add more settings as needed
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SyncConfig {
    pub local_dir: PathBuf,
    pub remote_dir: String,
    pub sync_interval: Duration,
}

impl Default for SyncConfig {
    fn default() -> Self {
        let mut local_dir = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        local_dir.push("OneDrive");

        Self {
            local_dir,
            remote_dir: "/".to_string(),
            sync_interval: Duration::from_secs(30),
        }
    }
}

impl Settings {
    pub fn get_settings_path() -> Result<PathBuf> {
        let mut path =
            dirs::home_dir().ok_or_else(|| anyhow!("Could not determine home directory"))?;
        println!("path: {:?}", path);

        path.push(".onedrive");
        fs::create_dir_all(&path)?;
        path.push("settings.json");
        Ok(path)
    }

    pub fn load_from_file() -> Result<Self> {
        let path = Self::get_settings_path()?;
        if !path.exists() {
            // Create default settings file if it doesn't exist
            let default = Self::default();
            default.save_to_file()?;
            return Ok(default);
        }
        let data = fs::read_to_string(&path)?;
        let settings: Self = serde_json::from_str(&data)?;
        Ok(settings)
    }

    pub fn save_to_file(&self) -> Result<()> {
        let path = Self::get_settings_path()?;
        let data = serde_json::to_string_pretty(self)?;
        fs::write(path, data)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::env;
    use tempfile::tempdir;

    fn setup_test_env() {
        let temp_dir = tempdir().unwrap();
        unsafe {
            env::set_var("HOME", temp_dir.path());
        }
    }

    #[test]
    #[serial]
    fn test_settings_load_and_save() {
        setup_test_env();
        let mut settings = Settings::default();
        settings.sync_folders = vec!["/Documents".to_string(), "/Pictures".to_string()];

        settings.save_to_file().unwrap();
        let loaded = Settings::load_from_file().unwrap();
        assert_eq!(loaded.sync_folders, settings.sync_folders);
    }

    #[test]
    #[serial]
    fn test_sync_config_default() {
        println!("test_sync_config_default");
        setup_test_env();
        let config = SyncConfig::default();
        println!("config: {:?}", config);
        assert!(config.local_dir.to_string_lossy().contains("OneDrive"));
        assert_eq!(config.remote_dir, "/");
        assert_eq!(config.sync_interval, Duration::from_secs(120));
    }
}
