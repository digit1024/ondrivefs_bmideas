use anyhow::Result;
use clap::{Arg, Command};
use log::{info, error, warn};
use std::path::PathBuf;
use std::time::Duration;
use tokio::time::sleep;

mod onedrive_auth;
mod onedrive_client;
mod token_store;
mod config;

use onedrive_auth::OneDriveAuth;
use onedrive_client::OneDriveClient;
use token_store::AuthConfig;
use config::{Settings, SyncConfig, ChangedQueue};


struct SyncDaemon {
    client: OneDriveClient,
    config: SyncConfig,
    settings: Settings,
}

impl SyncDaemon {
    fn new(config: SyncConfig, settings: Settings) -> Result<Self> {
        Ok(Self {
            client: OneDriveClient::new()?,
            config,
            settings,
        })
    }

    /// Run initial authorization if needed
    async fn ensure_authorized(&self) -> Result<()> {
        match self.client.list_root().await {
            Ok(_) => {
                info!("Already authorized and tokens are valid");
                Ok(())
            }
            Err(_) => {
                warn!("Authorization needed or tokens expired");
                // This will trigger the OAuth flow
                let auth = OneDriveAuth::new()?;
                auth.get_valid_token().await?;
                info!("Authorization completed");
                Ok(())
            }
        }
    }

    /// Sync files from OneDrive to local directory
    async fn sync_from_remote(&self) -> Result<()> {
        info!("Starting sync from remote...");
        tokio::fs::create_dir_all(&self.config.local_dir).await?;
        for folder in &self.settings.sync_folders {
            let items = match self.client.list_folder_by_path(folder).await {
                Ok(items) => items,
                Err(e) => {
                    warn!("Remote directory '{}' might not exist: {}", folder, e);
                    self.client.create_folder("/", folder.trim_start_matches('/')).await?;
                    Vec::new()
                }
            };
            for item in items {
                if item.file.is_some() {
                    let local_path = self.config.local_dir.join(&item.name);
                    if let Some(download_url) = &item.download_url {
                        match self.client.download_file(download_url, &local_path).await {
                            Ok(_) => info!("Downloaded: {}", item.name),
                            Err(e) => error!("Failed to download {}: {}", item.name, e),
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// Sync files from local directory to OneDrive
    async fn sync_to_remote(&self) -> Result<()> {
        info!("Starting sync to remote...");
        for folder in &self.settings.sync_folders {
            let local_folder = self.config.local_dir.join(folder.trim_start_matches('/'));
            let mut entries = match tokio::fs::read_dir(&local_folder).await {
                Ok(e) => e,
                Err(_) => continue, // Skip if local folder doesn't exist
            };
            while let Some(entry) = entries.next_entry().await? {
                let path = entry.path();
                if path.is_file() {
                    if let Some(file_name) = path.file_name().and_then(|n| n.to_str()) {
                        let remote_path = format!("{}/{}", folder, file_name);
                        match self.client.upload_file(&path, &remote_path).await {
                            Ok(_) => info!("Uploaded: {}", file_name),
                            Err(e) => error!("Failed to upload {}: {}", file_name, e),
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// Run a single sync cycle
    async fn sync_cycle(&self) -> Result<()> {
        info!("Starting sync cycle");
        
        // Bi-directional sync
        if let Err(e) = self.sync_from_remote().await {
            error!("Failed to sync from remote: {}", e);
        }

        if let Err(e) = self.sync_to_remote().await {
            error!("Failed to sync to remote: {}", e);
        }

        info!("Sync cycle completed");
        Ok(())
    }

    /// Run daemon mode
    async fn run_daemon(&self) -> Result<()> {
        info!("Starting OneDrive sync daemon");
        info!("Local directory: {:?}", self.config.local_dir);
        info!("Remote directory: {}", self.config.remote_dir);
        info!("Sync interval: {:?}", self.config.sync_interval);

        self.ensure_authorized().await?;

        loop {
            if let Err(e) = self.sync_cycle().await {
                error!("Sync cycle failed: {}", e);
            }

            info!("Waiting {} seconds until next sync", self.config.sync_interval.as_secs());
            sleep(self.config.sync_interval).await;
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();

    let matches = Command::new("OneDrive Sync")
        .version("1.0")
        .about("OneDrive synchronization daemon for Linux")
        .arg(
            Arg::new("daemon")
                .short('d')
                .long("daemon")
                .action(clap::ArgAction::SetTrue)
                .help("Run in daemon mode")
        )
        .arg(
            Arg::new("local-dir")
                .short('l')
                .long("local-dir")
                .value_name("PATH")
                .help("Local directory to sync")
        )
        .arg(
            Arg::new("remote-dir")
                .short('r')
                .long("remote-dir")
                .value_name("PATH")
                .help("Remote directory to sync")
        )
        .arg(
            Arg::new("interval")
                .short('i')
                .long("interval")
                .value_name("SECONDS")
                .help("Sync interval in seconds")
                .default_value("15")
        )
        .arg(
            Arg::new("auth")
                .long("auth")
                .action(clap::ArgAction::SetTrue)
                .help("Run authorization flow only")
        )
        .arg(
            Arg::new("list")
                .long("list")
                .action(clap::ArgAction::SetTrue)
                .help("List files in OneDrive root")
        )
        .arg(
            Arg::new("list-dir")
                .long("list-dir")
                .value_name("PATH")
                .help("List files in a specific OneDrive directory")
        )
        .arg(
            Arg::new("get-file")
                .long("get-file")
                .num_args(2)
                .value_names(["REMOTE_PATH", "LOCAL_PATH"])
                .help("Download a file from OneDrive to a local path")
        )
        .arg(
            Arg::new("put-file")
                .long("put-file")
                .num_args(2)
                .value_names(["LOCAL_PATH", "REMOTE_PATH"])
                .help("Upload a local file to a OneDrive directory")
        )
        .arg(
            Arg::new("settings-add-folder-to-sync")
                .long("settings-add-folder-to-sync")
                .value_name("FOLDER")
                .help("Add a folder to the sync list in settings.json")
        )
        .arg(
            Arg::new("settings-remove-folder-to-sync")
                .long("settings-remove-folder-to-sync")
                .value_name("FOLDER")
                .help("Remove a folder from the sync list in settings.json")
        )
        .arg(
            Arg::new("settings-list-folders-to-sync")
                .long("settings-list-folders-to-sync")
                .help("List all folders currently set to sync in settings.json")
                .action(clap::ArgAction::SetTrue)
        )
        .get_matches();

    let mut config = SyncConfig::default();

    if let Some(local_dir) = matches.get_one::<String>("local-dir") {
        config.local_dir = PathBuf::from(local_dir);
    }

    if let Some(remote_dir) = matches.get_one::<String>("remote-dir") {
        config.remote_dir = remote_dir.clone();
    }

    if let Some(interval) = matches.get_one::<String>("interval") {
        config.sync_interval = Duration::from_secs(interval.parse()?);
    }

    // Handle different modes
    if matches.get_flag("auth") {
        // Authorization only
        let auth = OneDriveAuth::new()?;
        auth.authorize().await?;
        println!("Authorization completed!");
        return Ok(());
    }
    if matches.get_flag("settings-list-folders-to-sync") {
        let settings = Settings::load()?;
        println!("Folders set to sync:");
        for folder in &settings.sync_folders {
            println!("- {}", folder);
        }
        return Ok(());
    }

    if matches.get_flag("list") {
        // List files only
        let client = OneDriveClient::new()?;
        let items = client.list_root().await?;
        
        println!("Files in OneDrive root:");
        for item in items {
            let type_str = if item.folder.is_some() { "📁" } else { "📄" };
            println!("{} {} ({})", type_str, item.name, item.last_modified);
        }
        return Ok(());
    }

    if let Some(list_dir) = matches.get_one::<String>("list-dir") {
        // List files in a specific OneDrive directory
        let client = OneDriveClient::new()?;
        let items = client.list_folder_by_path(list_dir).await?;
        println!("Files in OneDrive directory '{}':", list_dir);
        for item in items {
            let type_str = if item.folder.is_some() { "📁" } else { "📄" };
            println!("{} {} ({})", type_str, item.name, item.last_modified);
        }
        return Ok(());
    }

    if let Some(get_args) = matches.get_many::<String>("get-file") {
        let args: Vec<_> = get_args.collect();
        let remote_path = &args[0];
        let local_path = &args[1];
        let client = OneDriveClient::new()?;
        let item = client.get_item_by_path(remote_path).await?;
        if let Some(download_url) = &item.download_url {
            client.download_file(download_url, std::path::Path::new(local_path)).await?;
            println!("Downloaded '{}' to '{}'", remote_path, local_path);
        } else {
            println!("No download URL found for '{}'. Is it a folder?", remote_path);
        }
        return Ok(());
    }

    if let Some(put_args) = matches.get_many::<String>("put-file") {
        let args: Vec<_> = put_args.collect();
        let local_path = &args[0];
        let remote_path = &args[1];
        let client = OneDriveClient::new()?;
        client.upload_file(std::path::Path::new(local_path), remote_path).await?;
        println!("Uploaded '{}' to '{}'", local_path, remote_path);
        return Ok(());
    }

    // Handle settings management options
    if let Some(folder) = matches.get_one::<String>("settings-add-folder-to-sync") {
        let mut settings = Settings::load()?;
        if !settings.sync_folders.contains(folder) {
            settings.sync_folders.push(folder.clone());
            settings.save()?;
            println!("Added '{}' to sync folders.", folder);
        } else {
            println!("'{}' is already in the sync folders list.", folder);
        }
        return Ok(());
    }
    if let Some(folder) = matches.get_one::<String>("settings-remove-folder-to-sync") {
        let mut settings = Settings::load()?;
        if let Some(pos) = settings.sync_folders.iter().position(|f| f == folder) {
            settings.sync_folders.remove(pos);
            settings.save()?;
            println!("Removed '{}' from sync folders.", folder);
        } else {
            println!("'{}' was not found in the sync folders list.", folder);
        }
        return Ok(());
    }


    let settings = Settings::load()?;
    if matches.get_flag("daemon") {
        let daemon = SyncDaemon::new(config, settings)?;
        daemon.run_daemon().await?;
    } else {
        let daemon = SyncDaemon::new(config, settings)?;
        daemon.ensure_authorized().await?;
        daemon.sync_cycle().await?;
        println!("Sync completed!");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sync_config_default() {
        let config = SyncConfig::default();
        assert_eq!(config.local_dir, PathBuf::from("./sync"));
        assert_eq!(config.remote_dir, "/sync");
        assert_eq!(config.sync_interval, Duration::from_secs(300));
    }
}