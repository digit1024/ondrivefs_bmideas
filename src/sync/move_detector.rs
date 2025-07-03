//! Move detector for OneDrive item move operations

use crate::metadata_manager_for_files::MetadataManagerForFiles;
use crate::onedrive_service::onedrive_models::DriveItem;
use crate::operations::path_utils::{get_local_meta_cache_path_for_item, get_local_tmp_path_for_item};
use crate::operations::file_ops::move_item;
use anyhow::Result;
use log::info;
use std::path::{Path, PathBuf};

/// Move detection and handling for OneDrive items
#[allow(dead_code)]
pub struct MoveDetector {
    metadata_manager: &'static MetadataManagerForFiles,
}

impl MoveDetector {
    /// Create a new move detector
    #[allow(dead_code)]
    pub fn new(metadata_manager: &'static MetadataManagerForFiles) -> Self {
        Self { metadata_manager }
    }

    /// Detect if an item has moved and handle the move operation
    #[allow(dead_code)]
    pub async fn detect_and_handle_move(
        &self,
        item: &DriveItem,
        cache_dir: &Path,
        temp_dir: &Path,
    ) -> Result<Option<PathBuf>> {
        // Check if this item already exists at a different location
        if let Some(old_local_path_str) = self
            .metadata_manager
            .get_local_path_for_onedrive_id(&item.id)?
        {
            let old_local_path = PathBuf::from(&old_local_path_str);
            let new_local_path = get_local_meta_cache_path_for_item(item, cache_dir);

            // If the path changed, it's a move
            if old_local_path != new_local_path {
                info!(
                    "Detected move: {} -> {}",
                    old_local_path.display(),
                    new_local_path.display()
                );

                let old_temp_path = get_local_tmp_path_for_item(item, temp_dir);
                let _new_temp_path = get_local_tmp_path_for_item(item, temp_dir);

                // Handle the move operation
                move_item(
                    item,
                    &old_local_path,
                    &old_temp_path,
                    cache_dir,
                    temp_dir,
                ).await?;

                return Ok(Some(old_local_path));
            }
        }

        Ok(None)
    }

    /// Check if an item has moved without handling the move
    #[allow(dead_code)]
    pub fn has_moved(&self, item: &DriveItem, cache_dir: &Path) -> Result<bool> {
        if let Some(old_local_path_str) = self
            .metadata_manager
            .get_local_path_for_onedrive_id(&item.id)?
        {
            let old_local_path = PathBuf::from(&old_local_path_str);
            let new_local_path = get_local_meta_cache_path_for_item(item, cache_dir);

            Ok(old_local_path != new_local_path)
        } else {
            Ok(false)
        }
    }

    /// Get the old path for an item if it has moved
    #[allow(dead_code)]
    pub fn get_old_path(&self, item: &DriveItem) -> Result<Option<PathBuf>> {
        if let Some(old_local_path_str) = self
            .metadata_manager
            .get_local_path_for_onedrive_id(&item.id)?
        {
            Ok(Some(PathBuf::from(&old_local_path_str)))
        } else {
            Ok(None)
        }
    }
}
