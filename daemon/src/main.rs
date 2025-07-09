//! OneDrive FUSE filesystem for Linux
//!
//! This is a FUSE filesystem that provides access to OneDrive files
//! through a local mount point. Files are cached locally and synchronized
//! with OneDrive in the background.

mod auth;

mod log_appender;

mod onedrive_service;
mod scheduler;

use anyhow::{Context, Result};

use clap::Command;
use log::{ info};
use onedrive_sync_lib::config::ProjectConfig;

use crate::log_appender::setup_logging;

struct AppState {
    project_config: ProjectConfig,
    
}

#[tokio::main]
async fn main() -> Result<()> {

    let mut app_state = AppState{
        project_config: ProjectConfig::new().await?,
    };

    // Initialize logging
    setup_logging(&app_state.project_config.project_dirs.data_dir().to_path_buf()).await.context("Failed to setup logging")?;

    // Parse command line arguments
    let matches = Command::new("OneDrive Client for Linux by digit1024@github")
        .version("01.0")
        .about("Mount OneDrive as a FUSE filesystem")
        .get_matches();

    

    info!("Daemon started");
    Ok(())
}
