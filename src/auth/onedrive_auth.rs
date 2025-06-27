

use anyhow::{anyhow, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use rand::Rng;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};
use tiny_http::{Server, Response, Header};
use url::Url;

use crate::auth::token_store::{TokenStore, AuthConfig};

const CLIENT_ID: &str = "95367b4f-624c-452c-b099-bfc9c27b69b9"; // Replace with your Azure app ID
const REDIRECT_URI: &str = "http://localhost:8080/callback";
const SCOPES: &str = "https://graph.microsoft.com/Files.ReadWrite offline_access";
const AUTH_URL: &str = "https://login.microsoftonline.com/common/oauth2/v2.0/authorize";
const TOKEN_URL: &str = "https://login.microsoftonline.com/common/oauth2/v2.0/token";

#[derive(Debug, Serialize, Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: u64,
    token_type: String,
}

pub struct OneDriveAuth {
    client: Client,
    token_store: TokenStore,
}

impl OneDriveAuth {
    pub fn new() -> Result<Self> {
        Ok(Self {
            client: Client::new(),
            token_store: TokenStore::new()?,
        })
    }

    /// Generate PKCE code verifier and challenge
    fn generate_pkce() -> (String, String) {
        let code_verifier: String = (0..128)
            .map(|_| {
                let chars = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~";
                chars[rand::thread_rng().gen_range(0..chars.len())] as char
            })
            .collect();

        let mut hasher = Sha256::new();
        hasher.update(code_verifier.as_bytes());
        let code_challenge = URL_SAFE_NO_PAD.encode(hasher.finalize());

        (code_verifier, code_challenge)
    }

    /// Start the OAuth flow
    pub async fn authorize(&self) -> Result<AuthConfig> {
        let (code_verifier, code_challenge) = Self::generate_pkce();
        
        // Build authorization URL
        let mut auth_url = Url::parse(AUTH_URL)?;
        auth_url.query_pairs_mut()
            .append_pair("client_id", CLIENT_ID)
            .append_pair("response_type", "code")
            .append_pair("redirect_uri", REDIRECT_URI)
            .append_pair("scope", SCOPES)
            .append_pair("code_challenge", &code_challenge)
            .append_pair("code_challenge_method", "S256")
            .append_pair("response_mode", "query");

        println!("Opening browser for authentication...");
        webbrowser::open(auth_url.as_str())?;

        // Start local server to receive callback
        let server = Server::http("127.0.0.1:8080")
            .map_err(|e| anyhow!("Failed to start local server: {}", e))?;
        
        println!("Waiting for authorization callback...");
        
        for request in server.incoming_requests() {
            let url = format!("http://localhost:8080{}", request.url());
            let parsed_url = Url::parse(&url)?;
            
            if let Some(code) = parsed_url.query_pairs()
                .find(|(key, _)| key == "code")
                .map(|(_, value)| value.to_string()) 
            {
                // Send success response to browser
                let response = Response::from_string("Authorization successful! You can close this window.")
                    .with_header(Header::from_bytes(&b"Content-Type"[..], &b"text/html"[..]).unwrap());
                let _ = request.respond(response);
                
                // Exchange code for tokens
                let tokens = self.exchange_code_for_tokens(&code, &code_verifier).await?;
                let config = self.save_tokens(tokens)?;
                return Ok(config);
            }
            
            if parsed_url.query_pairs().any(|(key, _)| key == "error") {
                let response = Response::from_string("Authorization failed!")
                    .with_header(Header::from_bytes(&b"Content-Type"[..], &b"text/html"[..]).unwrap());
                let _ = request.respond(response);
                return Err(anyhow!("Authorization was denied or failed"));
            }
        }
        
        Err(anyhow!("Authorization flow incomplete"))
    }

    /// Exchange authorization code for tokens
    async fn exchange_code_for_tokens(&self, code: &str, code_verifier: &str) -> Result<TokenResponse> {
        let mut params = HashMap::new();
        params.insert("client_id", CLIENT_ID);
        params.insert("code", code);
        params.insert("redirect_uri", REDIRECT_URI);
        params.insert("grant_type", "authorization_code");
        params.insert("code_verifier", code_verifier);

        let response = self.client
            .post(TOKEN_URL)
            .form(&params)
            .send()
            .await?;

        if !response.status().is_success() {
            let error_text = response.text().await?;
            return Err(anyhow!("Token exchange failed: {}", error_text));
        }

        let token_response: TokenResponse = response.json().await?;
        Ok(token_response)
    }

    /// Save tokens to storage
    fn save_tokens(&self, tokens: TokenResponse) -> Result<AuthConfig> {
        let expires_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)?
            .as_secs() + tokens.expires_in;

        let config = AuthConfig {
            access_token: tokens.access_token,
            refresh_token: tokens.refresh_token.unwrap_or_default(),
            expires_at,
        };

        self.token_store.save_tokens(&config)?;
        println!("Tokens saved successfully using: {}", self.token_store.get_storage_info());
        Ok(config)
    }

    /// Load tokens from storage
    pub fn load_tokens(&self) -> Result<AuthConfig> {
        self.token_store.load_tokens()
    }

    /// Check if token is expired
    pub fn is_token_expired(&self, config: &AuthConfig) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        now >= config.expires_at - 300 // Refresh 5 minutes before expiry
    }

    /// Refresh access token
    pub async fn refresh_token(&self, refresh_token: &str) -> Result<AuthConfig> {
        let mut params = HashMap::new();
        params.insert("client_id", CLIENT_ID);
        params.insert("refresh_token", refresh_token);
        params.insert("grant_type", "refresh_token");

        let response = self.client
            .post(TOKEN_URL)
            .form(&params)
            .send()
            .await?;

        if !response.status().is_success() {
            let error_text = response.text().await?;
            return Err(anyhow!("Token refresh failed: {}", error_text));
        }

        let token_response: TokenResponse = response.json().await?;
        self.save_tokens(token_response)
    }

    /// Get valid access token (refresh if needed)
    pub async fn get_valid_token(&self) -> Result<String> {
        let config = self.load_tokens()?;
        
        if self.is_token_expired(&config) {
            println!("Token expired, refreshing...");
            let new_config = self.refresh_token(&config.refresh_token).await?;
            Ok(new_config.access_token)
        } else {
            Ok(config.access_token)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pkce_generation() {
        let (verifier, challenge) = OneDriveAuth::generate_pkce();
        assert_eq!(verifier.len(), 128);
        assert!(!challenge.is_empty());
    }
}