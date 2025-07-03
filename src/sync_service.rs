//! SyncService: Handles all OneDrive <-> local sync logic.

use crate::config::{Settings, SyncConfig};
use crate::file_manager::{DefaultFileManager, FileManager};
use crate::helpers::path_to_inode;
use crate::metadata_manager_for_files::{get_metadata_manager_singleton, MetadataManagerForFiles};
use crate::onedrive_service::onedrive_client::OneDriveClient;
use crate::onedrive_service::onedrive_models::DriveItem;
use anyhow::{Context, Result};
use log::warn;
use log::{debug, error, info};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use tokio::signal;
use tokio::time::sleep;

/// Service responsible for synchronizing files between OneDrive and local storage.
pub struct SyncService {
    pub client: OneDriveClient,
    pub file_manager: DefaultFileManager,
    pub config: SyncConfig,
    pub settings: Settings,
    pub metadata_manager: &'static MetadataManagerForFiles,
    
}

impl SyncService {
    pub async fn new(
        client: OneDriveClient,
        config: SyncConfig,
        settings: Settings,
    ) -> Result<Self> {
        let metadata_manager = get_metadata_manager_singleton();
        let file_manager = DefaultFileManager::new().await?;

        Ok(Self {
            client,
            file_manager,
            config,
            settings,
            metadata_manager,
        })
    }

    pub async fn init(&mut self) -> Result<()> {
        //if delta already exists in metadata manager then we can skip the initial sync

        self.update_cache().await?;

        Ok(())
    }

    async fn handle_deleted_item(&mut self, item: &DriveItem) -> Result<()> {
        // Get the old local path from metadata
        let local_path = self
            .metadata_manager
            .get_local_path_for_onedrive_id(&item.id)
            .context("Failed to get local path for onedrive id")?;
            
        if let Some(local_path_str) = local_path {
            let local_path = PathBuf::from(&local_path_str);
            info!("Found local path for deleted item: {}", local_path.display());
            
            // Delete the actual file/folder
            let deletion_result = if item.folder.is_some() {
                if local_path.exists() {
                    info!("Deleting folder: {}", local_path.display());
                    match std::fs::remove_dir_all(&local_path) {
                        Ok(_) => {
                            info!("Successfully deleted folder: {}", local_path.display());
                            Ok(())
                        }
                        Err(e) => {
                            error!("Failed to delete folder {}: {}", local_path.display(), e);
                            Err(anyhow::anyhow!("Failed to delete folder: {}", e))
                        }
                    }
                } else {
                    info!("Folder already does not exist: {}", local_path.display());
                    Ok(())
                }
            } else {
                if local_path.exists() {
                    info!("Deleting file: {}", local_path.display());
                    match std::fs::remove_file(&local_path) {
                        Ok(_) => {
                            info!("Successfully deleted file: {}", local_path.display());
                            Ok(())
                        }
                        Err(e) => {
                            error!("Failed to delete file {}: {}", local_path.display(), e);
                            Err(anyhow::anyhow!("Failed to delete file: {}", e))
                        }
                    }
                } else {
                    info!("File already does not exist: {}", local_path.display());
                    Ok(())
                }
            };
            
            // Clean up metadata mappings regardless of deletion success
            if let Err(e) = self.metadata_manager.remove_onedrive_id_to_local_path(&item.id) {
                error!("Failed to remove OneDrive ID mapping for {}: {}", item.id, e);
            }
            
            // Remove inode mapping if we can get the inode
            let inode = path_to_inode(&local_path.as_path());
            if let Err(e) = self.metadata_manager.remove_inode_to_local_path(inode) {
                error!("Failed to remove inode mapping for {}: {}", local_path.display(), e);
            }
            
            info!("Cleaned up metadata for deleted item: {}", item.id);
            
            // Propagate deletion errors but continue processing
            if let Err(e) = deletion_result {
                error!("Deletion failed for item {}: {}", item.id, e);
            }
        } else {
            warn!(
                "Deleted object not found in local cache: ID={}, name={:?}",
                item.id, item.name
            );
        }

        Ok(())
    }

    pub async fn update_cache(&mut self) -> Result<()> {
        //get the delta token from the metadata manager
        let delta_token = self.metadata_manager.get_folder_delta(&"".to_string())?;
        let url = match delta_token {
            Some(delta) => if delta.delta_token.is_empty() {
                "/me/drive/root/delta?select=id,name,eTag,lastModifiedDateTime,size,folder,file,@microsoft.graph.downloadUrl,deleted,parentReference".to_string()
            } else {
                delta.delta_token
            },
            None => "/me/drive/root/delta?select=id,name,eTag,lastModifiedDateTime,size,folder,file,@microsoft.graph.downloadUrl,deleted,parentReference".to_string(),
        };

        let mut delta_response = self
            .client
            .get_delta_by_url(url.as_str())
            .await
            .context("Failed to get delta ")?;
        info!("Updatitng Delta Cache");

        loop {
            let items = std::mem::take(&mut delta_response.value);
            for item in items {
                let local_path = if item.parent_reference.as_ref().unwrap().path.is_none() {
                    PathBuf::from("/")
                } else {
                    self.get_local_path_for_item(&item)
                };

                // Handle deleted items first (outside folder/file check)
                if let Some(_deleted_info) = &item.deleted {
                    self.handle_deleted_item(&item).await?;
                    continue;
                }

                // Only handle folders and files
                if item.folder.is_some() || item.file.is_some() {
                    //Root folder
                    if item.parent_reference.as_ref().unwrap().path.is_none() {
                        //this is the root folder
                        //we shoudl save it as a .dir.json in root
                        let dir_path = self.file_manager.get_cache_dir().join(".dir.json");
                        let dir_json = serde_json::to_string(&item)?;
                        std::fs::write(dir_path, dir_json)?;
                        continue;
                    }

                    //skip the dir.json file - if it exists its really unusuall and should not be the case honestly but we should handle it
                    if item.name.as_ref().unwrap().eq(".dir.json") {
                        continue;
                    }

                    // Check if this item already exists at a different location (move detection)
                    if let Some(old_local_path_str) = self.metadata_manager
                        .get_local_path_for_onedrive_id(&item.id)? 
                    {
                        let old_local_path = PathBuf::from(&old_local_path_str);
                        
                        // If the path changed, it's a move - clean up old location
                        if old_local_path != local_path {
                            info!("Detected move: {} -> {}", old_local_path.display(), local_path.display());
                            
                            if item.folder.is_some() {
                                if old_local_path.exists() {
                                    std::fs::remove_dir_all(&old_local_path)
                                        .context("Failed to remove old folder location")?;
                                    info!("Removed old folder location: {}", old_local_path.display());
                                }
                            } else {
                                if old_local_path.exists() {
                                    std::fs::remove_file(&old_local_path)
                                        .context("Failed to remove old file location")?;
                                    info!("Removed old file location: {}", old_local_path.display());
                                }
                            }
                        }
                    }

                    //update or create the file or folder at new location
                    info!("Updating or creating object: {}", local_path.display());

                    let object_json = serde_json::to_string(&item)?;
                    self.metadata_manager.store_onedrive_id_to_local_path(
                        &item.id,
                        &local_path.display().to_string(),
                    )?;

                    if item.folder.is_some() {
                        std::fs::create_dir_all(&local_path)
                            .context("Failed to create directory")?;
                        std::fs::write(local_path.join(".dir.json"), object_json)
                            .context("Failed to write dir.json")?;
                    } else {
                        //there is always parent for file
                        std::fs::create_dir_all(&local_path.parent().unwrap().clone())
                            .context("Failed to create directory for file")?;
                        std::fs::write(&local_path, object_json)
                            .context("Failed to write file")?;
                    }
                    let inode = path_to_inode(&local_path.as_path());
                    self.metadata_manager.store_inode_to_local_path(
                        inode,
                        local_path.display().to_string().as_str(),
                    )?;
                }
            }
            if delta_response.next_link.is_some() {
                delta_response = self
                    .client
                    .get_delta_by_url(delta_response.next_link.as_ref().unwrap())
                    .await?;
            } else {
                self.metadata_manager
                    .store_folder_delta("", &delta_response.delta_link.as_ref().unwrap())
                    .context("Failed to store delta token")?;
                self.metadata_manager
                    .flush()
                    .context("Failed to flush metadata manager")?;
                break;
            }
        }

        self.metadata_manager
            .store_folder_delta("", &delta_response.delta_link.as_ref().unwrap())?;
        self.metadata_manager.flush()?; //Remember to Save! 
        info!("Delta Cache Updated");
        info!(
            "New Delta Token: {}",
            delta_response.delta_link.as_ref().unwrap()
        );
        Ok(())
    }

    // pub async fn get_initial_directory_cache(&mut self, path: Option<String>) -> Result<()> {
    //     let mut realpath: String = "".to_string();

    //     if path.is_some() {
    //         realpath = path.unwrap().clone();
    //     }
    //     info!("Getting initial directory cache for path: {}", realpath);

    //     let mut delta_response = self.client.get_delta_for_root().await?;
    //     // WHile getting  deltas  save files and folders to cache dir
    //     while delta_response.next_link.is_some() {
    //         let items = std::mem::take(&mut delta_response.value);
    //         for item in items {
    //             //skip the dir.json file - if it exists its really unusuall and should not be the case honestly but we should handle it
    //             if item.name.as_ref().unwrap().eq(".dir.json") {
    //                 continue;
    //             }
    //             //if item is the root folder then save it as a .dir.json in the cache folder
    //             if item.parent_reference.as_ref().unwrap().path.is_none() {
    //                 //this is the root folder
    //                 //we shoudl save it as a .dir.json in root
    //                 let dir_path = self.file_manager.get_cache_dir().join(".dir.json");
    //                 let dir_json = serde_json::to_string(&item)?;
    //                 std::fs::write(dir_path, dir_json)?;
    //                 continue;
    //             }

    //             //if it is folder then save it as a .dir.json in the cache folder but make sure to get propper path from parent reference
    //             if item.folder.is_some() && (item.deleted.is_none()) {
    //                 let local_path = self.get_local_path_for_item(&item);
    //                 //create the directory in the cache folder always make sure that path exist
    //                 info!("Saving folder: {}", local_path.display());
    //                 std::fs::create_dir_all(&local_path)?;

    //                 let dir_json = serde_json::to_string(&item)?;
    //                 std::fs::write(local_path.join(".dir.json"), dir_json)?;
    //                 continue;
    //             }
    //             //if it is folder then save it as a .dir.json in the cache folder but make sure to get propper path from parent reference
    //             if item.file.is_some() && (item.deleted.is_none()) {
    //                 let local_path = self.get_local_path_for_item(&item);
    //                 info!("Saving file: {}", local_path.display());
    //                 //create the directory in the cache folder always make sure that path exist
    //                 std::fs::create_dir_all(&local_path)?;

    //                 let dir_json = serde_json::to_string(&item)?;
    //                 std::fs::write(local_path.join(item.name.as_ref().unwrap()), dir_json)?;
    //                 continue;
    //             }
    //             // Since it's a initial run we don't need to bother to much about deleted files and folders

    //             // Congrats! We can now use out cache!
    //         }
    //         delta_response = self
    //             .client
    //             .get_delta_by_url(delta_response.next_link.as_ref().unwrap())
    //             .await?;
    //     }

    //     //the loop is over so all the deltas has been fetched.
    //     // we should store delta permamently in the metadata manager
    //     self.metadata_manager
    //         .store_folder_delta(&realpath, &delta_response.delta_link.as_ref().unwrap())?;
    //     self.metadata_manager.flush()?; //Remember to Save!

    //     Ok(())
    // }

    pub fn get_local_path_for_item(
        &self,
        item: &crate::onedrive_service::onedrive_models::DriveItem,
    ) -> PathBuf {
        // Get the remote path from the parent reference
        let remote_path_from_parent = item
            .parent_reference
            .as_ref()
            .unwrap()
            .path
            .as_ref()
            .unwrap();

        // Remove the /drive/root:/ prefix - for joining the path
        let remote_path_from_parent = remote_path_from_parent
            .trim_start_matches("/drive/root:")
            .to_string();
        let remote_path_from_parent = remote_path_from_parent.trim_start_matches("/").to_string();

        // Get the synchronization root
        let synchronization_root: PathBuf = self.file_manager.get_cache_dir();

        // Get the folder path and join it with the item name
        let file_path = synchronization_root
            .join(remote_path_from_parent)
            .join(item.name.as_ref().unwrap());

        // Return the file path (wow this is nicely encapsulated logic)
        file_path
    }

    pub fn get_local_root_path(&self) -> PathBuf {
        let local_path = self.config.local_dir.clone();
        local_path
    }
}
