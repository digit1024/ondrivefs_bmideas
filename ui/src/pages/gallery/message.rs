// SPDX-License-Identifier: MPL-2.0

use onedrive_sync_lib::dbus::types::MediaItem;

#[derive(Debug, Clone)]
pub enum Message {
    FetchPage,
    MediaLoaded(Result<Vec<MediaItem>, String>),
    LoadMore,
    DateStartChanged(String),
    DateEndChanged(String),
    ApplyFilters,
    AutoRefresh,
    ThumbLoaded(u64, Result<String, String>),
    OpenItem(String), // virtual_path
    Opened(Result<String, String>),
    Noop,
}

