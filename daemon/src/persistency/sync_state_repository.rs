use anyhow::{Context, Result};
use log::info;
use sqlx::{Pool, Row, Sqlite};

/// Database operations for sync state
#[derive(Clone)]
pub struct SyncStateRepository {
    pool: Pool<Sqlite>,
}

impl SyncStateRepository {
    /// Create a new sync state repository
    pub fn new(pool: Pool<Sqlite>) -> Self {
        Self { pool }
    }

    pub async fn clear_all_items(&self) -> Result<()> {
        sqlx::query("DELETE FROM sync_state")
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Store sync state
    pub async fn store_sync_state(
        &self,
        delta_link: Option<String>,
        status: &str,
        error_message: Option<String>,
    ) -> Result<()> {
        sqlx::query(
            r#"
            INSERT OR REPLACE INTO sync_state (delta_link, last_sync_time, sync_status, error_message)
            VALUES (?, CURRENT_TIMESTAMP, ?, ?)
            "#,
        )
        .bind(&delta_link)
        .bind(status)
        .bind(error_message)
        .execute(&self.pool)
        .await?;

        info!(
            "Stored sync state: status={}, delta_link={:?}",
            status, delta_link
        );
        Ok(())
    }

    /// Get the latest sync state
    pub async fn get_latest_sync_state(&self) -> Result<Option<(String, String, Option<String>)>> {
        let row = sqlx::query(
            r#"
            SELECT delta_link, sync_status, error_message
            FROM sync_state ORDER BY id DESC LIMIT 1
            "#,
        )
        .fetch_optional(&self.pool)
        .await?;

        if let Some(row) = row {
            let delta_link: String = row.try_get("delta_link")?;
            let sync_status: String = row.try_get("sync_status")?;
            let error_message: Option<String> = row.try_get("error_message")?;
            Ok(Some((delta_link, sync_status, error_message)))
        } else {
            Ok(None)
        }
    }
}
