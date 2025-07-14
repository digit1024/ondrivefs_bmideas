use crate::persistency::drive_item_repository::DriveItemRepository;
use crate::persistency::local_changes_repository::LocalChangesRepository;
use crate::persistency::types::{VirtualFile, FileSource};
use crate::onedrive_service::onedrive_models::DriveItem;
use crate::file_manager::SyncFileManager;
use anyhow::{Context, Result};
use std::path::PathBuf;
use std::collections::HashMap;
use sqlx::Pool;

/// Inode mapping for tracking source of each inode
#[derive(Debug)]
struct InodeMapping {
    inode_to_source: HashMap<u64, FileSource>,
    onedrive_id_to_inode: HashMap<String, u64>,
    temporary_id_to_inode: HashMap<String, u64>,
}

impl InodeMapping {
    fn new() -> Self {
        Self {
            inode_to_source: HashMap::new(),
            onedrive_id_to_inode: HashMap::new(),
            temporary_id_to_inode: HashMap::new(),
        }
    }
}

pub struct FuseRepository {
    pool: Pool<sqlx::Sqlite>,
    drive_items_repo: DriveItemRepository,
    local_changes_repo: LocalChangesRepository,
    file_manager: Option<Box<dyn SyncFileManager + Send + Sync>>,
    inode_mapping: InodeMapping,
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
            inode_mapping: InodeMapping::new(),
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
            inode_mapping: InodeMapping::new(),
        }
    }

    /// Generate inode for remote items (OneDrive ID based)
    fn generate_inode_for_remote(&mut self, onedrive_id: &str) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut hasher = DefaultHasher::new();
        format!("REMOTE_{}", onedrive_id).hash(&mut hasher);
        let ino = hasher.finish();
        
        // Store mapping
        self.inode_mapping.onedrive_id_to_inode.insert(onedrive_id.to_string(), ino);
        self.inode_mapping.inode_to_source.insert(ino, FileSource::Remote);
        ino
    }

    /// Generate inode for local changes (Temporary ID based)
    fn generate_inode_for_local(&mut self, temporary_id: &str) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut hasher = DefaultHasher::new();
        format!("LOCAL_{}", temporary_id).hash(&mut hasher);
        let ino = hasher.finish();
        
        // Store mapping
        self.inode_mapping.temporary_id_to_inode.insert(temporary_id.to_string(), ino);
        self.inode_mapping.inode_to_source.insert(ino, FileSource::Local);
        ino
    }

    /// Get the source of an inode
    fn get_inode_source(&self, ino: u64) -> Option<FileSource> {
        self.inode_mapping.inode_to_source.get(&ino).copied()
    }

    /// Check if inode belongs to a local change
    fn is_local_change_inode(&self, ino: u64) -> bool {
        matches!(self.get_inode_source(ino), Some(FileSource::Local))
    }

    /// Check if inode belongs to a remote item
    fn is_remote_item_inode(&self, ino: u64) -> bool {
        matches!(self.get_inode_source(ino), Some(FileSource::Remote))
    }

    /// Get OneDrive ID from inode (for remote items)
    fn get_onedrive_id_from_inode(&self, ino: u64) -> Option<String> {
        self.inode_mapping.inode_to_source.get(&ino)
            .filter(|&source| *source == FileSource::Remote)
            .and_then(|_| {
                self.inode_mapping.onedrive_id_to_inode.iter()
                    .find(|&(_, &inode)| inode == ino)
                    .map(|(id, _)| id.clone())
            })
    }

    /// Get temporary ID from inode (for local changes)
    fn get_temporary_id_from_inode(&self, ino: u64) -> Option<String> {
        self.inode_mapping.inode_to_source.get(&ino)
            .filter(|&source| *source == FileSource::Local)
            .and_then(|_| {
                self.inode_mapping.temporary_id_to_inode.iter()
                    .find(|&(_, &inode)| inode == ino)
                    .map(|(id, _)| id.clone())
            })
    }

    /// List all virtual files in a directory (unified view)
    pub async fn list_directory(&mut self, virtual_path: &str) -> Result<Vec<VirtualFile>> {
        // Get remote items in this directory
        let remote_items = self.drive_items_repo.get_drive_items_by_parent_path(virtual_path).await?;
        // Get local changes in this directory
        let local_changes = self.local_changes_repo.get_changes_by_parent_id(virtual_path).await?;
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
    async fn remote_item_to_virtual_file(&mut self, item: &DriveItem) -> Result<VirtualFile> {
        // Generate inode from OneDrive ID
        let virtual_path = self.get_virtual_path(item);
        let ino = self.generate_inode_for_remote(&item.id);
        
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
    fn local_change_to_virtual_file(&mut self, change: &crate::persistency::local_changes_repository::LocalChange) -> VirtualFile {
        // For local changes, we need to construct the virtual path based on the change type
        let virtual_path = match change.change_type {
            crate::persistency::local_changes_repository::LocalChangeType::CreateFile | 
            crate::persistency::local_changes_repository::LocalChangeType::CreateFolder => {
                // For create operations, construct path from parent_id and file_name
                if let (Some(parent_id), Some(file_name)) = (&change.parent_id, &change.file_name) {
                    // This is a simplified path construction - in a real implementation,
                    // you'd need to resolve parent_id to parent path
                    format!("/temp/{}", file_name)
                } else {
                    format!("/temp/{}", change.temporary_id)
                }
            },
            crate::persistency::local_changes_repository::LocalChangeType::Move => {
                // For move operations, use the new parent path
                if let Some(new_inode) = change.new_inode {
                    format!("/temp/moved_{}", new_inode)
                } else {
                    format!("/temp/{}", change.temporary_id)
                }
            },
            crate::persistency::local_changes_repository::LocalChangeType::Rename => {
                // For rename operations, use the new name
                if let Some(new_name) = &change.new_name {
                    format!("/temp/{}", new_name)
                } else {
                    format!("/temp/{}", change.temporary_id)
                }
            },
            _ => {
                // For other operations, use temporary ID
                format!("/temp/{}", change.temporary_id)
            }
        };

        let ino = self.generate_inode_for_local(&change.temporary_id);
        
        // Determine the name based on change type
        let name = match change.change_type {
            crate::persistency::local_changes_repository::LocalChangeType::CreateFile | 
            crate::persistency::local_changes_repository::LocalChangeType::CreateFolder => {
                change.file_name.clone().unwrap_or_else(|| change.temporary_id.clone())
            },
            crate::persistency::local_changes_repository::LocalChangeType::Rename => {
                change.new_name.clone().unwrap_or_else(|| change.temporary_id.clone())
            },
            _ => {
                change.temporary_id.clone()
            }
        };

        VirtualFile {
            ino,
            name,
            virtual_path: virtual_path.clone(),
            display_path: Some(virtual_path), // Local files use same path for display
            parent_ino: None,
            is_folder: change.temp_is_folder.unwrap_or(false),
            size: change.file_size.unwrap_or(0) as u64,
            mime_type: change.mime_type.clone(),
            created_date: change.temp_created_date.clone(),
            last_modified: change.temp_last_modified.clone(),
            content_file_id: change.onedrive_id.clone(),
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
} 