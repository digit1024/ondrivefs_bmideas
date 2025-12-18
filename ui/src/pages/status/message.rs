// SPDX-License-Identifier: MPL-2.0

use onedrive_sync_lib::dbus::types::{DaemonStatus, UserProfile};

#[derive(Debug, Clone)]
pub enum Message {
    FetchStatus,
    StatusSignal(DaemonStatus),
    StatusLoaded(Result<DaemonStatus, String>),
    ProfileLoaded(Result<UserProfile, String>),
    Refresh,
    AutoRefresh,
    FullReset,
    ToggleSyncPause,
}

