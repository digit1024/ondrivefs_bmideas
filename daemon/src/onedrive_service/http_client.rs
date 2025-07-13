use anyhow::{Context, Result};
use log::debug;
use reqwest::Client;
use serde::Serialize;

const GRAPH_API_BASE: &str = "https://graph.microsoft.com/v1.0";

/// HTTP client for Microsoft Graph API operations
#[derive(Clone)]
pub struct HttpClient {
    client: Client,
}

impl HttpClient {
    pub fn new() -> Self {
        Self {
            client: Client::new(),
        }
    }

    /// Get full URL by prepending Graph API base if needed
    pub fn get_full_url(&self, url: &str) -> Result<String> {
        if url.starts_with("http") {
            Ok(url.to_string())
        } else {
            Ok(format!("{}{}", GRAPH_API_BASE, url))
        }
    }

    /// Make a GET request with authorization header
    pub async fn get<T>(&self, url: &str, auth_header: &str) -> Result<T>
    where
        T: Serialize + serde::de::DeserializeOwned + std::fmt::Debug,
    {
        let url = self.get_full_url(url)?;
        debug!("Getting url: {}", url);

        let response = self
            .client
            .get(&url)
            .header("Authorization", auth_header)
            .send()
            .await
            .context("Failed to get response")?
            .error_for_status()
            .context("Not a success status")?;

        let response_json = response
            .json::<T>()
            .await
            .context("Failed to deserialize response to type T")?;
        Ok(response_json)
    }

    /// Make a POST request with authorization header
    #[allow(dead_code)]
    pub async fn post<T, B>(&self, url: &str, body: &B, auth_header: &str) -> Result<T>
    where
        T: Serialize + serde::de::DeserializeOwned + std::fmt::Debug,
        B: Serialize,
    {
        let url = self.get_full_url(url)?;
        let response = self
            .client
            .post(&url)
            .header("Authorization", auth_header)
            .header("Content-Type", "application/json")
            .json(body)
            .send()
            .await
            .context("Failed to get response for post")?
            .error_for_status()
            .context("Not a success status")?
            .json::<T>()
            .await
            .context("Failed to deserialize response to type T")?;
        Ok(response)
    }

    /// Make a DELETE request with authorization header
    #[allow(dead_code)]
    pub async fn delete(&self, url: &str, auth_header: &str) -> Result<()> {
        let url = self.get_full_url(url)?;
        self.client
            .delete(&url)
            .header("Authorization", auth_header)
            .send()
            .await
            .context("Failed to get response for delete")?
            .error_for_status()
            .context("Not a success status")?;
        Ok(())
    }

    /// Make a PUT request with authorization header
    #[allow(dead_code)]
    pub async fn put<T>(&self, url: &str, body: &T, auth_header: &str) -> Result<T>
    where
        T: Serialize + serde::de::DeserializeOwned + std::fmt::Debug,
    {
        let url = self.get_full_url(url)?;
        let response = self
            .client
            .put(&url)
            .header("Authorization", auth_header)
            .header("Content-Type", "application/json")
            .json(body)
            .send()
            .await
            .context("Failed to get response for put")?
            .error_for_status()
            .context("Not a success status")?
            .json::<T>()
            .await
            .context("Failed to deserialize response to type T")?;
        Ok(response)
    }
    #[allow(dead_code)]
    /// Download file content with custom headers
    pub async fn download_file(&self, download_url: &str) -> Result<reqwest::Response> {
        let response = self
            .client
            .get(download_url)
            .send()
            .await
            .context("Failed to get response for download")?
            .error_for_status()
            .context("Not a success status")?;

        Ok(response)
    }

    /// Get a request builder for custom HTTP requests
    pub fn request_builder(&self, method: &str, url: &str) -> reqwest::RequestBuilder {
        match method.to_uppercase().as_str() {
            "GET" => self.client.get(url),
            "POST" => self.client.post(url),
            "PUT" => self.client.put(url),
            "DELETE" => self.client.delete(url),
            _ => self.client.get(url), // Default to GET
        }
    }

    /// Upload file content with authorization header
    #[allow(dead_code)]
    pub async fn upload_file(
        &self,
        url: &str,
        file_data: &[u8],
        auth_header: &str,
    ) -> Result<reqwest::Response> {
        let url = self.get_full_url(url)?;
        let response = self
            .client
            .put(&url)
            .header("Authorization", auth_header)
            .header("Content-Type", "application/octet-stream")
            .body(file_data.to_vec())
            .send()
            .await?;

        Ok(response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_full_url_with_relative_path() {
        let client = HttpClient::new();
        let result = client.get_full_url("/me/drive/root").unwrap();
        assert_eq!(result, "https://graph.microsoft.com/v1.0/me/drive/root");
    }

    #[test]
    fn test_get_full_url_with_absolute_url() {
        let client = HttpClient::new();
        let full_url = "https://example.com/api/test";
        let result = client.get_full_url(full_url).unwrap();
        assert_eq!(result, full_url);
    }

    #[test]
    fn test_get_full_url_with_http_url() {
        let client = HttpClient::new();
        let http_url = "http://example.com/api/test";
        let result = client.get_full_url(http_url).unwrap();
        assert_eq!(result, http_url);
    }

    #[test]
    fn test_graph_api_base_constant() {
        assert_eq!(GRAPH_API_BASE, "https://graph.microsoft.com/v1.0");
    }
}
