// SPDX-License-Identifier: MPL-2.0

use cosmic::widget::segmented_button;
use onedrive_sync_lib::dbus::types::SyncQueueItem;

#[derive(Debug, Clone)]
pub enum Message {
    FetchQueues,
    DownloadQueueLoaded(Result<Vec<SyncQueueItem>, String>),
    UploadQueueLoaded(Result<Vec<SyncQueueItem>, String>),
    Refresh,
    QueSelected(segmented_button::Entity),
    AutoRefresh,
}



