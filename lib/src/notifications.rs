// SPDX-License-Identifier: MPL-2.0

use anyhow::Result;
use zbus::Connection;
use zbus::zvariant::Value;

/// Notification urgency levels
#[derive(Debug, Clone)]
pub enum NotificationUrgency {
    Low,
    Normal,
    Critical,
}

impl NotificationUrgency {
    pub fn to_u8(&self) -> u8 {
        match self {
            NotificationUrgency::Low => 0,
            NotificationUrgency::Normal => 1,
            NotificationUrgency::Critical => 2,
        }
    }
}

/// Desktop notification sender
#[derive(Debug, Clone)]
pub struct NotificationSender {
    connection: Connection,
}

impl NotificationSender {
    /// Create a new notification sender
    pub async fn new() -> Result<Self> {
        let connection = Connection::session().await?;
        Ok(Self { connection })
    }

    /// Send a desktop notification
    pub async fn send_notification(
        &self,
        app_name: &str,
        notification_id: u32,
        icon: &str,
        summary: &str,
        body: &str,
        actions: Vec<&str>,
        hints: Vec<(&str, &str)>,
        timeout: i32,
    ) -> Result<()> {
        let proxy = zbus::Proxy::new(
            &self.connection,
            "org.freedesktop.Notifications",
            "/org/freedesktop/Notifications",
            "org.freedesktop.Notifications",
        )
        .await?;

        // Convert hints to the format expected by DBus
        let mut hints_map = std::collections::HashMap::new();
        for (key, value) in hints {
            hints_map.insert(key, value);
        }

        // Add urgency hint if not present
        if !hints_map.contains_key("urgency") {
            hints_map.insert("urgency", "1"); // Normal urgency
        }

        // Convert hints to DBus variant format
        let mut dbus_hints = std::collections::HashMap::new();
        for (key, value) in hints_map {
            
            dbus_hints.insert(key, Value::Str(value.into()));
        }

        proxy
            .call::<_, _, ()>(
                "Notify",
                &(
                    app_name,
                    notification_id,
                    icon,
                    summary,
                    body,
                    actions,
                    dbus_hints,
                    timeout,
                ),
            )
            .await?;

        Ok(())
    }

    /// Send a simple notification
    pub async fn send_simple_notification(
        &self,
        summary: &str,
        body: &str,
        urgency: NotificationUrgency,
    ) -> Result<()> {
        let s = urgency.to_u8().to_string();
        let hints = vec![("urgency", s.as_str())];
        
        self.send_notification(
            "OneDrive Sync",
            0,
            "cloud-upload",
            summary,
            body,
            vec![],
            hints,
            5000, // 5 second timeout
        )
        .await
    }

    /// Send sync status notification
    pub async fn send_sync_status_notification(
        &self,
        status: &str,
        details: &str,
    ) -> Result<()> {
        let urgency = if status.contains("error") {
            NotificationUrgency::Critical
        } else if status.contains("paused") {
            NotificationUrgency::Low
        } else {
            NotificationUrgency::Normal
        };

        self.send_simple_notification(
            &format!("OneDrive Sync: {}", status),
            details,
            urgency,
        )
        .await
    }

    /// Send sync progress notification
    pub async fn send_sync_progress_notification(
        &self,
        current: u32,
        total: u32,
        message: &str,
    ) -> Result<()> {
        let progress_text = if total > 0 {
            let percentage = (current as f32 / total as f32 * 100.0) as u32;
            format!("{}% complete - {}", percentage, message)
        } else {
            message.to_string()
        };

        self.send_simple_notification(
            "OneDrive Sync Progress",
            &progress_text,
            NotificationUrgency::Normal,
        )
        .await
    }

    /// Send error notification
    pub async fn send_error_notification(&self, error: &str) -> Result<()> {
        self.send_simple_notification(
            "OneDrive Sync Error",
            error,
            NotificationUrgency::Critical,
        )
        .await
    }

    /// Send success notification
    pub async fn send_success_notification(&self, message: &str) -> Result<()> {
        self.send_simple_notification(
            "OneDrive Sync Success",
            message,
            NotificationUrgency::Normal,
        )
        .await
    }
} 