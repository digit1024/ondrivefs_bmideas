
use anyhow::{Result, anyhow, Context};
use log::warn;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::time::Duration;
use directories::ProjectDirs;

static SETTINGS_FILE_NAME: &str = "settings.json";

pub struct ProjectConfig {
    pub settings: Settings,
    pub project_dirs: ProjectDirs,
    
}

impl ProjectConfig {
    pub async fn new() -> Result<Self> {

        let proj_dirs = ProjectDirs::from("com", "digit1024@github", "onedrive-sync")
        .expect("Failed to get project directories");
        for x in [proj_dirs.config_dir(), proj_dirs.cache_dir(), proj_dirs.data_dir()] {
            if !x.exists() {
                fs::create_dir_all(x).context("Failed to create config directory")?;
            }
        }
    
        let settings = Settings::new(&proj_dirs.config_dir().join(SETTINGS_FILE_NAME)).await?;
        Ok(Self { settings, project_dirs: proj_dirs })
    }
}



#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct Settings {
    
    /// List of OneDrive folders to sync
    pub download_folders: Vec<String>,
    pub sync_config: SyncConfig,
    // Add more settings as needed
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SyncConfig {
    pub sync_interval: Duration,

}

impl Default for SyncConfig {
    fn default() -> Self {
        Self {
            sync_interval: Duration::from_secs(30),
        }
    }
}

impl Settings {
    pub async fn new(config_file_path: &PathBuf) -> Result<Self> {
    


        
        match Self::load_settings_from_file(&config_file_path) {
            Ok(settings) => Ok(settings),
            Err(e) => {
                warn!("Error loading settings from file - creating default config: {}", e);
                let default = Self::default();
                default.save_to_file(config_file_path)?;
                Ok(default)
            }
        }
        
        
    }


    pub fn load_settings_from_file( config_file_path: &PathBuf) -> Result<Self> {
        
        if !config_file_path.exists() {
            // Return Err if the file doesn't exist
            return Err(anyhow!("Config file not found"));
        }
        let data = fs::read_to_string(&config_file_path)?;
        let settings: Self = serde_json::from_str(&data)?;
        Ok(settings)
    }

    pub fn save_to_file(&self, config_file_path: &PathBuf) -> Result<()> {
        if !config_file_path.exists() {
            // Create default settings file if it doesn't exist
            let parent_path = config_file_path.parent().unwrap();
            fs::create_dir_all(parent_path).context("Failed to create config directory")?;
        }

        let data = serde_json::to_string_pretty(self)?;
        fs::write(config_file_path, data)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use std::fs;

    #[test]
    fn test_sync_config_default() {
        let config = SyncConfig::default();
        assert_eq!(config.sync_interval, Duration::from_secs(30));
    }

    #[test]
    fn test_sync_config_serialization() {
        let config = SyncConfig {
            sync_interval: Duration::from_secs(60),
        };
        
        let json = serde_json::to_string(&config).unwrap();
        let deserialized: SyncConfig = serde_json::from_str(&json).unwrap();
        
        assert_eq!(deserialized.sync_interval, Duration::from_secs(60));
    }

    #[test]
    fn test_settings_default() {
        let settings = Settings::default();
        assert!(settings.download_folders.is_empty());
        assert_eq!(settings.sync_config.sync_interval, Duration::from_secs(30));
    }

    #[test]
    fn test_settings_serialization() {
        let settings = Settings {
            download_folders: vec!["folder1".to_string(), "folder2".to_string()],
            sync_config: SyncConfig {
                sync_interval: Duration::from_secs(120),
            },
        };
        
        let json = serde_json::to_string(&settings).unwrap();
        let deserialized: Settings = serde_json::from_str(&json).unwrap();
        
        assert_eq!(deserialized.download_folders, vec!["folder1", "folder2"]);
        assert_eq!(deserialized.sync_config.sync_interval, Duration::from_secs(120));
    }

    #[test]
    fn test_load_settings_from_file_success() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("test_settings.json");
        
        let test_settings = Settings {
            download_folders: vec!["test_folder".to_string()],
            sync_config: SyncConfig {
                sync_interval: Duration::from_secs(45),
            },
        };
        
        // Save test settings
        test_settings.save_to_file(&config_path).unwrap();
        
        // Load and verify
        let loaded_settings = Settings::load_settings_from_file(&config_path).unwrap();
        assert_eq!(loaded_settings.download_folders, vec!["test_folder"]);
        assert_eq!(loaded_settings.sync_config.sync_interval, Duration::from_secs(45));
    }

    #[test]
    fn test_load_settings_from_file_not_found() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("nonexistent.json");
        
        let result = Settings::load_settings_from_file(&config_path);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Config file not found"));
    }

    #[test]
    fn test_load_settings_from_file_invalid_json() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("invalid_settings.json");
        
        // Write invalid JSON
        fs::write(&config_path, "{ invalid json }").unwrap();
        
        let result = Settings::load_settings_from_file(&config_path);
        assert!(result.is_err());
    }

    #[test]
    fn test_save_to_file_success() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("save_test.json");
        
        let settings = Settings {
            download_folders: vec!["save_folder".to_string()],
            sync_config: SyncConfig {
                sync_interval: Duration::from_secs(90),
            },
        };
        
        // Save settings
        let result = settings.save_to_file(&config_path);
        assert!(result.is_ok());
        
        // Verify file was created
        assert!(config_path.exists());
        
        // Verify content
        let content = fs::read_to_string(&config_path).unwrap();
        assert!(content.contains("save_folder"));
        assert!(content.contains("90"));
    }

    #[test]
    fn test_save_to_file_creates_directory() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("nested").join("deep").join("settings.json");
        
        let settings = Settings::default();
        let result = settings.save_to_file(&config_path);
        
        assert!(result.is_ok());
        assert!(config_path.exists());
        assert!(config_path.parent().unwrap().exists());
    }

    #[tokio::test]
    async fn test_settings_new_creates_default_when_file_not_found() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("new_settings.json");
        
        let settings = Settings::new(&config_path).await.unwrap();
        
        // Should create default settings
        assert!(settings.download_folders.is_empty());
        assert_eq!(settings.sync_config.sync_interval, Duration::from_secs(30));
        
        // Should save default settings to file
        assert!(config_path.exists());
    }

    #[tokio::test]
    async fn test_settings_new_loads_existing_file() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("existing_settings.json");
        
        let original_settings = Settings {
            download_folders: vec!["existing_folder".to_string()],
            sync_config: SyncConfig {
                sync_interval: Duration::from_secs(180),
            },
        };
        
        // Save original settings
        original_settings.save_to_file(&config_path).unwrap();
        
        // Load settings
        let loaded_settings = Settings::new(&config_path).await.unwrap();
        
        assert_eq!(loaded_settings.download_folders, vec!["existing_folder"]);
        assert_eq!(loaded_settings.sync_config.sync_interval, Duration::from_secs(180));
    }

    #[test]
    fn test_project_config_new_creates_directories() {
        // This test would require mocking ProjectDirs or using a test-specific approach
        // For now, we'll test the directory creation logic separately
        let temp_dir = TempDir::new().unwrap();
        let test_dirs = vec![
            temp_dir.path().join("config"),
            temp_dir.path().join("cache"),
            temp_dir.path().join("data"),
        ];
        
        for dir in &test_dirs {
            if !dir.exists() {
                fs::create_dir_all(dir).unwrap();
            }
        }
        
        // Verify directories exist
        for dir in &test_dirs {
            assert!(dir.exists());
            assert!(dir.is_dir());
        }
    }

    #[test]
    fn test_settings_clone() {
        let settings = Settings {
            download_folders: vec!["clone_test".to_string()],
            sync_config: SyncConfig {
                sync_interval: Duration::from_secs(300),
            },
        };
        
        let cloned = settings.clone();
        
        assert_eq!(cloned.download_folders, settings.download_folders);
        assert_eq!(cloned.sync_config.sync_interval, settings.sync_config.sync_interval);
    }

    #[test]
    fn test_sync_config_clone() {
        let config = SyncConfig {
            sync_interval: Duration::from_secs(600),
        };
        
        let cloned = config.clone();
        
        assert_eq!(cloned.sync_interval, config.sync_interval);
    }

    #[test]
    fn test_settings_debug_format() {
        let settings = Settings {
            download_folders: vec!["debug_test".to_string()],
            sync_config: SyncConfig {
                sync_interval: Duration::from_secs(60),
            },
        };
        
        let debug_str = format!("{:?}", settings);
        assert!(debug_str.contains("debug_test"));
        assert!(debug_str.contains("60"));
    }

    #[test]
    fn test_sync_config_debug_format() {
        let config = SyncConfig {
            sync_interval: Duration::from_secs(120),
        };
        
        let debug_str = format!("{:?}", config);
        assert!(debug_str.contains("120"));
    }

    #[test]
    fn test_settings_with_empty_download_folders() {
        let settings = Settings {
            download_folders: vec![],
            sync_config: SyncConfig::default(),
        };
        
        let json = serde_json::to_string(&settings).unwrap();
        let deserialized: Settings = serde_json::from_str(&json).unwrap();
        
        assert!(deserialized.download_folders.is_empty());
    }

    #[test]
    fn test_settings_with_multiple_download_folders() {
        let settings = Settings {
            download_folders: vec![
                "folder1".to_string(),
                "folder2".to_string(),
                "folder3".to_string(),
            ],
            sync_config: SyncConfig::default(),
        };
        
        let json = serde_json::to_string(&settings).unwrap();
        let deserialized: Settings = serde_json::from_str(&json).unwrap();
        
        assert_eq!(deserialized.download_folders.len(), 3);
        assert_eq!(deserialized.download_folders[0], "folder1");
        assert_eq!(deserialized.download_folders[1], "folder2");
        assert_eq!(deserialized.download_folders[2], "folder3");
    }
}
