//! High-level sync service for OneDrive synchronization

use crate::config::{Settings, SyncConfig};
use crate::file_manager::DefaultFileManager;
use crate::metadata_manager_for_files::{MetadataManagerForFiles, get_metadata_manager_singleton};
use crate::onedrive_service::onedrive_client::OneDriveClient;
use crate::sync::delta_sync::DeltaSyncProcessor;
use crate::sync::item_processor::ItemProcessor;
use anyhow::Result;
use log::info;

/// High-level service responsible for orchestrating OneDrive synchronization
pub struct SyncService {
    #[allow(dead_code)]
    pub client: OneDriveClient,
    #[allow(dead_code)]
    pub file_manager: DefaultFileManager,
    #[allow(dead_code)]
    pub config: SyncConfig,
    pub settings: Settings,
    pub metadata_manager: &'static MetadataManagerForFiles,
    delta_processor: DeltaSyncProcessor,
    item_processor: ItemProcessor,
}

impl SyncService {
    /// Create a new sync service
    pub async fn new(
        client: OneDriveClient,
        config: SyncConfig,
        settings: Settings,
    ) -> Result<Self> {
        let metadata_manager = get_metadata_manager_singleton();
        let file_manager = DefaultFileManager::new().await?;
        
        let delta_processor = DeltaSyncProcessor::new(
            client.clone(),
            metadata_manager,
        );
        
        let item_processor = ItemProcessor::new(
            file_manager.clone(),
            metadata_manager,
            client.clone(),
        );

        Ok(Self {
            client,
            file_manager,
            config,
            settings,
            metadata_manager,
            delta_processor,
            item_processor,
        })
    }

    /// Initialize the sync service
    pub async fn init(&mut self) -> Result<()> {
        info!("Initializing OneDrive sync service");
        self.update_delta().await?;
        info!("OneDrive sync service initialized successfully");
        Ok(())
    }


    pub async fn process_delta_items(&mut self) -> Result<()> {
        let delta_items = self.metadata_manager.get_delta_items_to_process()?;
        for item in delta_items {
            self.item_processor.process_item(&item, &self.settings.sync_folders).await?;
        }
        Ok(())
    }

    /// Update the local cache with latest OneDrive changes
    pub async fn update_delta(&mut self) -> Result<()> {
        info!("Starting cache update");
        
        // Process delta changes
        self.delta_processor.get_delta_items_and_update_queue().await?;
        
      
        
        info!("Cache update completed successfully");
        Ok(())
    }

    /// Get the local root path
    #[allow(dead_code)]
    pub fn get_local_root_path(&self) -> std::path::PathBuf {
        self.config.local_dir.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Settings, SyncConfig};

    #[tokio::test]
    async fn test_sync_service_new() {
        let auth = std::sync::Arc::new(crate::auth::onedrive_auth::OneDriveAuth::new().unwrap());
        let client = OneDriveClient::new(auth).unwrap();
        let config = SyncConfig {
            local_dir: "/tmp/test".into(),
            ..Default::default()
        };
        let settings = Settings {
            sync_folders: vec!["Documents".to_string()],
            ..Default::default()
        };
        
        let sync_service = SyncService::new(client, config, settings).await;
        assert!(sync_service.is_ok());
    }

    #[tokio::test]
    async fn test_get_local_root_path() {
        let auth = std::sync::Arc::new(crate::auth::onedrive_auth::OneDriveAuth::new().unwrap());
        let client = OneDriveClient::new(auth).unwrap();
        let config = SyncConfig {
            local_dir: "/tmp/test".into(),
            ..Default::default()
        };
        let settings = Settings::default();
        
        let sync_service = SyncService::new(client, config, settings).await.unwrap();
        let root_path = sync_service.get_local_root_path();
        assert_eq!(root_path, std::path::PathBuf::from("/tmp/test"));
    }
} 