
use anyhow::{Result, anyhow, Context};
use log::warn;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use std::fs;
use std::path::PathBuf;

use directories::ProjectDirs;

static SETTINGS_FILE_NAME: &str = "settings.json";
//APPLICATION FILES ARE STORED UNDER ~/.local/share/onedrive-sync
//logs are under  logs
//downloaded files are under ~/.local/share/onedrive-sync/downloads
//db is under ~/.local/share/onedrive-sync/ondrive.db
//uploads are under ~/.local/share/onedrive-sync/uploads
//secrets are under ~/.config/onedrive-sync/secrets.json
//settings are under ~/.config/onedrive-sync/settings.json


pub struct ProjectConfig {
    pub settings: RwLock<Settings>,
    pub project_dirs: ProjectDirs,
    
}

impl ProjectConfig {
    pub async fn new() -> Result<Self> {

        let proj_dirs = ProjectDirs::from("com", "digit1024@github", "onedrive-sync")
        .expect("Failed to get project directories");
        let d   = proj_dirs.data_dir().join("downloads");
        let u   = proj_dirs.data_dir().join("uploads");
        let l   = proj_dirs.data_dir().join("local");
    
        for x in [proj_dirs.config_dir(), proj_dirs.cache_dir(), proj_dirs.data_dir(), &d.to_path_buf(), &u.to_path_buf() , &l.to_path_buf()] {
            if !x.exists() {
                fs::create_dir_all(x).context("Failed to create config directory")?;
            }
        }
    
        let settings = Settings::new(&proj_dirs.config_dir().join(SETTINGS_FILE_NAME)).await?;
        
        Ok(Self { settings: RwLock::new(settings), project_dirs: proj_dirs })
    }
    
    pub fn download_dir(&self) -> PathBuf { 
        self.project_dirs.data_dir().join("downloads")
    }

    pub fn local_dir(&self) -> PathBuf { 
        self.project_dirs.data_dir().join("local")
    }
    pub fn upload_dir(&self) -> PathBuf {
        self.project_dirs.data_dir().join("uploads")
    }
    
}



#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct Settings {
    /// List of OneDrive folders to sync
    pub download_folders: Vec<String>,
    pub sync_config: SyncConfig,
    /// Conflict resolution strategy
    pub conflict_resolution_strategy: ConflictResolutionStrategy,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SyncConfig {
    pub sync_interval_seconds: u64,
    pub max_retry_count: u32,
    pub enable_notifications: bool,
}

impl Default for SyncConfig {
    fn default() -> Self {
        Self {
            sync_interval_seconds: 30,
            max_retry_count: 3,
            enable_notifications: true,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub enum ConflictResolutionStrategy {
    AlwaysRemote,  // Always favor remote changes
    AlwaysLocal,   // Always favor local changes
    Manual,        // Wait for user decision
}

impl Default for ConflictResolutionStrategy {
    fn default() -> Self {
        ConflictResolutionStrategy::Manual
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
