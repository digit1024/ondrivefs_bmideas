//! High-level sync service for OneDrive synchronization

use crate::config::{Settings, SyncConfig};
use crate::file_manager::DefaultFileManager;
use crate::metadata_manager_for_files::{MetadataManagerForFiles, get_metadata_manager_singleton};
use crate::onedrive_service::onedrive_client::OneDriveClient;
use crate::scheduler::{PeriodicScheduler, PeriodicTask, TaskMetrics};
use crate::sync::delta_sync::DeltaSyncProcessor;
use crate::sync::download_processor::DownloadProcessor;
use crate::sync::item_processor::ItemProcessor;
use anyhow::Result;
use log::info;
use std::collections::HashMap;
use std::time::Duration;

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
    download_processor: DownloadProcessor,
    scheduler: Option<PeriodicScheduler>,
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

        let delta_processor = DeltaSyncProcessor::new(client.clone(), metadata_manager);

        let item_processor =
            ItemProcessor::new(file_manager.clone(), metadata_manager, client.clone());
        
        let download_processor = DownloadProcessor::new(
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
            download_processor,
            scheduler: None,
        })
    }

    /// Initialize the sync service
    pub async fn init(&mut self) -> Result<()> {
        info!("Initializing OneDrive sync service");
        self.update_delta().await?;
        self.process_delta_items().await?;
        info!("OneDrive sync service initialized successfully");
        Ok(())
    }

    pub async fn process_delta_items(&mut self) -> Result<()> {
        let delta_items = self.metadata_manager.get_delta_items_to_process()?;
        for item in delta_items {
            self.item_processor
                .process_item(&item, &self.settings.sync_folders)
                .await?;
        }
        Ok(())
    }

    /// Update the local cache with latest OneDrive changes
    pub async fn update_delta(&mut self) -> Result<()> {
        info!("Starting cache update");

        // Process delta changes
        self.delta_processor
            .get_delta_items_and_update_queue()
            .await?;

        info!("Cache update completed successfully");
        Ok(())
    }

    /// Get the local root path
    #[allow(dead_code)]
    pub fn get_local_root_path(&self) -> std::path::PathBuf {
        self.config.local_dir.clone()
    }

    /// Start periodic sync operations
    pub async fn start_periodic_sync(&mut self) -> Result<()> {
        let mut scheduler = PeriodicScheduler::new();

        // Add delta update task
        let delta_processor = self.delta_processor.clone();
        scheduler.add_task(PeriodicTask {
            name: "delta_update".to_string(),
            interval: Duration::from_secs(30),
            metrics: TaskMetrics::new(10, Duration::from_secs(25)),
            task: Box::new(move || {
                let delta_processor = delta_processor.clone();
                Box::pin(async move { delta_processor.get_delta_items_and_update_queue().await })
            }),
        });

        // Add item processing task
        let item_processor = self.item_processor.clone();
        let metadata_manager = self.metadata_manager;
        let settings = self.settings.clone();
        scheduler.add_task(PeriodicTask {
            name: "item_processing".to_string(),
            interval: Duration::from_secs(10),
            metrics: TaskMetrics::new(20, Duration::from_secs(8)),
            task: Box::new(move || {
                let item_processor = item_processor.clone();
                let metadata_manager = metadata_manager;
                let settings = settings.clone();
                Box::pin(async move {
                    
                    let delta_items = metadata_manager.get_delta_items_to_process()?;
                    info!("Starting delta Items Procesing {} delta items to process", delta_items.len());
                    for item in delta_items {
                        item_processor
                            .process_item(&item, &settings.sync_folders)
                            .await?;
                    }
                    Ok(())
                })
            }),
        });

        // Add download processing task
        let download_processor = self.download_processor.clone();
        let settings = self.settings.clone();
        scheduler.add_task(PeriodicTask {
            name: "download_processing".to_string(),
            interval: Duration::from_secs(15), // More frequent than delta
            metrics: TaskMetrics::new(15, Duration::from_secs(12)),
            task: Box::new(move || {
                let download_processor = download_processor.clone();
                let settings = settings.clone();
                Box::pin(async move {
                    download_processor.process_download_queue(&settings.sync_folders).await
                })
            }),
        });

        scheduler.start().await?;
        self.scheduler = Some(scheduler);

        info!("Periodic sync started");
        Ok(())
    }

    /// Stop periodic sync operations
    pub async fn stop_periodic_sync(&mut self) -> Result<()> {
        if let Some(mut scheduler) = self.scheduler.take() {
            scheduler.stop().await;
            info!("Periodic sync stopped");
        }
        Ok(())
    }

    /// Get sync metrics
    pub async fn get_sync_metrics(&self) -> Result<HashMap<String, crate::scheduler::TaskState>> {
        if let Some(scheduler) = &self.scheduler {
            // For now, return empty map - we can implement proper metrics later
            Ok(HashMap::new())
        } else {
            Ok(HashMap::new())
        }
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
