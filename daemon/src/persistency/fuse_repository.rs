use crate::persistency::drive_item_repository::DriveItemRepository;
use crate::persistency::local_changes_repository::LocalChangesRepository;
use crate::persistency::types::{VirtualFile, FileSource};
use crate::onedrive_service::onedrive_models::DriveItem;
use crate::file_manager::SyncFileManager;
use anyhow::{Context, Result};
use std::path::PathBuf;
use sqlx::Pool;

pub struct FuseRepository {
    pool: Pool<sqlx::Sqlite>,
    drive_items_repo: DriveItemRepository,
    local_changes_repo: LocalChangesRepository,
    file_manager: Option<Box<dyn SyncFileManager + Send + Sync>>,
}

impl FuseRepository {
    pub fn new(pool: Pool<sqlx::Sqlite>) -> Self {
        let drive_items_repo = DriveItemRepository::new(pool.clone());
        let local_changes_repo = LocalChangesRepository::new(pool.clone());
        Self {
            pool,
            drive_items_repo,
            local_changes_repo,
            file_manager: None,
        }
    }

    /// Create a new FuseRepository with file manager for local file checking
    pub fn new_with_file_manager(
        pool: Pool<sqlx::Sqlite>, 
        file_manager: Box<dyn SyncFileManager + Send + Sync>
    ) -> Self {
        let drive_items_repo = DriveItemRepository::new(pool.clone());
        let local_changes_repo = LocalChangesRepository::new(pool.clone());
        Self {
            pool,
            drive_items_repo,
            local_changes_repo,
            file_manager: Some(file_manager),
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
            files.push(self.remote_item_to_virtual_file(&item).await?);
        }
        for change in local_changes {
            files.push(self.local_change_to_virtual_file(&change));
        }
        Ok(files)
    }

    /// Convert a remote DriveItem to a VirtualFile
    async fn remote_item_to_virtual_file(&self, item: &DriveItem) -> Result<VirtualFile> {
        // Generate inode from virtual path
        let virtual_path = self.get_virtual_path(item);
        let ino = self.generate_inode(&virtual_path);
        
        // Determine the display name based on whether file is downloaded locally
        let display_name = if item.folder.is_some() {
            // Directories should never get .onedrivedownload extension
            item.name.clone().unwrap_or_default()
        } else if let Some(ref file_manager) = self.file_manager {
            // Check if file exists locally using OneDrive ID
            let onedrive_id = item.id.as_str();
            if file_manager.file_exists_in_locally(onedrive_id) {
                // File is downloaded locally - use original name
                item.name.clone().unwrap_or_default()
            } else {
                // File is remote - append .onedrivedownload extension
                let original_name = item.name.clone().unwrap_or_default();
                if original_name.is_empty() {
                    original_name
                } else {
                    format!("{}.onedrivedownload", original_name)
                }
            }
        } else {
            // No file manager available - assume remote and append extension
            let original_name = item.name.clone().unwrap_or_default();
            if original_name.is_empty() {
                original_name
            } else {
                format!("{}.onedrivedownload", original_name)
            }
        };

        // Generate display path for path map lookups
        let display_path = if item.folder.is_some() {
            virtual_path.clone()
        } else {
            // For files, construct the display path with the display name
            if let Some(parent_ref) = &item.parent_reference {
                if let Some(parent_path) = &parent_ref.path {
                    let mut path = parent_path.replace("/drive/root:", "");
                    if !path.starts_with('/') {
                        path = format!("/{}", path);
                    }
                    if path == "/" {
                        format!("/{}", display_name)
                    } else {
                        format!("{}/{}", path, display_name)
                    }
                } else {
                    format!("/{}", display_name)
                }
            } else {
                format!("/{}", display_name)
            }
        };

        Ok(VirtualFile {
            ino,
            name: display_name,
            virtual_path,
            display_path: Some(display_path), // Add display path for lookups
            parent_ino: None, // Set by parent lookup
            is_folder: item.folder.is_some(),
            size: item.size.unwrap_or(0) as u64,
            mime_type: item.file.as_ref().and_then(|f| f.mime_type.clone()),
            created_date: item.created_date.clone(),
            last_modified: item.last_modified.clone(),
            content_file_id: item.id.clone().into(),
            source: FileSource::Remote,
            sync_status: None,
        })
    }

    /// Convert a local change to a VirtualFile
    fn local_change_to_virtual_file(&self, change: &crate::persistency::local_changes_repository::LocalChange) -> VirtualFile {
        let ino = self.generate_inode(&change.virtual_path);
        VirtualFile {
            ino,
            name: change.file_name.clone().or_else(|| change.temp_name.clone()).unwrap_or_default(),
            virtual_path: change.virtual_path.clone(),
            display_path: Some(change.virtual_path.clone()), // Local files use same path for display
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