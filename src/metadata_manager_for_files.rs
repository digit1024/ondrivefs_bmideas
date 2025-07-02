use anyhow::Result;
use log::{debug, info, warn};
use serde::{Deserialize, Serialize};
use sled::{Db, Tree};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Simplified metadata record that only stores local path as value
#[derive(Debug, Serialize, Deserialize)]
pub struct FileMetadata {
    pub local_path: String,
}

/// Delta record for folder synchronization
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct FolderDelta {
    pub delta_token: String,
    pub last_sync: i64,
}

/// Onedrive file metadata
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct OnedriveFileMeta {
    pub etag: String,
    pub id: String,
}

/// Metadata manager using sled key-value storage
pub struct MetadataManagerForFiles {
    db: Db,

    folder_deltas: Tree,
    changed_queue: Tree,
    onedrive_id_to_local_path: Tree,
}



impl MetadataManagerForFiles {
    /// Create a new metadata manager instance
    pub fn new() -> Result<Self> {
        let home_dir = std::env::var("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("/tmp"));

        let onedrive_dir = home_dir.join(".onedrive");
        let db_path = onedrive_dir.join("metadata.sled");

        // Create directory if it doesn't exist
        std::fs::create_dir_all(&onedrive_dir)?;

        // Open sled database
        let db = sled::open(&db_path)?;

        // Open trees for different data types
        let onedrive_id_to_local_path = db.open_tree("onedrive_id_to_local_path")?;
        let folder_deltas = db.open_tree("folder_deltas")?;
        let changed_queue = db.open_tree("changed_queue")?;

        let manager = Self {
            db,
            folder_deltas,
            changed_queue,
            onedrive_id_to_local_path,
        };

        info!(
            "Initialized metadata manager with sled database at {:?}",
            db_path
        );
        manager.flush().unwrap();
        Ok(manager)
    }

    /// Store delta token for a folder
    pub fn store_folder_delta(&self, folder_path: &str, delta_token: &str) -> Result<()> {
        let delta = FolderDelta {
            delta_token: delta_token.to_string(),
            last_sync: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64,
        };

        let json_value = serde_json::to_string(&delta)?;
        self.folder_deltas
            .insert(folder_path.as_bytes(), json_value.as_bytes())?;

        debug!("Stored delta token for folder: {}", folder_path);
        Ok(())
    }

    /// Get delta token for a folder
    pub fn get_folder_delta(&self, folder_path: &str) -> Result<Option<FolderDelta>> {
        if let Some(value) = self.folder_deltas.get(folder_path.as_bytes())? {
            let json_str = String::from_utf8(value.to_vec())?;
            let delta: FolderDelta = serde_json::from_str(&json_str)?;
            Ok(Some(delta))
        } else {
            Ok(None)
        }
    }

    /// Add a file to the changed queue
    pub fn add_to_changed_queue(&self, full_path: &str) -> Result<()> {
        // Store empty string as value (we only need the key)
        self.changed_queue.insert(full_path.as_bytes(), b"")?;
        debug!("Added to changed queue: {}", full_path);
        Ok(())
    }

    /// Remove a file from the changed queue
    pub fn remove_from_changed_queue(&self, full_path: &str) -> Result<()> {
        let removed = self.changed_queue.remove(full_path.as_bytes())?;

        if removed.is_some() {
            debug!("Removed from changed queue: {}", full_path);
        }

        Ok(())
    }

    /// Get all files in the changed queue
    pub fn get_changed_queue_files(&self) -> Result<Vec<String>> {
        let mut files = Vec::new();

        for result in self.changed_queue.iter() {
            let (key, _) = result?;
            let file_path = String::from_utf8(key.to_vec())?;
            files.push(file_path);
        }

        Ok(files)
    }

    /// Clear the changed queue
    pub fn clear_changed_queue(&self) -> Result<()> {
        self.changed_queue.clear()?;
        info!("Cleared changed queue");
        Ok(())
    }


    /// Flush all pending writes to disk
    pub fn flush(&self) -> Result<()> {
        self.db.flush()?;
        Ok(())
    }

    pub fn store_onedrive_id_to_local_path(&self, onedrive_id: &str, local_path: &str) -> Result<()> {
        self.onedrive_id_to_local_path.insert(onedrive_id.as_bytes(), local_path.as_bytes())?;
        Ok(())
    }

    pub fn get_local_path_for_onedrive_id(&self, onedrive_id: &str) -> Result<Option<String>> {
        if let Some(value) = self.onedrive_id_to_local_path.get(onedrive_id.as_bytes())? {
            let local_path = String::from_utf8(value.to_vec())?;
            return Ok(Some(local_path));
        }
        return Ok(None);
    }

    pub fn remove_onedrive_id_to_local_path(&self, onedrive_id: &str) -> Result<()> {
        self.onedrive_id_to_local_path.remove(onedrive_id.as_bytes())?;
        Ok(())
    }


}

impl Drop for MetadataManagerForFiles {
    fn drop(&mut self) {
        if let Err(e) = self.flush() {
            warn!("Failed to flush metadata database on drop: {}", e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::env;
    use tempfile::tempdir;

    fn setup_test_env() {
        let temp_dir = tempdir().unwrap();
        unsafe {
            env::set_var("HOME", temp_dir.path());
        }
    }

    #[test]
    #[serial]
    fn test_folder_delta_operations() {
        setup_test_env();

        let manager = MetadataManagerForFiles::new().unwrap();
        let folder_path = "/Documents";
        let delta_token = "test_delta_token_123";

        // Test storing delta
        manager
            .store_folder_delta(folder_path, delta_token)
            .unwrap();

        // Test retrieving delta
        let delta = manager.get_folder_delta(folder_path).unwrap();
        assert!(delta.is_some());
        let delta = delta.unwrap();
        assert_eq!(delta.delta_token, delta_token);
        assert!(delta.last_sync > 0);
    }

    #[test]
    #[serial]
    fn test_changed_queue_operations() {
        setup_test_env();

        let manager = MetadataManagerForFiles::new().unwrap();
        let test_file = "/home/user/test_file.txt";

        // Test adding to queue
        manager.add_to_changed_queue(test_file).unwrap();

        // Test getting queue contents
        let files = manager.get_changed_queue_files().unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0], test_file);

        // Test removing from queue
        manager.remove_from_changed_queue(test_file).unwrap();
        let files = manager.get_changed_queue_files().unwrap();
        assert_eq!(files.len(), 0);

        // Test clearing queue
        manager.add_to_changed_queue(test_file).unwrap();
        manager
            .add_to_changed_queue("/home/user/another_file.txt")
            .unwrap();
        manager.clear_changed_queue().unwrap();
        let files = manager.get_changed_queue_files().unwrap();
        assert_eq!(files.len(), 0);
    }
}
