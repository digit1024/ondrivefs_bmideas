use anyhow::Result;
use log::{debug, error, info, warn};
use reqwest::Client;
use std::time::Duration;
use tokio::time::timeout;

/// Default timeout for connectivity checks
const DEFAULT_TIMEOUT_SECS: u64 = 10;

/// Internet connectivity test endpoints
const INTERNET_ENDPOINTS: &[&str] = &[
    "https://www.google.com",
    "https://www.cloudflare.com", 
    "https://www.microsoft.com",
];

/// Microsoft Graph API endpoints for connectivity testing
const GRAPH_ENDPOINTS: &[&str] = &[
    "https://graph.microsoft.com/v1.0/",
];

/// Connectivity status enumeration
#[derive(Debug, Clone, PartialEq)]
pub enum ConnectivityStatus {
    /// Full connectivity: Internet and MS Graph accessible
    Online,
    /// No internet connection available
    Offline,
    /// Internet available but MS Graph not accessible
    NotReachable,
    /// Internet available, MS Graph status uncertain
    Partial,
}

impl std::fmt::Display for ConnectivityStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConnectivityStatus::Online => write!(f, "🟢 Online"),
            ConnectivityStatus::Offline => write!(f, "🔴 Offline"),
            ConnectivityStatus::NotReachable => write!(f, "🟡 Not Reachable"),
            ConnectivityStatus::Partial => write!(f, "🟠 Partial"),
        }
    }
}

/// Network connectivity checker for OneDrive synchronization
pub struct ConnectivityChecker {
    http_client: Client,
    timeout_duration: Duration,
}

impl ConnectivityChecker {
    /// Create a new connectivity checker with default timeout
    pub fn new() -> Self {
        Self {
            http_client: Client::new(),
            timeout_duration: Duration::from_secs(DEFAULT_TIMEOUT_SECS),
        }
    }

    /// Create a connectivity checker with custom timeout
    pub fn with_timeout(timeout_secs: u64) -> Self {
        Self {
            http_client: Client::new(),
            timeout_duration: Duration::from_secs(timeout_secs),
        }
    }

    /// Check overall connectivity status
    pub async fn check_connectivity(&self) -> ConnectivityStatus {
        info!("🔍 Checking connectivity status...");

        // First check basic internet connectivity
        match self.check_internet_connectivity().await {
            Ok(true) => {
                info!("✅ Internet connectivity confirmed");
                self.check_ms_graph_connectivity().await
            }
            Ok(false) => {
                warn!("⚠️ No internet connectivity detected");
                ConnectivityStatus::Offline
            }
            Err(e) => {
                error!("❌ Error checking internet connectivity: {}", e);
                ConnectivityStatus::Offline
            }
        }
    }

    /// Check internet connectivity using multiple reliable endpoints
    async fn check_internet_connectivity(&self) -> Result<bool> {
        for endpoint in INTERNET_ENDPOINTS {
            if let Ok(true) = self.ping_endpoint(endpoint).await {
                debug!("✅ Internet connectivity confirmed via {}", endpoint);
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Check Microsoft Graph API connectivity
    async fn check_ms_graph_connectivity(&self) -> ConnectivityStatus {
        for endpoint in GRAPH_ENDPOINTS {
            if let Ok(true) = self.ping_endpoint(endpoint).await {
                debug!("✅ MS Graph connectivity confirmed via {}", endpoint);
                return ConnectivityStatus::Online;
            }
        }
        
        warn!("⚠️ Internet available but MS Graph not reachable");
        ConnectivityStatus::NotReachable
    }

    /// Ping a specific endpoint with timeout
    async fn ping_endpoint(&self, url: &str) -> Result<bool> {
        let request = self.http_client.get(url);
        
        match timeout(self.timeout_duration, request.send()).await {
            Ok(Ok(response)) => {
                let status = response.status();
                debug!("✅ Pinged {} successfully - status {}", url, status);
                Ok(status.is_success() || status.is_redirection())
            }
            Ok(Err(e)) => {
                debug!("❌ Failed to ping {}: {}", url, e);
                Ok(false)
            }
            Err(_) => {
                debug!("⏰ Timeout pinging {}", url);
                Ok(false)
            }
        }
    }

    /// Get detailed connectivity information with status and description
    pub async fn get_detailed_status(&self) -> (ConnectivityStatus, String) {
        let status = self.check_connectivity().await;
        let details = match status {
            ConnectivityStatus::Online => {
                "Full connectivity: Internet and MS Graph API accessible".to_string()
            }
            ConnectivityStatus::Offline => "No internet connection detected".to_string(),
            ConnectivityStatus::NotReachable => {
                "Internet available but MS Graph API not accessible".to_string()
            }
            ConnectivityStatus::Partial => {
                "Internet available but MS Graph API status uncertain".to_string()
            }
        };
        (status, details)
    }

    /// Check if the current connectivity status allows for OneDrive operations
    pub fn is_operational(&self, status: &ConnectivityStatus) -> bool {
        matches!(status, ConnectivityStatus::Online)
    }
}

impl Default for ConnectivityChecker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_connectivity_checker_creation() {
        let checker = ConnectivityChecker::new();
        assert_eq!(checker.timeout_duration, Duration::from_secs(DEFAULT_TIMEOUT_SECS));
    }

    #[tokio::test]
    async fn test_connectivity_checker_with_custom_timeout() {
        let checker = ConnectivityChecker::with_timeout(5);
        assert_eq!(checker.timeout_duration, Duration::from_secs(5));
    }

    #[test]
    fn test_connectivity_status_display() {
        assert_eq!(ConnectivityStatus::Online.to_string(), "🟢 Online");
        assert_eq!(ConnectivityStatus::Offline.to_string(), "🔴 Offline");
        assert_eq!(
            ConnectivityStatus::NotReachable.to_string(),
            "🟡 Not Reachable"
        );
        assert_eq!(ConnectivityStatus::Partial.to_string(), "🟠 Partial");
    }

    #[test]
    fn test_is_operational() {
        let checker = ConnectivityChecker::new();
        assert!(checker.is_operational(&ConnectivityStatus::Online));
        assert!(!checker.is_operational(&ConnectivityStatus::Offline));
        assert!(!checker.is_operational(&ConnectivityStatus::NotReachable));
        assert!(!checker.is_operational(&ConnectivityStatus::Partial));
    }
}
