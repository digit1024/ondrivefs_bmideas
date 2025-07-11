use std::{sync::Arc, time::Duration};

use tokio::sync::Mutex;
use anyhow::Result;

use crate::{app_state::AppState, onedrive_service::onedrive_models::DriveItem, persistency::database::SyncStateRepository, scheduler::{PeriodicTask, TaskMetrics}};


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
        // Get the latest sync state
        let sync_state_repo = SyncStateRepository::new(self.app_state.persistency_manager.pool().clone());
        let sync_state = sync_state_repo.get_latest_sync_state().await?;
        let delta_token = sync_state.map(|(_, _, delta_token)| delta_token).unwrap_or(None);
        let delta = self.app_state.onedrive_client.get_delta_changes("/",delta_token.as_deref()).await?;
        
        // Store the delta token
        sync_state_repo.store_sync_state(delta.next_link, "syncing", None).await?;



        Ok(delta.value)
    }

    pub async fn run(&self) -> Result<()> {

        let items = self.get_delta_changes().await?;
        let mut conflicts = Vec::new();
         // 1. Apply remote changes in order
        for event in items {
            if let Err(conflict) = self.apply_remote_event(event) {
                conflicts.push(conflict);
            }
        }
        

        Ok(())
    }
    
    fn apply_remote_event(&self, event: DriveItem) -> Result<()> {
        todo!()
    }
}



