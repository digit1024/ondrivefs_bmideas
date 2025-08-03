// SPDX-License-Identifier: MPL-2.0

use anyhow::Result;
use log::info;
use onedrive_sync_lib::dbus::types::DaemonStatus;
use zbus::connection::Builder;
use zbus::Proxy;

const DBUS_SERVICE: &str = "org.freedesktop.OneDriveSync";
const DBUS_PATH: &str = "/org/freedesktop/OneDriveSync";
const DBUS_INTERFACE: &str = "org.freedesktop.OneDriveSync";

pub async fn with_dbus_client<CallFn, CallFuture, Output, CallError>(
    call: CallFn,
) -> Result<Output, String>
where
    CallFn: FnOnce(DbusClient) -> CallFuture,
    CallFuture: std::future::Future<Output = Result<Output, CallError>>,
    CallError: std::fmt::Display,
{
    match DbusClient::new().await {
        Ok(client) => {
            info!("Dbus client created successfully");
            match call(client).await {
                Ok(val) => Ok(val),
                Err(e) => Err(format!("{}", e)),
            }
        }
        Err(e) => Err(format!("Failed to connect to daemon: {}", e)),
    }
}

/// DBus client for communicating with the OneDrive sync daemon
pub struct DbusClient {
    connection: zbus::Connection,
}

impl DbusClient {
    /// Create a new DBus client instance
    pub async fn new() -> Result<Self> {
        info!("Creating new DBus client instance");
        let connection = Builder::session()?.build().await?;

        info!("DBus client created successfully");
        Ok(Self { connection })
    }

    /// Get the daemon status
    pub async fn get_daemon_status(&self) -> Result<DaemonStatus> {
        info!("Fetching daemon status");
        let proxy = self.get_proxy().await?;

        let status = proxy
            .call_method("GetDaemonStatus", &())
            .await?
            .body()
            .deserialize::<DaemonStatus>()?;

        info!(
            "Successfully fetched daemon status: authenticated={}, connected={}, sync_status={:?}",
            status.is_authenticated, status.is_connected, status.sync_status
        );
        Ok(status)
    }

    async fn get_proxy(&self) -> Result<Proxy<'_>, anyhow::Error> {
        let proxy = Proxy::new(&self.connection, DBUS_SERVICE, DBUS_PATH, DBUS_INTERFACE).await?;
        Ok(proxy)
    }
}
