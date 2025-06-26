// Configuration management for OneDrive sync will go here. 

use anyhow::{Result, anyhow};
use serde::{Serialize, Deserialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Settings {
    /// List of OneDrive folders to sync
    pub sync_folders: Vec<String>,
    // Add more settings as needed
}

impl Settings {
    pub fn config_path() -> Result<PathBuf> {
        let mut path = dirs::home_dir().ok_or_else(|| anyhow!("Could not determine home directory"))?;
        path.push(".onedrive");
        fs::create_dir_all(&path)?;
        path.push("settings.json");
        Ok(path)
    }

    pub fn load() -> Result<Self> {
        let path = Self::config_path()?;
        if !path.exists() {
            // Create default settings file if it doesn't exist
            let default = Self::default();
            default.save()?;
            return Ok(default);
        }
        let data = fs::read_to_string(&path)?;
        let settings: Self = serde_json::from_str(&data)?;
        Ok(settings)
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::config_path()?;
        let data = serde_json::to_string_pretty(self)?;
        fs::write(path, data)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_settings_load_and_save() {
        let mut settings = Settings::default();
        settings.sync_folders = vec!["/Documents".to_string(), "/Pictures".to_string()];
        settings.save().unwrap();
        let loaded = Settings::load().unwrap();
        assert_eq!(loaded.sync_folders, settings.sync_folders);
    }
} 