// Configuration management for OneDrive sync will go here. 

use anyhow::{Result, anyhow};
use serde::{Serialize, Deserialize};
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Settings {
    /// List of OneDrive folders to sync
    pub sync_folders: Vec<String>,
    // Add more settings as needed
}

#[derive(Debug, Serialize, Deserialize)]
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
            sync_interval: Duration::from_secs(120),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct ChangedQueue {
    /// List of files that have been changed locally and need to be synced
    pub changed_files: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct MetaConfig {
    /// Delta tokens for each sync folder
    pub delta_tokens: std::collections::HashMap<String, String>,
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

impl ChangedQueue {
    pub fn queue_path() -> Result<PathBuf> {
        let mut path = dirs::home_dir().ok_or_else(|| anyhow!("Could not determine home directory"))?;
        path.push(".onedrive");
        fs::create_dir_all(&path)?;
        path.push("changedqueue.json");
        Ok(path)
    }

    pub fn load() -> Result<Self> {
        let path = Self::queue_path()?;
        if !path.exists() {
            // Create default changed queue file if it doesn't exist
            let default = Self::default();
            default.save()?;
            return Ok(default);
        }
        let data = fs::read_to_string(&path)?;
        let queue: Self = serde_json::from_str(&data)?;
        Ok(queue)
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::queue_path()?;
        let data = serde_json::to_string_pretty(self)?;
        fs::write(path, data)?;
        Ok(())
    }

    /// Add a file to the changed queue
    pub fn add_file(&mut self, file_path: &str) -> Result<()> {
        if !self.changed_files.contains(&file_path.to_string()) {
            self.changed_files.push(file_path.to_string());
            self.save()?;
        }
        Ok(())
    }

    /// Remove a file from the changed queue
    pub fn remove_file(&mut self, file_path: &str) -> Result<()> {
        if let Some(pos) = self.changed_files.iter().position(|f| f == file_path) {
            self.changed_files.remove(pos);
            self.save()?;
        }
        Ok(())
    }

    /// Get all changed files
    pub fn get_changed_files(&self) -> &Vec<String> {
        &self.changed_files
    }

    /// Clear all changed files
    pub fn clear(&mut self) -> Result<()> {
        self.changed_files.clear();
        self.save()?;
        Ok(())
    }
}

impl MetaConfig {
    pub fn meta_path() -> Result<PathBuf> {
        let mut path = dirs::home_dir().ok_or_else(|| anyhow!("Could not determine home directory"))?;
        path.push(".onedrive");
        fs::create_dir_all(&path)?;
        path.push("meta.json");
        Ok(path)
    }

    pub fn load() -> Result<Self> {
        let path = Self::meta_path()?;
        if !path.exists() {
            // Create default meta file if it doesn't exist
            let default = Self::default();
            default.save()?;
            return Ok(default);
        }
        let data = fs::read_to_string(&path)?;
        let meta: Self = serde_json::from_str(&data)?;
        Ok(meta)
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::meta_path()?;
        let data = serde_json::to_string_pretty(self)?;
        fs::write(path, data)?;
        Ok(())
    }

    /// Get delta token for a specific folder
    pub fn get_delta_token(&self, folder: &str) -> Option<&String> {
        self.delta_tokens.get(folder)
    }

    /// Set delta token for a specific folder
    pub fn set_delta_token(&mut self, folder: &str, token: String) -> Result<()> {
        self.delta_tokens.insert(folder.to_string(), token);
        self.save()?;
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

    #[test]
    fn test_sync_config_default() {
        let config = SyncConfig::default();
        assert!(config.local_dir.to_string_lossy().contains("OneDrive"));
        assert_eq!(config.remote_dir, "/");
        assert_eq!(config.sync_interval, Duration::from_secs(15));
    }

    #[test]
    fn test_changed_queue_operations() {
        let mut queue = ChangedQueue::default();
        queue.add_file("/test/file1.txt").unwrap();
        queue.add_file("/test/file2.txt").unwrap();
        assert_eq!(queue.get_changed_files().len(), 2);
        
        queue.remove_file("/test/file1.txt").unwrap();
        assert_eq!(queue.get_changed_files().len(), 1);
        assert_eq!(queue.get_changed_files()[0], "/test/file2.txt");
        
        queue.clear().unwrap();
        assert_eq!(queue.get_changed_files().len(), 0);
    }
} 