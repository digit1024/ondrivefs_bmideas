use crate::dbus::types::{DaemonStatus,  SyncQueueItem};


pub trait OnedRiveDbusInterface {
    async fn get_daemon_status(&self) -> zbus::Result<DaemonStatus>;
    async fn get_download_queue(&self) -> zbus::Result<Vec<SyncQueueItem>>;
    async fn get_upload_queue(&self) -> zbus::Result<Vec<SyncQueueItem>>;
    async fn full_reset(&self) -> zbus::Result<()>;

}
