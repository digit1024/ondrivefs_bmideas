use crate::onedrive_service::onedrive_models::{DriveItem, ParentReference};
use anyhow::{Context, Result};
use log::{debug, warn};
use sqlx::{Pool, Row, Sqlite};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq)]
pub enum ProcessingStatus {
    New,
    Processing,
    Conflict,
    Error,
    Done,
}

impl ProcessingStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            ProcessingStatus::New => "new",
            ProcessingStatus::Processing => "processing",
            ProcessingStatus::Conflict => "conflict",
            ProcessingStatus::Error => "error",
            ProcessingStatus::Done => "done",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "new" => Some(ProcessingStatus::New),
            "processing" => Some(ProcessingStatus::Processing),
            "conflict" => Some(ProcessingStatus::Conflict),
            "error" => Some(ProcessingStatus::Error),
            "done" => Some(ProcessingStatus::Done),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ProcessingItem {
    pub drive_item: DriveItem,
    pub status: ProcessingStatus,
    pub local_path: Option<PathBuf>,
    pub error_message: Option<String>,
    pub last_status_update: Option<String>,
    pub retry_count: i32,
    pub priority: i32,
}

impl ProcessingItem {
    pub fn new(drive_item: DriveItem) -> Self {
        Self {
            drive_item,
            status: ProcessingStatus::New,
            local_path: None,
            error_message: None,
            last_status_update: None,
            retry_count: 0,
            priority: 0,
        }
    }

    pub fn into_drive_item(self) -> DriveItem {
        self.drive_item
    }

    pub fn drive_item(&self) -> &DriveItem {
        &self.drive_item
    }

    pub fn drive_item_mut(&mut self) -> &mut DriveItem {
        &mut self.drive_item
    }
}

pub struct ProcessingItemRepository {
    pool: Pool<Sqlite>,
}

impl ProcessingItemRepository {
    pub fn new(pool: Pool<Sqlite>) -> Self {
        Self { pool }
    }

    // ... (rest of ProcessingItemRepository impl copied from database.rs) ...
} 