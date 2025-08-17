//! Simple scheduler replacement for periodic tasks
//! 
//! This replaces the complex PeriodicScheduler with a lightweight approach
//! that avoids memory leaks by not capturing heavy objects in closures.

use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::{interval, Duration};
use tokio::task::JoinHandle;
use log::{info, warn, error};
use anyhow::Result;

use crate::app_state::AppState;
use crate::tasks::delta_update::SyncCycle;


/// Simple task manager that avoids memory leaks
pub struct SimpleTaskManager {
    shutdown_handles: Vec<JoinHandle<()>>,
}

impl SimpleTaskManager {
    pub fn new() -> Self {
        Self {
            shutdown_handles: Vec::new(),
        }
    }

    /// Start the sync task with overlap protection
    pub async fn start_sync_task(&mut self, app_state: Arc<AppState>) -> Result<()> {
        let sync_running = Arc::new(Mutex::new(false));
        let app_state_weak = Arc::downgrade(&app_state);

        let handle = tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(30));
            
            loop {
                interval.tick().await;
                
                // Try to acquire lock - if already running, skip this cycle
                if let Ok(_guard) = sync_running.try_lock() {
                    // Get fresh app_state reference - if app is shutting down, break
                    let Some(app_state) = app_state_weak.upgrade() else {
                        info!("🛑 App state dropped, stopping sync task");
                        break;
                    };

                    let sync_running = sync_running.clone();
                    
                    // Spawn fresh task - no captured closures, clean scope
                    tokio::spawn(async move {
                        let _guard = sync_running.lock().await;
                        
                        info!("🔄 Starting sync cycle");
                        
                        // Create everything fresh - no permanent captures
                        let sync_cycle = SyncCycle::new(app_state);
                        let result = sync_cycle.run().await;
                        
                        match result {
                            Ok(_) => info!("✅ Sync cycle completed successfully"),
                            Err(e) => error!("❌ Sync cycle failed: {}", e),
                        }
                        
                        // Everything drops naturally here - sync_cycle, app_state clone, etc.
                        // No permanent references held
                    });
                } else {
                    warn!("⏭️ Sync task still running, skipping this cycle");
                }
            }
        });

        self.shutdown_handles.push(handle);
        info!("✅ Sync task started");
        Ok(())
    }



    /// Gracefully shutdown all tasks
    pub async fn shutdown(self) {
        info!("🛑 Shutting down task manager...");
        
        for handle in self.shutdown_handles {
            handle.abort();
        }
        
        info!("✅ Task manager shutdown complete");
    }
}

impl Default for SimpleTaskManager {
    fn default() -> Self {
        Self::new()
    }
}
