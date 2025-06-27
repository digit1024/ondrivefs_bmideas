use anyhow::Result;
use sled::{Db, Tree};
use std::path::{Path, PathBuf};
use std::collections::HashMap;
use serde::{Serialize, Deserialize};
use log::{info, warn, debug};

/// Simplified metadata record that only stores local path as value
#[derive(Debug, Serialize, Deserialize)]
pub struct FileMetadata {
    pub local_path: String,
}

/// Delta record for folder synchronization
#[derive(Debug, Serialize, Deserialize)]
pub struct FolderDelta {
    pub delta_token: String,
    pub last_sync: i64,
}

/// Metadata manager using sled key-value storage
pub struct MetadataManagerForFiles {
    db: Db,
    files_mapping: Tree,
    folder_deltas: Tree,
    changed_queue: Tree,
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
        let files_mapping = db.open_tree("files_mapping")?;
        let folder_deltas = db.open_tree("folder_deltas")?;
        let changed_queue = db.open_tree("changed_queue")?;
        
        let manager = Self {
            db,
            files_mapping,
            folder_deltas,
            changed_queue,
        };
        
        info!("Initialized metadata manager with sled database at {:?}", db_path);
        Ok(manager)
    }

    /// Add metadata for a file (OneDrive ID -> local path mapping)
    pub fn add_metadata_for_file(&self, onedrive_id: &str, local_path: &Path) -> Result<()> {
        let metadata = FileMetadata {
            local_path: local_path.to_string_lossy().to_string(),
        };
        
        let json_value = serde_json::to_string(&metadata)?;
        self.files_mapping.insert(onedrive_id.as_bytes(), json_value.as_bytes())?;
        
        info!("Added file metadata: {} -> {}", onedrive_id, local_path.display());
        Ok(())
    }

    /// Delete metadata for a file
    pub fn delete_metadata_for_file(&self, onedrive_id: &str) -> Result<()> {
        let removed = self.files_mapping.remove(onedrive_id.as_bytes())?;
        
        if removed.is_some() {
            info!("Deleted file metadata: {}", onedrive_id);
        } else {
            warn!("No file metadata found to delete: {}", onedrive_id);
        }
        
        Ok(())
    }

    /// Get local path from OneDrive ID
    pub fn get_local_path_from_one_drive_id(&self, onedrive_id: &str) -> Result<Option<String>> {
        if let Some(value) = self.files_mapping.get(onedrive_id.as_bytes())? {
            let json_str = String::from_utf8(value.to_vec())?;
            let metadata: FileMetadata = serde_json::from_str(&json_str)?;
            Ok(Some(metadata.local_path))
        } else {
            Ok(None)
        }
    }

    /// Get OneDrive ID from local path (reverse lookup)
    pub fn get_one_drive_id_from_local_path(&self, local_path: &Path) -> Result<Option<String>> {
        let path_str = local_path.to_string_lossy();
        
        // Scan through all entries to find matching local path
        for result in self.files_mapping.iter() {
            let (key, value) = result?;
            let json_str = String::from_utf8(value.to_vec())?;
            let metadata: FileMetadata = serde_json::from_str(&json_str)?;
            
            if metadata.local_path == path_str {
                let onedrive_id = String::from_utf8(key.to_vec())?;
                return Ok(Some(onedrive_id));
            }
        }
        
        Ok(None)
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
        self.folder_deltas.insert(folder_path.as_bytes(), json_value.as_bytes())?;
        
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

    /// Get all file mappings
    pub fn get_all_file_mappings(&self) -> Result<HashMap<String, String>> {
        let mut mappings = HashMap::new();
        
        for result in self.files_mapping.iter() {
            let (key, value) = result?;
            let onedrive_id = String::from_utf8(key.to_vec())?;
            let json_str = String::from_utf8(value.to_vec())?;
            let metadata: FileMetadata = serde_json::from_str(&json_str)?;
            
            mappings.insert(onedrive_id, metadata.local_path);
        }
        
        Ok(mappings)
    }

    /// Get all folder deltas
    pub fn get_all_folder_deltas(&self) -> Result<HashMap<String, FolderDelta>> {
        let mut deltas = HashMap::new();
        
        for result in self.folder_deltas.iter() {
            let (key, value) = result?;
            let folder_path = String::from_utf8(key.to_vec())?;
            let json_str = String::from_utf8(value.to_vec())?;
            let delta: FolderDelta = serde_json::from_str(&json_str)?;
            
            deltas.insert(folder_path, delta);
        }
        
        Ok(deltas)
    }

    /// Flush all pending writes to disk
    pub fn flush(&self) -> Result<()> {
        self.db.flush()?;
        Ok(())
    }

    /// Get database statistics
    pub fn get_stats(&self) -> Result<HashMap<String, usize>> {
        let mut stats = HashMap::new();
        
        stats.insert("files_mapping_count".to_string(), self.files_mapping.len());
        stats.insert("folder_deltas_count".to_string(), self.folder_deltas.len());
        stats.insert("changed_queue_count".to_string(), self.changed_queue.len());
        
        Ok(stats)
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
    use tempfile::tempdir;
    use std::env;

    fn setup_test_env() {
        let temp_dir = tempdir().unwrap();
        unsafe {
            env::set_var("HOME", temp_dir.path());
        }
    }

    #[test]
    fn test_file_metadata_operations() {
        setup_test_env();
        
        let manager = MetadataManagerForFiles::new().unwrap();
        let test_id = "test_onedrive_id_123";
        let test_path = Path::new("/home/user/test_file.txt");
        
        // Test adding metadata
        manager.add_metadata_for_file(test_id, test_path).unwrap();
        
        // Test retrieving metadata
        let retrieved_path = manager.get_local_path_from_one_drive_id(test_id).unwrap();
        assert_eq!(retrieved_path, Some(test_path.to_string_lossy().to_string()));
        
        // Test reverse lookup
        let retrieved_id = manager.get_one_drive_id_from_local_path(test_path).unwrap();
        assert_eq!(retrieved_id, Some(test_id.to_string()));
        
        // Test deleting metadata
        manager.delete_metadata_for_file(test_id).unwrap();
        let deleted_path = manager.get_local_path_from_one_drive_id(test_id).unwrap();
        assert_eq!(deleted_path, None);
    }

    #[test]
    fn test_folder_delta_operations() {
        setup_test_env();
        
        let manager = MetadataManagerForFiles::new().unwrap();
        let folder_path = "/Documents";
        let delta_token = "test_delta_token_123";
        
        // Test storing delta
        manager.store_folder_delta(folder_path, delta_token).unwrap();
        
        // Test retrieving delta
        let delta = manager.get_folder_delta(folder_path).unwrap();
        assert!(delta.is_some());
        let delta = delta.unwrap();
        assert_eq!(delta.delta_token, delta_token);
        assert!(delta.last_sync > 0);
    }

    #[test]
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
        manager.add_to_changed_queue("/home/user/another_file.txt").unwrap();
        manager.clear_changed_queue().unwrap();
        let files = manager.get_changed_queue_files().unwrap();
        assert_eq!(files.len(), 0);
    }

    #[test]
    fn test_get_all_mappings() {
        setup_test_env();
        
        let manager = MetadataManagerForFiles::new().unwrap();
        
        // Add multiple mappings
        manager.add_metadata_for_file("id1", Path::new("/file1.txt")).unwrap();
        manager.add_metadata_for_file("id2", Path::new("/file2.txt")).unwrap();
        
        let mappings = manager.get_all_file_mappings().unwrap();
        assert_eq!(mappings.len(), 2);
        assert_eq!(mappings.get("id1"), Some(&"/file1.txt".to_string()));
        assert_eq!(mappings.get("id2"), Some(&"/file2.txt".to_string()));
    }

    #[test]
    fn test_get_stats() {
        setup_test_env();
        
        let manager = MetadataManagerForFiles::new().unwrap();
        
        // Add some data
        manager.add_metadata_for_file("id1", Path::new("/file1.txt")).unwrap();
        manager.store_folder_delta("/docs", "token1").unwrap();
        manager.add_to_changed_queue("/file1.txt").unwrap();
        
        let stats = manager.get_stats().unwrap();
        assert_eq!(stats.get("files_mapping_count"), Some(&1));
        assert_eq!(stats.get("folder_deltas_count"), Some(&1));
        assert_eq!(stats.get("changed_queue_count"), Some(&1));
    }
} 