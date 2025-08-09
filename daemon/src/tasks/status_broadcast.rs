use std::sync::Arc;
use tokio::time::{sleep, Duration};
use zbus::object_server::SignalEmitter;
use onedrive_sync_lib::dbus::types::DaemonStatus;

pub struct StatusBroadcastTask {
    app_state: Arc<crate::app_state::AppState>,
    connection: zbus::Connection,
}

impl StatusBroadcastTask {
    pub fn new(app_state: Arc<crate::app_state::AppState>, connection: zbus::Connection) -> Self {
        Self { app_state, connection }
    }

    async fn compute_status(&self) -> DaemonStatus {
        use onedrive_sync_lib::dbus::types::SyncStatus;
        let is_authenticated = self.app_state.auth().get_valid_token().await.is_ok();
        let is_connected = matches!(
            self.app_state.connectivity().check_connectivity().await,
            crate::connectivity::ConnectivityStatus::Online
        );
        let sync_status = if let Some(metrics) = self
            .app_state
            .scheduler()
            .get_task_metrics("sync_cycle")
            .await
        {
            if metrics.is_running {
                SyncStatus::Running
            } else {
                SyncStatus::Paused
            }
        } else {
            SyncStatus::Paused
        };

        let has_conflicts = self
            .app_state
            .persistency()
            .processing_item_repository()
            .get_processing_items_by_status(
                &crate::persistency::processing_item_repository::ProcessingStatus::Conflicted,
            )
            .await
            .map(|items| !items.is_empty())
            .unwrap_or(false);

        let path_str = format!("{}/OneDrive", std::env::var("HOME").unwrap_or_default());
        let p = std::path::Path::new(&path_str);
        let mounts = std::fs::read_to_string("/proc/mounts").unwrap_or_default();
        let is_mounted = mounts
            .lines()
            .any(|line| line.split_whitespace().nth(1) == Some(p.to_str().unwrap_or_default()));

        DaemonStatus { is_authenticated, is_connected, sync_status, has_conflicts, is_mounted }
    }

    pub async fn run(self) {
        let mut last: Option<DaemonStatus> = None;
        loop {
            let status = self.compute_status().await;
            let changed = match &last {
                Some(prev) => prev != &status,
                None => true,
            };
            if changed {
                if let Ok(emitter) = SignalEmitter::new(&self.connection, "/org/freedesktop/OneDriveSync") {
                    let _ = crate::dbus_server::server::ServiceImpl::emit_daemon_status_changed(&emitter, status.clone()).await;
                }
                last = Some(status);
            }
            sleep(Duration::from_secs(10)).await;
        }
    }
}



