//! Path utilities for OneDrive path transformations and conversions

use crate::onedrive_service::onedrive_models::DriveItem;
use std::path::{Path, PathBuf};

/// Convert OneDrive parent reference path to local path components
pub fn onedrive_path_to_local_components(parent_path: &str) -> PathBuf {
    // Remove the /drive/root:/ prefix - for joining the path
    let clean_path = parent_path
        .trim_start_matches("/drive/root:")
        .trim_start_matches("/");
    
    PathBuf::from(clean_path)
}

/// Get local temporary path for a OneDrive item
pub fn get_local_tmp_path_for_item(
    item: &DriveItem,
    temp_download_dir: &Path,
) -> PathBuf {
    let remote_path_from_parent = item
        .parent_reference
        .as_ref()
        .unwrap()
        .path
        .as_deref()
        .unwrap_or(&"");

    let local_components = onedrive_path_to_local_components(remote_path_from_parent);
    
    // Get the folder path and join it with the item name
    temp_download_dir
        .join(local_components)
        .join(item.name.as_ref().unwrap())
}

/// Get local metadata cache path for a OneDrive item
pub fn get_local_meta_cache_path_for_item(
    item: &DriveItem,
    cache_dir: &Path,
) -> PathBuf {
    let remote_path_from_parent = item
        .parent_reference
        .as_ref()
        .unwrap()
        .path
        .as_ref()
        .unwrap();

    let local_components = onedrive_path_to_local_components(remote_path_from_parent);
    
    // Get the folder path and join it with the item name
    cache_dir
        .join(local_components)
        .join(item.name.as_ref().unwrap())
}

/// Convert virtual path to cache path
#[allow(dead_code)]
pub fn virtual_path_to_cache_path(virtual_path: &Path, cache_dir: &Path) -> PathBuf {
    if virtual_path == Path::new("/") {
        // Root directory
        cache_dir.to_path_buf()
    } else {
        // Remove leading slash and join with cache dir
        let relative_path = virtual_path.strip_prefix("/").unwrap_or(virtual_path);
        cache_dir.join(relative_path)
    }
}

/// Convert cache path to virtual path
#[allow(dead_code)]
pub fn cache_path_to_virtual_path(cache_path: &Path, cache_dir: &Path) -> PathBuf {
    let relative_path = cache_path.strip_prefix(cache_dir).unwrap();
    if relative_path == Path::new("") {
        // Root directory case
        PathBuf::from("/")
    } else {
        // Add leading slash to make it a proper virtual path
        PathBuf::from("/").join(relative_path)
    }
}

/// Convert virtual path to temp download path
#[allow(dead_code)]
pub fn virtual_path_to_temp_path(virtual_path: &Path, temp_dir: &Path) -> PathBuf {
    if virtual_path == Path::new("/") {
        temp_dir.to_path_buf()
    } else {
        let relative_path = virtual_path.strip_prefix("/").unwrap_or(virtual_path);
        temp_dir.join(relative_path)
    }
}
