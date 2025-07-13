use crate::persistency::drive_item_repository::DriveItemRepository;
use crate::persistency::local_changes_repository::LocalChangesRepository;
use crate::persistency::types::{VirtualFile, FileSource};
use crate::onedrive_service::onedrive_models::DriveItem;
use anyhow::{Context, Result};
use std::path::PathBuf;
use sqlx::Pool;

pub struct FuseRepository {
    pool: Pool<sqlx::Sqlite>,
    drive_items_repo: DriveItemRepository,
    local_changes_repo: LocalChangesRepository,
}

impl FuseRepository {
    pub fn new(pool: Pool<sqlx::Sqlite>) -> Self {
        let drive_items_repo = DriveItemRepository::new(pool.clone());
        let local_changes_repo = LocalChangesRepository::new(pool.clone());
        Self {
            pool,
            drive_items_repo,
            local_changes_repo,
        }
    }

    /// List all virtual files in a directory (unified view)
    pub async fn list_directory(&self, virtual_path: &str) -> Result<Vec<VirtualFile>> {
        // Get remote items in this directory
        let remote_items = self.drive_items_repo.get_drive_items_by_parent_path(virtual_path).await?;
        // Get local changes in this directory
        let local_changes = self.local_changes_repo.get_local_changes_by_parent_path(virtual_path).await?;
        // Merge remote and local changes into a unified view
        let mut files = Vec::new();
        for item in remote_items {
            files.push(self.remote_item_to_virtual_file(&item));
        }
        for change in local_changes {
            files.push(self.local_change_to_virtual_file(&change));
        }
        Ok(files)
    }

    /// Convert a remote DriveItem to a VirtualFile
    fn remote_item_to_virtual_file(&self, item: &DriveItem) -> VirtualFile {
        // Generate inode from virtual path
        let ino = self.generate_inode(&self.get_virtual_path(item));
        VirtualFile {
            ino,
            name: item.name.clone().unwrap_or_default(),
            virtual_path: self.get_virtual_path(item),
            parent_ino: None, // Set by parent lookup
            is_folder: item.folder.is_some(),
            size: item.size.unwrap_or(0) as u64,
            mime_type: item.file.as_ref().and_then(|f| f.mime_type.clone()),
            created_date: item.created_date.clone(),
            last_modified: item.last_modified.clone(),
            content_file_id: item.id.clone().into(),
            source: FileSource::Remote,
            sync_status: None,
        }
    }

    /// Convert a local change to a VirtualFile
    fn local_change_to_virtual_file(&self, change: &crate::persistency::local_changes_repository::LocalChange) -> VirtualFile {
        let ino = self.generate_inode(&change.virtual_path);
        VirtualFile {
            ino,
            name: change.file_name.clone().or_else(|| change.temp_name.clone()).unwrap_or_default(),
            virtual_path: change.virtual_path.clone(),
            parent_ino: None,
            is_folder: change.temp_is_folder.unwrap_or(false),
            size: change.temp_size.unwrap_or(0) as u64,
            mime_type: change.temp_mime_type.clone(),
            created_date: change.temp_created_date.clone(),
            last_modified: change.temp_last_modified.clone(),
            content_file_id: change.content_file_id.clone(),
            source: FileSource::Local,
            sync_status: Some(change.status.as_str().into()),
        }
    }

    /// Generate a virtual path for a DriveItem
    fn get_virtual_path(&self, item: &DriveItem) -> String {
        if let Some(parent_ref) = &item.parent_reference {
            if let Some(parent_path) = &parent_ref.path {
                let mut path = parent_path.replace("/drive/root:", "");
                if !path.starts_with('/') {
                    path = format!("/{}", path);
                }
                if path == "/" {
                    format!("/{}", item.name.as_deref().unwrap_or(""))
                } else {
                    format!("{}/{}", path, item.name.as_deref().unwrap_or(""))
                }
            } else {
                format!("/{}", item.name.as_deref().unwrap_or(""))
            }
        } else {
            format!("/{}", item.name.as_deref().unwrap_or(""))
        }
    }

    /// Generate an inode from a virtual path
    fn generate_inode(&self, virtual_path: &str) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut hasher = DefaultHasher::new();
        virtual_path.hash(&mut hasher);
        hasher.finish()
    }
} 