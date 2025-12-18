// SPDX-License-Identifier: MPL-2.0

use onedrive_sync_lib::dbus::types::{ConflictItem, UserChoice};

#[derive(Debug, Clone)]
pub enum Message {
    Resolve { db_id: i64, choice: UserChoice },
    Resolved(Result<(), String>),
    Loaded(Result<Vec<ConflictItem>, String>),
    Reload,
}

