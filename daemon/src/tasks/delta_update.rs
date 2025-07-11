use std::{sync::Arc, time::Duration};

use anyhow::Result;

use crate::{app_state::AppState, onedrive_service::onedrive_models::DriveItem, persistency::database::{ProcessingItem, ProcessingItemRepository, SyncStateRepository}, scheduler::{PeriodicTask, TaskMetrics}};


struct SyncCycle {
    
    app_state: Arc<AppState>
}
impl SyncCycle {
    pub fn new(app_state: Arc<AppState>) -> Self {
        Self {
    
            app_state,
        }
    }
    pub async fn get_task(&self) -> Result<PeriodicTask> {
        let metrics = TaskMetrics::new(5, Duration::from_secs(1));

        // Clone the app_state to avoid lifetime issues
        let app_state = self.app_state.clone();
        
        let task = PeriodicTask {
            name: "adaptive_sync".to_string(),
            interval: Duration::from_secs(300),   // Start with 5 min interval
            metrics,
            task: Box::new(move || {
                let app_state = app_state.clone();
                Box::pin(async move {
                    // Your sync logic here
                    let sync_cycle = SyncCycle::new(app_state);
                    sync_cycle.run().await;
                    
                    Ok(())
                })
            }),
        };
    
        Ok(task)
    }




    pub async fn get_delta_changes(&self) -> Result<Vec<DriveItem>> {
        let sync_state_repo = SyncStateRepository::new(self.app_state.persistency_manager.pool().clone());
        let sync_state = sync_state_repo.get_latest_sync_state().await?;
        let delta_token = sync_state.map(|(_, _, delta_token)| delta_token).unwrap_or(None);
        
        let mut all_items = Vec::new();
        let mut current_token = delta_token;
        let mut final_delta_link = None;
        
        // 🔄 Handle pagination AND token expiration
        loop {
            match self.app_state.onedrive_client
                .get_delta_changes("/", current_token.as_deref())
                .await {
                
                Ok(delta) => {
                    all_items.extend(delta.value);
                    
                    if let Some(next_link) = delta.next_link {
                        // Continue pagination
                        current_token = Some(next_link);
                        continue;
                    } else {
                        // Pagination complete, store delta_link for next cycle
                        final_delta_link = delta.delta_link;
                        break;
                    }
                }
                
                Err(e) if e.to_string().contains("410") => {
                    // Token expired, restart delta sync
                    log::warn!("Delta token expired, restarting sync");
                    current_token = None;
                    continue;
                }
                
                Err(e) => return Err(e),
            }
        }
        
        // Store the delta_link for next sync cycle
        if let Some(delta_link) = final_delta_link {
            sync_state_repo.store_sync_state(Some(delta_link), "syncing", None).await?;
        }
        
        Ok(all_items)
    }

    pub async fn run(&self) -> Result<()> {

        let items = self.get_delta_changes().await?;
        let processing_repo = ProcessingItemRepository::new(self.app_state.persistency_manager.pool().clone());
        let folders_to_download = self.app_state.project_config.settings.download_folders.clone();
        
        loop {
            
        }
        for item in items {
            let processing_item = ProcessingItem::new(item);
            processing_repo.store_processing_item(&processing_item).await?;
            

        }
        

        Ok(())
    }
    
    fn apply_remote_event(&self, event: DriveItem) -> Result<()> {
        todo!()
    }
}



