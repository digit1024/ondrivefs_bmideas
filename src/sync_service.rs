//! SyncService: Handles all OneDrive <-> local sync logic.

use crate::onedrive_service::onedrive_client::OneDriveClient;
use crate::config::{Settings, SyncConfig};
use crate::metadata_manager_for_files::MetadataManagerForFiles;
use crate::file_manager::{FileManager, DefaultFileManager};
use anyhow::{Ok, Result};
use std::path::{Path, PathBuf};
use crate::onedrive_service::onedrive_models::DriveItem;
use tokio::time::sleep;
use tokio::signal;
use log::{info, error, debug};
use log::warn;
use std::str::FromStr;

/// Service responsible for synchronizing files between OneDrive and local storage.
pub struct SyncService {
    pub client: OneDriveClient,
    pub file_manager: DefaultFileManager,
    pub config: SyncConfig,
    pub settings: Settings,
    pub metadata_manager: MetadataManagerForFiles,
}

impl SyncService {
    pub async fn new(client: OneDriveClient, config: SyncConfig, settings: Settings) -> Result<Self> {
        let metadata_manager = MetadataManagerForFiles::new()?;
        let file_manager = DefaultFileManager::new().await?;
        
        Ok(Self { 
            client, 
            file_manager,
            config, 
            settings , 
            metadata_manager
        })
    }

    pub async fn init(&mut self) -> Result<()> {
        //if delta already exists in metadata manager then we can skip the initial sync
        let delta_token = self.metadata_manager.get_folder_delta(&"".to_string())?;
        if delta_token.is_none()   {
            info!("No delta token found, getting initial directory cache");
            self.get_initial_directory_cache(None).await?;
        }
        
        
        info!("Delta token found, skipping initial sync");
        info!("Delta token: {}", delta_token.unwrap().delta_token);
        self.update_cache().await?;

        Ok(())
    }

    pub async fn update_cache(&mut self) -> Result<()> {    
        //get the delta token from the metadata manager
        let delta_token = self.metadata_manager.get_folder_delta(&"".to_string())?;
        let mut delta_response = self.client.get_delta_by_url(delta_token.unwrap().delta_token.as_str()).await?;
        info!("Updatitng Delta Cache");
        loop {
            let items = std::mem::take(&mut delta_response.value);
            for item in items {
                let local_path  = if item.parent_reference.as_ref().unwrap().path.is_none() {
                    PathBuf::from("/")
                }else{
                     self.get_local_path_for_item(&item)
                };
                 
                //handle folders and files
                if item.folder.is_some() || item.file.is_some() {
                //if the item is a folder then we need to create the directory in the cache folder
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

                    
                    
                    if(item.deleted.is_some() && item.deleted.as_ref().unwrap() == &true) {
                        info!("Deleting object: {}", item.name.as_ref().unwrap());
                        //delete the folder
                        //TODO: item does not have a parent - so now we cannot know which folder or file to delete
                        //we should handle this case
                        info!("Deleting object: {}", local_path.display());
                        //std::fs::remove_dir_all(&local_path)?;
                    } else {
                        //update or create the folder
                        info!("Updating or creating object: {}", local_path.display());
                        std::fs::create_dir_all(&local_path)?;
                        let dir_json = serde_json::to_string(&item)?;
                        if item.folder.is_some() {
                            std::fs::write(local_path.join(".dir.json"), dir_json)?;
                        } else {
                            std::fs::write(local_path.join(item.name.as_ref().unwrap()), dir_json)?;
                        }
                    }
                }

            }
            if delta_response.next_link.is_some() {
                delta_response = self.client.get_delta_by_url(delta_response.next_link.as_ref().unwrap()).await?;
            } else {
                break;
            }
        }
        
        self.metadata_manager.store_folder_delta("", &delta_response.delta_link.as_ref().unwrap())?;
        self.metadata_manager.flush()?;//Remember to Save! 
        info!("Delta Cache Updated");
        info!("New Delta Token: {}", delta_response.delta_link.as_ref().unwrap());
        Ok(())
    }



    pub async fn get_initial_directory_cache(&mut self, path: Option<String>) -> Result<()> {
        let mut realpath: String = "".to_string();

        if path.is_some() {
            realpath = path.unwrap().clone();
        }
        info!("Getting initial directory cache for path: {}", realpath);

        let mut delta_response = self.client.get_delta_for_root().await?;
        // WHile getting  deltas  save files and folders to cache dir
        while delta_response.next_link.is_some() {
            
            let items = std::mem::take(&mut delta_response.value);
            for item in items {
                
                
                //skip the dir.json file - if it exists its really unusuall and should not be the case honestly but we should handle it
                if item.name.as_ref().unwrap().eq(".dir.json") {
                    continue;
                }
                //if item is the root folder then save it as a .dir.json in the cache folder
                if item.parent_reference.as_ref().unwrap().path.is_none() {
                    //this is the root folder
                    //we shoudl save it as a .dir.json in root
                    let dir_path = self.file_manager.get_cache_dir().join(".dir.json");
                    let dir_json = serde_json::to_string(&item)?;
                    std::fs::write(dir_path, dir_json)?;
                    continue;
                }

                //if it is folder then save it as a .dir.json in the cache folder but make sure to get propper path from parent reference
                if item.folder.is_some()  && ( item.deleted.is_none() ) {

                    let local_path = self.get_local_path_for_item(&item);
                    //create the directory in the cache folder always make sure that path exist
                    info!("Saving folder: {}", local_path.display());
                    std::fs::create_dir_all(&local_path)?;
                    

                    let dir_json = serde_json::to_string(&item)?;
                    std::fs::write(local_path.join(".dir.json"), dir_json)?;
                    continue;
                }
                //if it is folder then save it as a .dir.json in the cache folder but make sure to get propper path from parent reference
                if item.file.is_some()  && ( item.deleted.is_none() ) {

                    let local_path = self.get_local_path_for_item(&item);
                    info!("Saving file: {}", local_path.display());
                    //create the directory in the cache folder always make sure that path exist
                    std::fs::create_dir_all(&local_path)?;

                    let dir_json = serde_json::to_string(&item)?;
                    std::fs::write(local_path.join(item.name.as_ref().unwrap()), dir_json)?;
                    continue;
                }
                // Since it's a initial run we don't need to bother to much about deleted files and folders

                // Congrats! We can now use out cache!
                

        }
        delta_response = self.client.get_delta_by_url(delta_response.next_link.as_ref().unwrap()).await?;
    }

        //the loop is over so all the deltas has been fetched. 
        // we should store delta permamently in the metadata manager
        self.metadata_manager.store_folder_delta(&realpath, &delta_response.delta_link.as_ref().unwrap())?;
        self.metadata_manager.flush()?;//Remember to Save! 

        Ok(())
    }





   

    /// Get remote path from item's parent reference
    fn get_remote_path_from_item(&self, _folder: &str, item: &crate::onedrive_service::onedrive_models::DriveItem) -> Result<String> {
        let remote_path_from_parent = item.parent_reference.as_ref()
            .ok_or_else(|| anyhow::anyhow!("No parent reference for item"))?
            .path.as_ref()
            .ok_or_else(|| anyhow::anyhow!("No path in parent reference"))?;
        
        // Remove /drive/root: prefix
        let remote_path_from_parent = remote_path_from_parent.trim_start_matches("/drive/root:");
        
        let remote_path = format!("{}/{}", remote_path_from_parent, item.name.as_ref().map_or("Unknown", |v| v));
        Ok(remote_path)
    }



  



  

 
    



    /// Helper: Get local path for a OneDrive item
    pub fn get_local_path_for_item(&self, item: &crate::onedrive_service::onedrive_models::DriveItem) -> PathBuf {
 
        // Get the remote path from the parent reference
        let remote_path_from_parent = item.parent_reference.as_ref().unwrap().path.as_ref().unwrap();
        
        // Remove the /drive/root:/ prefix - for joining the path
        let remote_path_from_parent = remote_path_from_parent.trim_start_matches("/drive/root:").to_string();
        let remote_path_from_parent = remote_path_from_parent.trim_start_matches("/").to_string();
        
        // Get the synchronization root
        let synchronization_root  : PathBuf = self.file_manager.get_cache_dir();
        
        // Get the folder path and join it with the item name
        let file_path = synchronization_root.join(remote_path_from_parent).join(item.name.as_ref().unwrap());
        

        // Return the file path (wow this is nicely encapsulated logic)
        file_path
    }
    
    pub fn get_local_root_path(&self) -> PathBuf {
        let local_path = self.config.local_dir.clone();
        local_path
    }

    // pub async fn ensure_authorized(&self) -> Result<()> {
    //     match self.client.list_root().await {
    //         Result::Ok(_) => {
    //             info!("Already authorized and tokens are valid");
    //             Ok(())
    //         }
    //         Result::Err(_) => {
    //             warn!("Authorization needed or tokens expired");
    //             let auth = crate::auth::onedrive_auth::OneDriveAuth::new()?;
    //             auth.get_valid_token().await?;
    //             info!("Authorization completed");
    //             Ok(())
    //         }
    //     }
    // }


    // /// Extract delta token from delta link URL
    // pub fn extract_delta_token(delta_link: &str) -> Option<String> {
    //     if let Some(token_start) = delta_link.find("token=") {
    //         let token = &delta_link[token_start + 6..];
    //         Some(token.to_string())
    //     } else {
    //         None
    //     }
    // }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use tempfile::tempdir;

    #[tokio::test]
    #[serial]
    async fn test_sync_service_skeleton() {
        // TODO: Setup a mock OneDriveClient, config, and settings
        // let client = ...;
        // let config = ...;
        // let settings = ...;
        // let mut service = SyncService::new(client, config, settings);
        // assert!(service.sync_from_remote().await.is_ok());
        assert!(true); // Placeholder
    }
} 