# Authentication System

## Overview

The authentication system implements OAuth2 with PKCE (Proof Key for Code Exchange) for secure OneDrive access, providing automatic token refresh and secure token storage.

## OAuth2 Flow

### Authorization Flow
**File**: `auth/onedrive_auth.rs`

#### 1. PKCE Code Generation
```rust
fn generate_pkce() -> (String, String) {
    let code_verifier: String = (0..PKCE_CODE_VERIFIER_LENGTH)
        .map(|_| PKCE_CHARS[rand::rng().random_range(0..PKCE_CHARS.len())] as char)
        .collect();

    let mut hasher = Sha256::new();
    hasher.update(code_verifier.as_bytes());
    let code_challenge = URL_SAFE_NO_PAD.encode(hasher.finalize());

    (code_verifier, code_challenge)
}
```

**PKCE Parameters**:
- **Code Verifier**: 128-character random string
- **Code Challenge**: SHA256 hash of verifier (Base64URL encoded)
- **Character Set**: A-Z, a-z, 0-9, -, ., _, ~

#### 2. Authorization URL Construction
```rust
fn build_auth_url(&self, code_challenge: &str) -> Result<Url> {
    let mut url = Url::parse(AUTH_URL)?;
    url.query_pairs_mut()
        .append_pair("client_id", CLIENT_ID)
        .append_pair("response_type", "code")
        .append_pair("redirect_uri", REDIRECT_URI)
        .append_pair("scope", SCOPES)
        .append_pair("code_challenge", code_challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("response_mode", "query");

    Ok(url)
}
```

**Authorization Parameters**:
- **Client ID**: Azure application identifier
- **Response Type**: `code` for authorization code flow
- **Redirect URI**: `http://localhost:8080/callback`
- **Scopes**: OneDrive permissions
- **Code Challenge**: PKCE challenge
- **Code Challenge Method**: `S256` (SHA256)

#### 3. Local Server Setup
```rust
pub async fn authorize(&self) -> Result<AuthConfig> {
    let (code_verifier, code_challenge) = Self::generate_pkce();
    let auth_url = self.build_auth_url(&code_challenge)?;
    
    // Open browser for user authentication
    webbrowser::open(auth_url.as_str())?;
    
    // Start local server for callback
    let server = Server::http(format!("{}:{}", CALLBACK_ADDRESS, CALLBACK_PORT))?;
    
    // Wait for authorization callback
    let auth_code = self.wait_for_callback(server, &code_verifier).await?;
    
    Ok(auth_code)
}
```

**Local Server Configuration**:
- **Address**: `127.0.0.1:8080`
- **Callback Path**: `/callback`
- **Timeout**: User interaction timeout

### Token Exchange

#### 1. Authorization Code to Token
```rust
async fn exchange_code_for_token(&self, auth_code: &str, code_verifier: &str) -> Result<TokenResponse> {
    let token_url = Url::parse(TOKEN_URL)?;
    
    let params = [
        ("client_id", CLIENT_ID),
        ("grant_type", "authorization_code"),
        ("code", auth_code),
        ("redirect_uri", REDIRECT_URI),
        ("code_verifier", code_verifier),
    ];
    
    let response = self.client.post(token_url)
        .form(&params)
        .send()
        .await?;
    
    let token_response: TokenResponse = response.json().await?;
    Ok(token_response)
}
```

**Token Request Parameters**:
- **Client ID**: Azure application identifier
- **Grant Type**: `authorization_code`
- **Code**: Authorization code from callback
- **Redirect URI**: Must match authorization request
- **Code Verifier**: Original PKCE verifier

#### 2. Token Response Processing
```rust
#[derive(Debug, Serialize, Deserialize)]
struct TokenResponse {
    access_token: String,      // Access token for API calls
    refresh_token: Option<String>, // Refresh token for renewal
    expires_in: u64,           // Token lifetime in seconds
    token_type: String,        // Token type (Bearer)
}
```

## Token Management

### TokenStore
**File**: `auth/token_store.rs`

Secure token storage:

```rust
pub struct TokenStore {
    config_path: PathBuf,
    encryption_key: [u8; 32],
}
```

**Security Features**:
- **Encrypted Storage**: AES-256 encryption for sensitive data
- **Secure Key Derivation**: PBKDF2 key derivation
- **File Permissions**: Restricted file access (600)

### Token Refresh

#### Automatic Refresh
```rust
pub async fn get_valid_token(&self) -> Result<String> {
    let config = self.load_config().await?;
    
    // Check if token is expired or near expiry
    if self.is_token_expired(&config) || self.should_refresh_token(&config) {
        let new_config = self.refresh_token(&config).await?;
        self.store_config(&new_config).await?;
        Ok(new_config.access_token)
    } else {
        Ok(config.access_token)
    }
}
```

**Refresh Logic**:
- **Expiry Check**: Token expired
- **Buffer Time**: Refresh 5 minutes before expiry
- **Automatic Renewal**: Transparent to application

#### Refresh Token Flow
```rust
async fn refresh_token(&self, config: &AuthConfig) -> Result<AuthConfig> {
    let refresh_token = config.refresh_token
        .as_ref()
        .ok_or_else(|| anyhow!("No refresh token available"))?;
    
    let token_url = Url::parse(TOKEN_URL)?;
    let params = [
        ("client_id", CLIENT_ID),
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh_token),
    ];
    
    let response = self.client.post(token_url)
        .form(&params)
        .send()
        .await?;
    
    let token_response: TokenResponse = response.json().await?;
    
    Ok(AuthConfig {
        access_token: token_response.access_token,
        refresh_token: token_response.refresh_token.unwrap_or(refresh_token.clone()),
        expires_at: self.calculate_expiry(token_response.expires_in),
    })
}
```

## Configuration

### OAuth2 Settings
**File**: `auth/onedrive_auth.rs`

```rust
// Azure application client ID
const CLIENT_ID: &str = "95367b4f-624c-452c-b099-bfc9c27b69b9";

// OAuth redirect URI
const REDIRECT_URI: &str = "http://localhost:8080/callback";

// OAuth scopes for OneDrive access
const SCOPES: &str = " https://graph.microsoft.com/User.Read https://graph.microsoft.com/Files.ReadWrite openid profile email offline_access";

// Microsoft OAuth URLs
const AUTH_URL: &str = "https://login.microsoftonline.com/common/oauth2/v2.0/authorize";
const TOKEN_URL: &str = "https://login.microsoftonline.com/common/oauth2/v2.0/token";

// Local server configuration
const CALLBACK_PORT: u16 = 8080;
const CALLBACK_ADDRESS: &str = "127.0.0.1";

// Token refresh configuration
const TOKEN_REFRESH_BUFFER_SECS: u64 = 300; // 5 minutes
```

### Required Scopes
- **User.Read**: Access to user profile information
- **Files.ReadWrite**: Full file access (read/write/delete)
- **openid**: OpenID Connect authentication
- **profile**: Basic profile information
- **email**: Email address access
- **offline_access**: Refresh token for offline use

## Security Features

### PKCE Implementation
- **Code Verifier**: Cryptographically secure random string
- **Code Challenge**: SHA256 hash of verifier
- **Challenge Method**: S256 (SHA256 with Base64URL encoding)

### Token Security
- **Encrypted Storage**: AES-256 encryption for tokens
- **Secure Key Derivation**: PBKDF2 with high iteration count
- **File Permissions**: Restricted access (600)
- **Memory Protection**: Secure memory handling

### Network Security
- **HTTPS Only**: All OAuth endpoints use HTTPS
- **Local Callback**: Callback only from localhost
- **Token Validation**: Validate token format and expiry

## Error Handling

### Authentication Errors
1. **User Cancellation**: User aborts authentication
2. **Network Failures**: Connection issues during auth
3. **Invalid Response**: Malformed OAuth response
4. **Token Errors**: Invalid or expired tokens

### Recovery Mechanisms
- **Automatic Retry**: Retry failed authentication
- **Token Refresh**: Automatic token renewal
- **User Notification**: Clear error messages
- **Fallback Options**: Alternative authentication methods

### Error Logging
```rust
match auth.load_tokens() {
    Ok(_) => {
        info!("✅ Existing tokens loaded successfully");
        Ok(())
    }
    Err(_) => {
        info!("🔑 No valid tokens found, starting authorization flow...");
        auth.authorize().await.context("Authorization failed")?;
        
        auth.load_tokens()
            .context("Failed to load tokens after authorization")?;
        
        info!("✅ Authentication completed successfully");
        Ok(())
    }
}
```

## Integration Points

### OneDriveClient Integration
**File**: `onedrive_service/onedrive_client.rs`

```rust
impl OneDriveClient {
    async fn get_authorization_header(&self) -> Result<String> {
        let token = self.auth.get_valid_token().await?;
        Ok(format!("Bearer {}", token))
    }
}
```

### AppState Integration
**File**: `app_state.rs`

```rust
impl AppState {
    pub async fn new() -> Result<Self> {
        // Initialize authentication
        let auth = OneDriveAuth::new()
            .await
            .context("Failed to create authentication manager")?;
        
        // Initialize OneDrive client with auth
        let onedrive_client = OneDriveClient::new(auth.clone())
            .context("Failed to create OneDrive client")?;
        
        Ok(Self {
            auth: Arc::new(auth),
            onedrive_client: Arc::new(onedrive_client),
            // ... other fields
        })
    }
}
```

## User Experience

### Authentication Flow
1. **Initial Launch**: Check for existing tokens
2. **Token Missing**: Open browser for authentication
3. **User Authentication**: Microsoft login page
4. **Authorization**: User grants permissions
5. **Callback Processing**: Local server receives code
6. **Token Exchange**: Convert code to tokens
7. **Storage**: Securely store tokens
8. **Ready**: Application ready for OneDrive access

### Re-authentication
- **Token Expiry**: Automatic refresh when possible
- **Refresh Failure**: Prompt user to re-authenticate
- **Seamless Operation**: Minimal user interruption

## Debugging & Troubleshooting

### Common Issues
1. **Port Conflicts**: Port 8080 already in use
2. **Firewall Blocking**: Local server blocked
3. **Browser Issues**: Authentication page problems
4. **Token Corruption**: Stored token issues

### Debug Tools
```bash
# Enable authentication debug logging
RUST_LOG=debug onedrive-daemon

# Check token storage
ls -la ~/.local/share/onedrive-sync/

# Test local server
curl http://localhost:8080/callback
```

### Log Analysis
- **Authentication Flow**: Step-by-step logging
- **Token Operations**: Token refresh and validation
- **Error Details**: Comprehensive error information
- **Network Issues**: Connection and timeout problems
