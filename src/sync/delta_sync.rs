//! Delta sync processor for OneDrive delta operations

use crate::metadata_manager_for_files::MetadataManagerForFiles;
use crate::onedrive_service::onedrive_client::OneDriveClient;
use crate::onedrive_service::onedrive_models::DriveItem;
use anyhow::{Context, Result};
use log::info;

/// Processor for OneDrive delta synchronization
pub struct DeltaSyncProcessor {
    client: OneDriveClient,
    metadata_manager: &'static MetadataManagerForFiles,
}

impl DeltaSyncProcessor {
    /// Create a new delta sync processor
    pub fn new(
        client: OneDriveClient,
        metadata_manager: &'static MetadataManagerForFiles,
    ) -> Self {
        Self {
            client,
            metadata_manager,
        }
    }

    /// Get all delta items from OneDrive
    pub async fn get_delta_items(&self) -> Result<Vec<DriveItem>> {
        let delta_url = self.build_delta_url()?;
        let mut all_items = Vec::new();
        
        let mut delta_response = self
            .client
            .get_delta_by_url(&delta_url)
            .await
            .context("Failed to get delta")?;
        
        info!("Processing delta changes");

        loop {
            let items = std::mem::take(&mut delta_response.value);
            all_items.extend(items);

            if let Some(next_link) = &delta_response.next_link {
                delta_response = self
                    .client
                    .get_delta_by_url(next_link)
                    .await?;
            } else {
                // Store the delta token for next sync
                if let Some(delta_link) = &delta_response.delta_link {
                    self.metadata_manager
                        .store_folder_delta("", delta_link)
                        .context("Failed to store delta token")?;
                }
                break;
            }
        }

        info!("Processed {} delta items", all_items.len());
        Ok(all_items)
    }

    /// Build the delta URL based on stored token
    fn build_delta_url(&self) -> Result<String> {
        let delta_token = self.metadata_manager.get_folder_delta(&"".to_string())?;
        
        let url = match delta_token {
            Some(delta) => {
                if delta.delta_token.is_empty() {
                    self.get_initial_delta_url()
                } else {
                    delta.delta_token
                }
            }
            None => self.get_initial_delta_url(),
        };
        
        Ok(url)
    }

    /// Get the initial delta URL for first sync
    fn get_initial_delta_url(&self) -> String {
        "/me/drive/root/delta?select=id,name,eTag,lastModifiedDateTime,size,folder,file,@microsoft.graph.downloadUrl,deleted,parentReference".to_string()
    }
}
