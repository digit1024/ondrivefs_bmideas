use crate::persistency::drive_item_repository::DriveItemRepository;
use crate::persistency::local_changes_repository::{LocalChange, LocalChangeType, LocalChangesRepository};
use crate::persistency::types::{VirtualFile, FileSource};
use crate::onedrive_service::onedrive_models::{DriveItem, FileFacet, FolderFacet, ParentReference};
use crate::file_manager::SyncFileManager;
use anyhow::{Context, Result};
use std::path::PathBuf;
use std::collections::HashMap;
use sqlx::Pool;

/// Inode mapping for tracking source of each inode
#[derive(Debug)]
struct InodeMapping {
    inode_to_source: HashMap<u64, FileSource>,
    // Forward mappings (ID → inode)
    onedrive_id_to_inode: HashMap<String, u64>,
    temporary_id_to_inode: HashMap<String, u64>,
    // Reverse mappings (inode → ID)
    inode_to_onedrive_id: HashMap<u64, String>,
    inode_to_temporary_id: HashMap<u64, String>,
}

impl InodeMapping {
    fn new() -> Self {
        Self {
            inode_to_source: HashMap::new(),
            onedrive_id_to_inode: HashMap::new(),
            temporary_id_to_inode: HashMap::new(),
            inode_to_onedrive_id: HashMap::new(),
            inode_to_temporary_id: HashMap::new(),
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

    /// Get the database pool
    pub fn get_pool(&self) -> &Pool<sqlx::Sqlite> {
        &self.pool
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
        self.inode_mapping.inode_to_onedrive_id.insert(ino, onedrive_id.to_string());
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
        self.inode_mapping.inode_to_temporary_id.insert(ino, temporary_id.to_string());
        ino
    }

    /// Get the source of an inode
    pub fn get_inode_source(&self, ino: u64) -> Option<FileSource> {
        self.inode_mapping.inode_to_source.get(&ino).copied()
    }

    /// Check if inode belongs to a local change
    pub fn is_local_change_inode(&self, ino: u64) -> bool {
        matches!(self.get_inode_source(ino), Some(FileSource::Local))
    }

    /// Check if inode belongs to a remote item
    pub fn is_remote_item_inode(&self, ino: u64) -> bool {
        matches!(self.get_inode_source(ino), Some(FileSource::Remote))
    }
  

    /// Get OneDrive ID from inode (for remote items)
    fn get_onedrive_id_from_inode(&self, ino: u64) -> Option<String> {
        self.inode_mapping.inode_to_source.get(&ino)
            .filter(|&source| *source == FileSource::Remote)
            .and_then(|_| self.inode_mapping.inode_to_onedrive_id.get(&ino).cloned())
    }

    /// Get temporary ID from inode (for local changes)
    pub fn get_temporary_id_from_inode(&self, ino: u64) -> Option<String> {
        self.inode_mapping.inode_to_source.get(&ino)
            .filter(|&source| *source == FileSource::Local)
            .and_then(|_| self.inode_mapping.inode_to_temporary_id.get(&ino).cloned())
    }

    /// Get ID from inode (works for both remote and local items)
    pub fn get_id_from_inode(&self, ino: u64) -> Option<String> {
        match self.get_inode_source(ino) {
            Some(FileSource::Remote) | Some(FileSource::Merged) => self.get_onedrive_id_from_inode(ino),
            Some(FileSource::Local) => self.get_temporary_id_from_inode(ino),
            None => None,
        }
    }

    /// Add a local inode mapping (for newly created files/directories)
    pub fn add_local_inode_mapping(&mut self, temporary_id: &str, ino: u64) {
        self.inode_mapping.temporary_id_to_inode.insert(temporary_id.to_string(), ino);
        self.inode_mapping.inode_to_source.insert(ino, FileSource::Local);
        self.inode_mapping.inode_to_temporary_id.insert(ino, temporary_id.to_string());
    }

    /// List all virtual files in a directory (unified view)
    pub async fn list_directory(&mut self, virtual_path: &str) -> Result<Vec<VirtualFile>> {
        // Get remote items in this directory
        let mut remote_items = self.drive_items_repo.get_drive_items_by_parent_path(virtual_path).await?;
        // Get local changes in this directory
        let mut local_changes = self.local_changes_repo.get_all_pending_changes().await?;
        self.patch_drive_items_with_local_changes(&mut remote_items, local_changes).await?;

        // Merge remote and local changes into a unified view
        let mut files = Vec::new();
        for item in remote_items {
            files.push(self.remote_item_to_virtual_file(&item).await?);
        }


        Ok(files)
    }
    async fn patch_drive_items_with_local_changes(&mut self, DriveItems: &mut Vec<DriveItem> , local_changes: Vec<LocalChange>) -> Result<()> {
        for local_change in local_changes {
            match local_change.change_type {
                LocalChangeType::Delete => self.apply_delete_drive_item(DriveItems, local_change).await?,
                LocalChangeType::CreateFile | LocalChangeType::CreateFolder => self.apply_create_drive_item(DriveItems, local_change).await?,
                LocalChangeType::Rename | LocalChangeType::Move => self.apply_modify_drive_item(DriveItems, local_change).await?,
                LocalChangeType::Modify => self.apply_move_drive_item(DriveItems, local_change).await?,
                _ => {}
            }
        }
        Ok(())
    }
    async fn apply_modify_drive_item(&mut self, drive_items: &mut Vec<DriveItem> , local_change: LocalChange) -> Result<()> {
        
        Ok(())
    }
    async fn apply_move_drive_item(&mut self, drive_items: &mut Vec<DriveItem> , local_change: LocalChange) -> Result<()> {
        Ok(())
    }
    async fn apply_delete_drive_item(&mut self, drive_items: &mut Vec<DriveItem> , local_change: LocalChange) -> Result<()> {
        for i in 0..drive_items.len() {
            let file = &drive_items[i];
            let id = file.id.clone();
            if local_change.onedrive_id.is_some() && local_change.onedrive_id.as_ref().unwrap() == &id {// it will do nothing if the file is in VirtualFiles
                drive_items.remove(i);
            }
        }
        Ok(())
    }
    async fn apply_create_drive_item(&mut self, drive_items: &mut Vec<DriveItem> , local_change: LocalChange) -> Result<()> {
        let id = if local_change.onedrive_id.is_some() {
            local_change.onedrive_id.clone().unwrap()
        } else {
            local_change.temporary_id.clone()
        };
        let folder =  local_change.temp_is_folder.unwrap_or(false);
        let option_folder = if folder { Some(FolderFacet{child_count:0}) } else{ None };
        let option_file = if !folder { Some(FileFacet{mime_type:None}) } else{ None };
        
         
        let parent_id = local_change.parent_id.clone().unwrap();


        let virtual_drive_item = DriveItem {
            id: id,
            name: local_change.file_name.clone(),
            etag: local_change.new_etag.clone(),
            last_modified: local_change.temp_last_modified.clone(),
            created_date: local_change.temp_created_date.clone(),
            size: Some(local_change.file_size.unwrap_or(0) as u64),
            folder: option_folder, 
            file: option_file,
            download_url: None,
            deleted: None,
            parent_reference: Some(ParentReference{id:parent_id, path:None}),
        };
        drive_items.push(virtual_drive_item);
        Ok(())
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