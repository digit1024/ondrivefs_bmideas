//! UploadQueueRepository: Handles upload_queue table operations 
use anyhow::{Context, Result};
use log::debug;
use sqlx::{Pool, Row, Sqlite};
use std::path::PathBuf;

/// Database operations for upload queue
pub struct UploadQueueRepository {
    pool: Pool<Sqlite>,
}

impl UploadQueueRepository {
    /// Create a new upload queue repository
    pub fn new(pool: Pool<Sqlite>) -> Self {
        Self { pool }
    }

    /// Add item to upload queue
    pub async fn add_to_upload_queue(
        &self,
        local_path: &PathBuf,
        parent_id: Option<String>,
        file_name: &str,
    ) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO upload_queue (local_path, parent_id, file_name, status)
            VALUES (?, ?, ?, 'pending')
            "#,
        )
        .bind(local_path.to_string_lossy())
        .bind(parent_id)
        .bind(file_name)
        .execute(&self.pool)
        .await?;

        debug!(
            "Added to upload queue: {} -> {}",
            local_path.display(),
            file_name
        );
        Ok(())
    }

    /// Get pending upload items
    pub async fn get_pending_uploads(&self) -> Result<Vec<(i64, PathBuf, Option<String>, String)>> {
        let rows = sqlx::query(
            r#"
            SELECT id, local_path, parent_id, file_name
            FROM upload_queue 
            WHERE status = 'pending' 
            ORDER BY priority DESC, created_at ASC
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        let mut items = Vec::new();
        for row in rows {
            let id: i64 = row.try_get("id")?;
            let local_path: String = row.try_get("local_path")?;
            let parent_id: Option<String> = row.try_get("parent_id")?;
            let file_name: String = row.try_get("file_name")?;
            items.push((id, PathBuf::from(local_path), parent_id, file_name));
        }

        Ok(items)
    }

    /// Mark upload as completed
    pub async fn mark_upload_completed(&self, queue_id: i64) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE upload_queue 
            SET status = 'completed', updated_at = CURRENT_TIMESTAMP
            WHERE id = ?
            "#,
        )
        .bind(queue_id)
        .execute(&self.pool)
        .await?;

        debug!("Marked upload as completed: {}", queue_id);
        Ok(())
    }

    /// Mark upload as failed
    pub async fn mark_upload_failed(&self, queue_id: i64, retry_count: i32) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE upload_queue 
            SET status = 'failed', retry_count = ?, updated_at = CURRENT_TIMESTAMP
            WHERE id = ?
            "#,
        )
        .bind(retry_count)
        .bind(queue_id)
        .execute(&self.pool)
        .await?;

        debug!(
            "Marked upload as failed: {} (retry count: {})",
            queue_id, retry_count
        );
        Ok(())
    }
} 