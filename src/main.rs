mod auth;

use anyhow::Result;
use clap::{Arg, Command};

use std::{path::PathBuf};
use std::time::Duration;



mod onedrive_service;

mod config;
mod metadata_manager_for_files;


use auth::onedrive_auth::OneDriveAuth;
use onedrive_service::onedrive_client::OneDriveClient;
use config::{Settings, SyncConfig};

mod sync_service;
use sync_service::SyncService;

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
        let settings = Settings::load_from_file()?;
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
            println!("{} {} ({})", type_str, item.name.unwrap_or("Unknown".to_string()), item.last_modified.unwrap_or("Unknown".to_string()));
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
            println!("{} {} ({})", type_str, item.name.unwrap_or("Unknown".to_string()), item.last_modified.unwrap_or("Unknown".to_string()));
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
            client.download_file(download_url, std::path::Path::new(local_path), &item.id, item.name.as_ref().map_or("Unknown", |v| v), None).await?;
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
        let mut settings = Settings::load_from_file()?;
        if !settings.sync_folders.contains(folder) {
            settings.sync_folders.push(folder.clone());
            settings.save_to_file()?;
            println!("Added '{}' to sync folders.", folder);
        } else {
            println!("'{}' is already in the sync folders list.", folder);
        }
        return Ok(());
    }
    if let Some(folder) = matches.get_one::<String>("settings-remove-folder-to-sync") {
        let mut settings = Settings::load_from_file()?;
        if let Some(pos) = settings.sync_folders.iter().position(|f| f == folder) {
            settings.sync_folders.remove(pos);
            settings.save_to_file()?;
            println!("Removed '{}' from sync folders.", folder);
        } else {
            println!("'{}' was not found in the sync folders list.", folder);
        }
        return Ok(());
    }

    let settings = Settings::load_from_file()?;
    let client = OneDriveClient::new()?;
    

    if matches.get_flag("daemon") {
        let mut daemon = SyncService::new(client, config.clone(), settings.clone());
        daemon.run_daemon().await?;
    } else {
        let mut daemon = SyncService::new(client, config.clone(), settings.clone());
        daemon.ensure_authorized().await?;
        daemon.sync_cycle().await?;
        println!("Sync completed!");
    }

    Ok(())
}

