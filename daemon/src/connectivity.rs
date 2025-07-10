use std::time::Duration;
use tokio::time::timeout;
use reqwest::Client;
use anyhow::Result;
use log::{info, warn, error, debug};

#[derive(Debug, Clone, PartialEq)]
pub enum ConnectivityStatus {
    Online,           // Internet + MS Graph accessible
    Offline,          // No internet connection
    NotReachable,     // Internet available but MS Graph not accessible
    Partial,          // Internet available, MS Graph status unknown
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

pub struct ConnectivityChecker {
    http_client: Client,
    timeout_duration: Duration,
}

impl ConnectivityChecker {
    pub fn new() -> Self {
        Self {
            http_client: Client::new(),
            timeout_duration: Duration::from_secs(10),
        }
    }

    pub fn with_timeout(timeout_secs: u64) -> Self {
        Self {
            http_client: Client::new(),
            timeout_duration: Duration::from_secs(timeout_secs),
        }
    }

    pub async fn check_connectivity(&self) -> ConnectivityStatus {
        info!("🔍 Checking connectivity status...");
        
        // First check basic internet connectivity
        match self.check_internet_connectivity().await {
            Ok(true) => {
                info!("✅ Internet connectivity confirmed");
                // Internet is available, now check MS Graph
                match self.check_ms_graph_connectivity().await {
                    Ok(true) => {
                        info!("✅ MS Graph connectivity confirmed");
                        ConnectivityStatus::Online
                    }
                    Ok(false) => {
                        warn!("⚠️ Internet available but MS Graph not reachable");
                        ConnectivityStatus::NotReachable
                    }
                    Err(e) => {
                        error!("❌ Error checking MS Graph connectivity: {}", e);
                        ConnectivityStatus::Partial
                    }
                }
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

    async fn check_internet_connectivity(&self) -> Result<bool> {
        // Check multiple reliable endpoints
        let endpoints = vec![
            "https://www.google.com",
            "https://www.cloudflare.com", 
            "https://www.microsoft.com"
        ];

        for endpoint in endpoints {
            if let Ok(true) = self.ping_endpoint(endpoint).await {
                debug!("✅ Internet connectivity confirmed via {}", endpoint);
                return Ok(true);
            }
        }
        Ok(false)
    }

    async fn check_ms_graph_connectivity(&self) -> Result<bool> {
        // Check MS Graph API specifically
        let graph_endpoints = vec![
            
            "https://graph.microsoft.com/v1.0/"
        ];

        for endpoint in graph_endpoints {
            if let Ok(true) = self.ping_endpoint(endpoint).await {
                debug!("✅ MS Graph connectivity confirmed via {}", endpoint);
                return Ok(true);
            }
        }
        Ok(false)
    }

    async fn ping_endpoint(&self, url: &str) -> Result<bool> {
        match timeout(self.timeout_duration, self.http_client.get(url).send()).await {
            Ok(Ok(response)) => {
                debug!("✅ Pinged {} successfully - status {}", url, response.status());
                Ok(response.status().is_success() || response.status().is_redirection())
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

    /// Get detailed connectivity information
    pub async fn get_detailed_status(&self) -> (ConnectivityStatus, String) {
        let status = self.check_connectivity().await;
        let details = match status {
            ConnectivityStatus::Online => "Full connectivity: Internet and MS Graph API accessible".to_string(),
            ConnectivityStatus::Offline => "No internet connection detected".to_string(),
            ConnectivityStatus::NotReachable => "Internet available but MS Graph API not accessible".to_string(),
            ConnectivityStatus::Partial => "Internet available but MS Graph API status uncertain".to_string(),
        };
        (status, details)
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
        assert_eq!(checker.timeout_duration, Duration::from_secs(10));
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
        assert_eq!(ConnectivityStatus::NotReachable.to_string(), "🟡 Not Reachable");
        assert_eq!(ConnectivityStatus::Partial.to_string(), "�� Partial");
    }
} 