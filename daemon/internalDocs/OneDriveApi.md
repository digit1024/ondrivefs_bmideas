# OneDrive API Integration

## Overview

The OneDrive API integration provides access to Microsoft Graph API for OneDrive operations, including file management, synchronization, and user profile access.

## Core Components

### OneDriveClient
**File**: `onedrive_service/onedrive_client.rs`

Main API client implementation:

```rust
pub struct OneDriveClient {
    auth: Arc<OneDriveAuth>,
    http_client: Arc<HttpClient>,
    base_url: String,
}
```

**Key Responsibilities**:
- HTTP request management
- Authentication token handling
- API response processing
- Error handling and retry logic

### HttpClient
**File**: `onedrive_service/http_client.rs`

HTTP request handling:

```rust
pub struct HttpClient {
    client: reqwest::Client,
    timeout: Duration,
    max_retries: u32,
}
```

**Features**:
- **Connection Pooling**: Efficient HTTP connection reuse
- **Retry Logic**: Automatic retry for transient failures
- **Timeout Handling**: Configurable request timeouts
- **Error Mapping**: HTTP error to application error conversion

## API Models

### DriveItem
**File**: `onedrive_service/onedrive_models.rs`

Core OneDrive item representation:

```rust
pub struct DriveItem {
    pub id: String,                                    // Unique identifier
    pub name: Option<String>,                          // Item name
    pub etag: Option<String>,                          // ETag for concurrency
    pub last_modified: Option<String>,                 // Last modification time
    pub created_date: Option<String>,                  // Creation time
    pub size: Option<u64>,                            // File size in bytes
    pub folder: Option<FolderFacet>,                   // Folder metadata
    pub file: Option<FileFacet>,                      // File metadata
    pub download_url: Option<String>,                  // Download URL
    pub deleted: Option<DeletedFacet>,                 // Deletion status
    pub parent_reference: Option<ParentReference>,     // Parent location
}
```

### ParentReference
**File**: `onedrive_service/onedrive_models.rs`

Parent item reference:

```rust
pub struct ParentReference {
    pub id: String,                    // Parent item ID
    pub path: Option<String>,          // Parent path
}
```

### DeltaResponse
**File**: `onedrive_service/onedrive_models.rs`

Delta synchronization response:

```rust
pub struct DeltaResponse {
    pub next_link: Option<String>,     // Pagination link
    pub delta_link: Option<String>,    // Delta synchronization link
    pub items: Option<Vec<DriveItem>>, // Changed items
}
```

## API Operations

### File Operations

#### Get Item
```rust
pub async fn get_drive_item(&self, item_id: &str) -> Result<DriveItem> {
    let url = format!("{}/me/drive/items/{}", self.base_url, item_id);
    let response = self.http_client.get(&url).await?;
    Ok(response.json().await?)
}
```

#### List Children
```rust
pub async fn list_children(&self, parent_id: &str) -> Result<Vec<DriveItem>> {
    let url = format!("{}/me/drive/items/{}/children", self.base_url, parent_id);
    let response = self.http_client.get(&url).await?;
    Ok(response.json().await?)
}
```

#### Create Item
```rust
pub async fn create_item(&self, parent_id: &str, name: &str, is_folder: bool) -> Result<DriveItem> {
    let url = format!("{}/me/drive/items/{}/children", self.base_url, parent_id);
    let body = CreateItemRequest { name, folder: is_folder.then(|| FolderFacet::default()) };
    let response = self.http_client.post(&url, &body).await?;
    Ok(response.json().await?)
}
```

#### Update Item
```rust
pub async fn update_item(&self, item_id: &str, updates: &UpdateItemRequest) -> Result<DriveItem> {
    let url = format!("{}/me/drive/items/{}", self.base_url, item_id);
    let response = self.http_client.patch(&url, updates).await?;
    Ok(response.json().await?)
}
```

#### Delete Item
```rust
pub async fn delete_item(&self, item_id: &str) -> Result<()> {
    let url = format!("{}/me/drive/items/{}", self.base_url, item_id);
    self.http_client.delete(&url).await?;
    Ok(())
}
```

### Delta Synchronization

#### Get Delta Changes
```rust
pub async fn get_delta_changes(&self, delta_link: &str) -> Result<DeltaResponse> {
    let response = self.http_client.get(delta_link).await?;
    Ok(response.json().await?)
}
```

**Benefits**:
- **Incremental Updates**: Only changed items returned
- **Efficiency**: Reduced data transfer
- **Consistency**: Maintains sync state

#### Delta Link Management
```rust
pub async fn get_delta_link(&self) -> Result<String> {
    let url = format!("{}/me/drive/root/delta", self.base_url);
    let response = self.http_client.get(&url).await?;
    let delta_response: DeltaResponse = response.json().await?;
    Ok(delta_response.delta_link.unwrap_or_default())
}
```

### File Content Operations

#### Download File
```rust
pub async fn download_file(&self, item_id: &str) -> Result<Vec<u8>> {
    let item = self.get_drive_item(item_id).await?;
    let download_url = item.download_url.ok_or_else(|| anyhow!("No download URL"))?;
    
    let response = self.http_client.get(&download_url).await?;
    Ok(response.bytes().await?.to_vec())
}
```

#### Upload File
```rust
pub async fn upload_file(&self, parent_id: &str, name: &str, content: &[u8]) -> Result<DriveItem> {
    let url = format!("{}/me/drive/items/{}/children", self.base_url, parent_id);
    let body = CreateItemRequest { name, file: Some(FileFacet::default()) };
    
    // Create item first
    let item = self.http_client.post(&url, &body).await?.json().await?;
    
    // Upload content
    let upload_url = format!("{}/me/drive/items/{}/content", self.base_url, item.id);
    let response = self.http_client.put(&upload_url, content).await?;
    Ok(response.json().await?)
}
```

### User Profile Operations

#### Get User Profile
```rust
pub async fn get_user_profile(&self) -> Result<UserProfile> {
    let url = format!("{}/me", self.base_url);
    let response = self.http_client.get(&url).await?;
    Ok(response.json().await?)
}
```

## Authentication Integration

### Token Management
**File**: `onedrive_service/onedrive_client.rs`

```rust
impl OneDriveClient {
    async fn get_authorization_header(&self) -> Result<String> {
        let token = self.auth.get_valid_token().await?;
        Ok(format!("Bearer {}", token))
    }
    
    async fn make_authenticated_request(&self, request: RequestBuilder) -> Result<Response> {
        let auth_header = self.get_authorization_header().await?;
        let response = request.header("Authorization", auth_header).send().await?;
        
        if response.status().is_success() {
            Ok(response)
        } else {
            Err(anyhow!("API request failed: {}", response.status()))
        }
    }
}
```

### Token Refresh
**File**: `auth/onedrive_auth.rs`

Automatic token refresh:
- **Expiry Monitoring**: Track token expiration
- **Background Refresh**: Refresh before expiry
- **Seamless Operation**: Transparent to API operations

## Error Handling

### API Error Types
1. **Authentication Errors**: Invalid/expired tokens
2. **Rate Limiting**: API quota exceeded
3. **Network Errors**: Connection failures
4. **API Errors**: OneDrive service errors
5. **Validation Errors**: Invalid request data

### Retry Logic
**File**: `onedrive_service/http_client.rs`

```rust
impl HttpClient {
    async fn execute_with_retry<F, T>(&self, operation: F) -> Result<T>
    where
        F: Fn() -> Future<Output = Result<T>> + Send + Sync,
    {
        let mut attempts = 0;
        loop {
            match operation().await {
                Ok(result) => return Ok(result),
                Err(e) if self.should_retry(&e, attempts) => {
                    attempts += 1;
                    let delay = self.calculate_backoff(attempts);
                    tokio::time::sleep(delay).await;
                }
                Err(e) => return Err(e),
            }
        }
    }
}
```

### Error Mapping
- **HTTP 401**: Authentication required
- **HTTP 403**: Access denied
- **HTTP 404**: Item not found
- **HTTP 429**: Rate limited
- **HTTP 5xx**: Server errors

## Performance Optimizations

### Connection Pooling
- **Persistent Connections**: Reuse HTTP connections
- **Connection Limits**: Configurable connection pool size
- **Keep-Alive**: Maintain connections for multiple requests

### Caching Strategy
- **Response Caching**: Cache API responses
- **ETag Support**: Conditional requests for efficiency
- **Delta Sync**: Minimize data transfer

### Batch Operations
- **Bulk Operations**: Group multiple operations
- **Parallel Requests**: Concurrent API calls
- **Request Batching**: Combine related requests

## Configuration

### API Endpoints
- **Base URL**: `https://graph.microsoft.com/v1.0`
- **Drive API**: `/me/drive`
- **Delta API**: `/me/drive/root/delta`
- **User API**: `/me`

### Request Limits
- **Rate Limiting**: Respect API quotas
- **Timeout Settings**: Configurable request timeouts
- **Retry Configuration**: Retry attempts and backoff

### Authentication Settings
- **Token Refresh**: Automatic refresh before expiry
- **Scope Management**: Required API permissions
- **Error Handling**: Authentication failure recovery

## Monitoring & Debugging

### Request Logging
- **API Calls**: Log all API requests
- **Response Times**: Track request performance
- **Error Rates**: Monitor failure patterns

### Metrics Collection
- **Success Rate**: API call success percentage
- **Response Times**: Average response times
- **Error Distribution**: Error type breakdown

### Debug Tools
```bash
# Enable debug logging
RUST_LOG=debug onedrive-daemon

# Monitor API requests
tail -f /var/log/onedrive-daemon.log | grep "API"
```

## Future Enhancements

### Planned Features
- **GraphQL Support**: More efficient data queries
- **WebSocket Integration**: Real-time updates
- **Advanced Caching**: Intelligent cache invalidation
- **Metrics Dashboard**: Performance monitoring UI

### API Versioning
- **Version Management**: Support multiple API versions
- **Backward Compatibility**: Maintain compatibility
- **Feature Detection**: Dynamic capability detection
