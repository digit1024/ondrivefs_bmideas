use zbus::connection::Builder;
use zbus::Proxy;

const DBUS_SERVICE: &str = "org.freedesktop.OneDriveSync";
const DBUS_PATH: &str = "/org/freedesktop/OneDriveSync";
const DBUS_INTERFACE: &str = "org.freedesktop.OneDriveSync";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🔍 Testing OneDrive DBus pause functionality...");
    
    // Create DBus connection
    let connection = Builder::session()?.build().await?;
    let proxy = Proxy::new(&connection, DBUS_SERVICE, DBUS_PATH, DBUS_INTERFACE).await?;
    
    // Test 1: Get current daemon status
    println!("📊 Getting daemon status...");
    let status = proxy.call_method("GetDaemonStatus", &()).await?;
    let daemon_status: onedrive_sync_lib::dbus::types::DaemonStatus = status.body().deserialize()?;
    println!("✅ Daemon status: authenticated={}, connected={}, sync_status={:?}", 
             daemon_status.is_authenticated, daemon_status.is_connected, daemon_status.sync_status);
    
    // Test 2: Toggle sync pause
    println!("⏸️ Toggling sync pause...");
    let pause_result = proxy.call_method("ToggleSyncPause", &()).await?;
    let is_paused: bool = pause_result.body().deserialize()?;
    println!("✅ Sync pause toggled: {}", if is_paused { "paused" } else { "resumed" });
    
    // Test 3: Get status again to verify change
    println!("📊 Getting updated daemon status...");
    let status2 = proxy.call_method("GetDaemonStatus", &()).await?;
    let daemon_status2: onedrive_sync_lib::dbus::types::DaemonStatus = status2.body().deserialize()?;
    println!("✅ Updated daemon status: authenticated={}, connected={}, sync_status={:?}", 
             daemon_status2.is_authenticated, daemon_status2.is_connected, daemon_status2.sync_status);
    
    // Test 4: Toggle again to restore original state
    println!("⏸️ Toggling sync pause again...");
    let pause_result2 = proxy.call_method("ToggleSyncPause", &()).await?;
    let is_paused2: bool = pause_result2.body().deserialize()?;
    println!("✅ Sync pause toggled again: {}", if is_paused2 { "paused" } else { "resumed" });
    
    println!("🎉 All tests completed successfully!");
    Ok(())
} 