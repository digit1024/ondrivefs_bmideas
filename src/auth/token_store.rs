use anyhow::{Result, anyhow};
use keyring::Entry;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthConfig {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: u64,
}

pub struct TokenStore {
    keyring_entry: Option<Entry>,
    file_path: PathBuf,
}

impl TokenStore {
    pub fn new() -> Result<Self> {
        let keyring_entry = Self::create_keyring_entry();
        let file_path = Self::get_file_path()?;

        // Ensure the directory exists for file fallback
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent)?;
        }

        Ok(Self {
            keyring_entry,
            file_path,
        })
    }

    /// Create keyring entry if available
    fn create_keyring_entry() -> Option<Entry> {
        match Entry::new("onedrive-sync", "oauth_tokens") {
            Ok(entry) => Some(entry),
            Err(_) => None,
        }
    }

    /// Get the file path for fallback storage
    fn get_file_path() -> Result<PathBuf> {
        let mut path =
            dirs::home_dir().ok_or_else(|| anyhow!("Could not determine home directory"))?;
        path.push(".onedrive");
        path.push("auth");
        path.push("secrets.json");
        Ok(path)
    }

    /// Check if keyring is available and working
    fn is_keyring_available(&self) -> bool {
        //TODO: ON POP OS ALPHA 7 KEYRING IS NOT WORKING - NEED TO FIX THIS
        return false;
        // if let Some(ref entry) = self.keyring_entry {
        //     // Try to get a password for a dummy entry to test keyring availability
        //     match entry.get_password() {
        //         Ok(_) => true, // Key exists (unexpected, but keyring works)
        //         Err(keyring::Error::NoEntry) => true, // Keyring works, key doesn't exist
        //         Err(_) => false, // Keyring backend not available
        //     }
        // } else {
        //     false // No keyring entry created
        // }
    }

    /// Save tokens to storage (keyring if available, file otherwise)
    pub fn save_tokens(&self, tokens: &AuthConfig) -> Result<()> {
        let serialized = serde_json::to_string(tokens)?;

        if self.is_keyring_available() {
            // Save to keyring
            if let Some(ref entry) = self.keyring_entry {
                entry.set_password(&serialized)?;
                println!("Tokens saved to system keyring");
            }
        } else {
            // Save to file
            fs::write(&self.file_path, serialized)?;
            println!("Tokens saved to file: {:?}", self.file_path);
        }

        Ok(())
    }

    /// Load tokens from storage (keyring if available, file otherwise)
    pub fn load_tokens(&self) -> Result<AuthConfig> {
        if self.is_keyring_available() {
            // Load from keyring
            if let Some(ref entry) = self.keyring_entry {
                let stored = entry.get_password()?;
                let config: AuthConfig = serde_json::from_str(&stored)?;
                return Ok(config);
            }
        }

        // Load from file
        if self.file_path.exists() {
            let data = fs::read_to_string(&self.file_path)?;
            let config: AuthConfig = serde_json::from_str(&data)?;
            Ok(config)
        } else {
            Err(anyhow!(
                "No tokens found in keyring or file {}",
                self.file_path.display()
            ))
        }
    }

    /// Get storage method info for debugging
    pub fn get_storage_info(&self) -> String {
        if self.is_keyring_available() {
            "system keyring".to_string()
        } else {
            format!("file: {:?}", self.file_path)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_store_creation() {
        let store = TokenStore::new();
        assert!(store.is_ok());
    }

    #[test]
    fn test_file_path_creation() {
        let path = TokenStore::get_file_path();
        assert!(path.is_ok());
        let path = path.unwrap();
        assert!(
            path.to_string_lossy()
                .contains(".onedrive/auth/secrets.json")
        );
    }
}
