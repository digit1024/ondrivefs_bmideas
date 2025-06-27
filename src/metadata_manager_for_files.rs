use anyhow::Result;
use rusqlite::{Connection, params};
use std::path::{Path, PathBuf};
use std::collections::HashMap;
use serde::{Serialize, Deserialize};
use log::{info, warn};

#[derive(Debug, Serialize, Deserialize)]
pub struct MetadataRecord {
    pub onedrive_id: String,
    pub local_path: String,
    pub name: String,
    pub is_deleted: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

pub struct MetadataManagerForFiles {
    db_path: PathBuf,
}

impl MetadataManagerForFiles {
    pub fn new() -> Result<Self> {
        let home_dir = std::env::var("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("/tmp"));
        
        let onedrive_dir = home_dir.join(".onedrive");
        let db_path = onedrive_dir.join("metadata.db");
        
        // Create directory if it doesn't exist
        std::fs::create_dir_all(&onedrive_dir)?;
        
        let manager = Self { db_path };
        manager.init_database()?;
        
        Ok(manager)
    }

    fn init_database(&self) -> Result<()> {
        let conn = Connection::open(&self.db_path)?;
        
        conn.execute(
            "CREATE TABLE IF NOT EXISTS metadata (
                onedrive_id TEXT PRIMARY KEY,
                local_path TEXT NOT NULL,
                name TEXT NOT NULL,
                is_deleted BOOLEAN DEFAULT FALSE,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            )",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_local_path ON metadata(local_path)",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_is_deleted ON metadata(is_deleted)",
            [],
        )?;

        Ok(())
    }

    /// Add or update a metadata record
    pub fn add_mapping(&self, onedrive_id: &str, local_path: &Path, name: &str) -> Result<()> {
        let conn = Connection::open(&self.db_path)?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        conn.execute(
            "INSERT OR REPLACE INTO metadata (onedrive_id, local_path, name, is_deleted, created_at, updated_at)
             VALUES (?1, ?2, ?3, FALSE, ?4, ?5)",
            params![
                onedrive_id,
                local_path.to_string_lossy(),
                name,
                now,
                now
            ],
        )?;

        info!("Added metadata mapping: {} -> {}", onedrive_id, local_path.display());
        Ok(())
    }

    /// Get local path by OneDrive ID
    pub fn get_local_path(&self, onedrive_id: &str) -> Result<Option<String>> {
        let conn = Connection::open(&self.db_path)?;
        
        let mut stmt = conn.prepare(
            "SELECT local_path FROM metadata WHERE onedrive_id = ? AND is_deleted = FALSE"
        )?;
        
        let mut rows = stmt.query(params![onedrive_id])?;
        
        if let Some(row) = rows.next()? {
            let local_path: String = row.get(0)?;
            Ok(Some(local_path))
        } else {
            Ok(None)
        }
    }

    /// Get OneDrive ID by local path
    pub fn get_onedrive_id(&self, local_path: &Path) -> Result<Option<String>> {
        let conn = Connection::open(&self.db_path)?;
        
        let mut stmt = conn.prepare(
            "SELECT onedrive_id FROM metadata WHERE local_path = ? AND is_deleted = FALSE"
        )?;
        
        let mut rows = stmt.query(params![local_path.to_string_lossy()])?;
        
        if let Some(row) = rows.next()? {
            let onedrive_id: String = row.get(0)?;
            Ok(Some(onedrive_id))
        } else {
            Ok(None)
        }
    }

    /// Mark a record as deleted (soft delete)
    pub fn mark_as_deleted(&self, onedrive_id: &str) -> Result<()> {
        let conn = Connection::open(&self.db_path)?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        let rows_affected = conn.execute(
            "UPDATE metadata SET is_deleted = TRUE, updated_at = ? WHERE onedrive_id = ?",
            params![now, onedrive_id],
        )?;

        if rows_affected > 0 {
            info!("Marked metadata as deleted: {}", onedrive_id);
        } else {
            warn!("No metadata record found to mark as deleted: {}", onedrive_id);
        }

        Ok(())
    }

    /// Permanently remove a record
    pub fn remove_mapping(&self, onedrive_id: &str) -> Result<()> {
        let conn = Connection::open(&self.db_path)?;
        
        let rows_affected = conn.execute(
            "DELETE FROM metadata WHERE onedrive_id = ?",
            params![onedrive_id],
        )?;

        if rows_affected > 0 {
            info!("Removed metadata mapping: {}", onedrive_id);
        }

        Ok(())
    }

    /// Get all deleted records
    pub fn get_deleted_records(&self) -> Result<Vec<MetadataRecord>> {
        let conn = Connection::open(&self.db_path)?;
        
        let mut stmt = conn.prepare(
            "SELECT onedrive_id, local_path, name, is_deleted, created_at, updated_at 
             FROM metadata WHERE is_deleted = TRUE"
        )?;
        
        let mut records = Vec::new();
        let mut rows = stmt.query([])?;
        
        while let Some(row) = rows.next()? {
            records.push(MetadataRecord {
                onedrive_id: row.get(0)?,
                local_path: row.get(1)?,
                name: row.get(2)?,
                is_deleted: row.get(3)?,
                created_at: row.get(4)?,
                updated_at: row.get(5)?,
            });
        }
        
        Ok(records)
    }

    /// Clean up old deleted records (older than specified days)
    pub fn cleanup_deleted_records(&self, days_old: i64) -> Result<usize> {
        let conn = Connection::open(&self.db_path)?;
        let cutoff_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64 - (days_old * 24 * 60 * 60);

        let rows_affected = conn.execute(
            "DELETE FROM metadata WHERE is_deleted = TRUE AND updated_at < ?",
            params![cutoff_time],
        )?;

        info!("Cleaned up {} old deleted records", rows_affected);
        Ok(rows_affected)
    }

    /// Get all active mappings
    pub fn get_all_mappings(&self) -> Result<HashMap<String, String>> {
        let conn = Connection::open(&self.db_path)?;
        
        let mut stmt = conn.prepare(
            "SELECT onedrive_id, local_path FROM metadata WHERE is_deleted = FALSE"
        )?;
        
        let mut mappings = HashMap::new();
        let mut rows = stmt.query([])?;
        
        while let Some(row) = rows.next()? {
            let onedrive_id: String = row.get(0)?;
            let local_path: String = row.get(1)?;
            mappings.insert(onedrive_id, local_path);
        }
        
        Ok(mappings)
    }
} 