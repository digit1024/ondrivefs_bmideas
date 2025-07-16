//! DownloadQueueRepository: Handles download_queue table operations 
use anyhow::{Context, Result};
use log::{debug, warn};
use sqlx::{Pool, Row, Sqlite};
use std::path::PathBuf;

/// Database operations for download queue
pub struct DownloadQueueRepository {
    pool: Pool<Sqlite>,
}

impl DownloadQueueRepository {
    /// Create a new download queue repository
    pub fn new(pool: Pool<Sqlite>) -> Self {
        Self { pool }
    }

    /// Add item to download queue
    pub async fn add_to_download_queue(
        &self,
        drive_item_id: &str,
        local_path: &PathBuf,
    ) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO download_queue (drive_item_id, local_path, status)
            VALUES (?, ?, 'pending')
            "#,
        )
        .bind(drive_item_id)
        .bind(local_path.to_string_lossy())
        .execute(&self.pool)
        .await?;

        debug!(
            "Added to download queue: {} -> {}",
            drive_item_id,
            local_path.display()
        );
        Ok(())
    }

    /// Get pending download items
    pub async fn get_pending_downloads(&self) -> Result<Vec<(i64, String, PathBuf)>> {
        let rows = sqlx::query(
            r#"
            SELECT id, drive_item_id, local_path
            FROM download_queue 
            WHERE status = 'pending' 
            ORDER BY priority DESC, created_at ASC
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        let mut items = Vec::new();
        for row in rows {
            let id: i64 = row.try_get("id")?;
            let drive_item_id: String = row.try_get("drive_item_id")?;
            let local_path: String = row.try_get("local_path")?;
            items.push((id, drive_item_id, PathBuf::from(local_path)));
        }

        Ok(items)
    }

    /// Mark download as completed
    pub async fn mark_download_completed(&self, queue_id: i64) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE download_queue 
            SET status = 'completed', updated_at = CURRENT_TIMESTAMP
            WHERE id = ?
            "#,
        )
        .bind(queue_id)
        .execute(&self.pool)
        .await?;

        debug!("Marked download as completed: {}", queue_id);
        Ok(())
    }

    /// Mark download as failed
    pub async fn mark_download_failed(&self, queue_id: i64, retry_count: i32) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE download_queue 
            SET status = 'failed', retry_count = ?, updated_at = CURRENT_TIMESTAMP
            WHERE id = ?
            "#,
        )
        .bind(retry_count)
        .bind(queue_id)
        .execute(&self.pool)
        .await?;

        warn!(
            "Marked download as failed: {} (retry count: {})",
            queue_id, retry_count
        );
        Ok(())
    }

    /// Remove item from download queue by drive_item_id
    pub async fn remove_by_drive_item_id(&self, drive_item_id: &str) -> Result<()> {
        sqlx::query(
            r#"
            DELETE FROM download_queue 
            WHERE drive_item_id = ?
            "#,
        )
        .bind(drive_item_id)
        .execute(&self.pool)
        .await?;

        debug!("Removed item from download queue: {}", drive_item_id);
        Ok(())
    }
} 