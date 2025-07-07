//! Delta sync processor for OneDrive delta operations

use crate::metadata_manager_for_files::MetadataManagerForFiles;
use crate::onedrive_service::onedrive_client::OneDriveClient;
use anyhow::{Context, Result};
use log::info;

/// Processor for OneDrive delta synchronization
#[derive(Clone)]
pub struct DeltaSyncProcessor {
    client: OneDriveClient,
    metadata_manager: &'static MetadataManagerForFiles,
}

impl DeltaSyncProcessor {
    /// Create a new delta sync processor
    pub fn new(client: OneDriveClient, metadata_manager: &'static MetadataManagerForFiles) -> Self {
        Self {
            client,
            metadata_manager,
        }
    }

    /// Get all delta items from OneDrive
    pub async fn get_delta_items_and_update_queue(&self) -> Result<()> {
        let delta_url = self.build_delta_url()?;

        let mut delta_response = self
            .client
            .get_delta_by_url(&delta_url)
            .await
            .context("Failed to get delta")?;

        info!("Processing delta changes");

        loop {
            let items = delta_response.value;
            let mut c = 0;
            for item in items {
                self.metadata_manager.store_delta_items_to_process(&item).context("Failed to store delta items to process")?;
                c += 1;
            }
            self.metadata_manager.flush()?;
            info!(
                "New drive items added to queue for processing. NUmber of Items: {}",
                c
            );

            if let Some(next_link) = &delta_response.next_link {
                delta_response = self.client.get_delta_by_url(next_link).await?;
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
        self.metadata_manager.flush()?;

        Ok(())
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
        "/me/drive/root/delta?select=id,name,eTag,lastModifiedDateTime,size,folder,file,@microsoft.graph.downloadUrl,deleted,parentReference&top=5000".to_string()
    }
}
