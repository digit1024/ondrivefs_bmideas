use serde::{Deserialize, Serialize};

/// ParentReference: Represents the parent reference of a drive item.
/// Used to get the  actual path of the item.
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq)]
pub struct ParentReference {
    #[serde(default)]
    pub id: String,
    pub path: Option<String>,
}

impl From<&DriveItem> for ParentReference {
    fn from(item: &DriveItem) -> Self {
        Self {
            id: item.id.clone(),
            path: item.parent_reference.as_ref().and_then(|p| p.path.clone()),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DeltaResponse {
    pub next_link: Option<String>,
    pub delta_link: Option<String>,
    pub items: Option<Vec<DriveItem>>,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct DeltaResponseApi {
    #[serde(default)]
    pub value: Vec<DriveItem>,
    #[serde(rename = "@odata.nextLink")]
    pub next_link: Option<String>,
    #[serde(rename = "@odata.deltaLink")]
    pub delta_link: Option<String>,
}

/// DriveItem: Represents a drive item.
/// Used to get the metadata of the item.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct DriveItem {
    #[serde(default)]
    pub id: String,
    pub name: Option<String>,
    #[serde(rename = "eTag")]
    pub etag: Option<String>,
    #[serde(rename = "cTag")]
    pub ctag: Option<String>,
    #[serde(rename = "lastModifiedDateTime")]
    pub last_modified: Option<String>,
    #[serde(rename = "createdDateTime")]
    pub created_date: Option<String>,
    pub size: Option<u64>,
    pub folder: Option<FolderFacet>,
    pub file: Option<FileFacet>,
    #[serde(rename = "@microsoft.graph.downloadUrl")]
    pub download_url: Option<String>,
    pub deleted: Option<DeletedFacet>,
    #[serde(rename = "parentReference")]
    pub parent_reference: Option<ParentReference>,
}
#[allow(dead_code)]
impl DriveItem {
    /// Set the size of the drive item
    pub fn set_size(&mut self, size: u64) {
        self.size = Some(size);
    }

    /// Set the last modified timestamp
    pub fn set_last_modified(&mut self, last_modified: String) {
        self.last_modified = Some(last_modified);
    }

    /// Set the ETag
    pub fn set_etag(&mut self, etag: String) {
        self.etag = Some(etag);
    }

    /// Set the CTag
    pub fn set_ctag(&mut self, ctag: String) {
        self.ctag = Some(ctag);
    }

    /// Set the name
    pub fn set_name(&mut self, name: String) {
        self.name = Some(name);
    }

    /// Set the created date
    pub fn set_created_date(&mut self, created_date: String) {
        self.created_date = Some(created_date);
    }

    /// Set the download URL
    pub fn set_download_url(&mut self, download_url: String) {
        self.download_url = Some(download_url);
    }

    /// Set the parent reference
    pub fn set_parent_reference(&mut self, parent_reference: ParentReference) {
        self.parent_reference = Some(parent_reference);
    }

    /// Set the folder facet
    pub fn set_folder(&mut self, folder: FolderFacet) {
        self.folder = Some(folder);
        self.file = None; // Clear file facet when setting folder
    }

    /// Set the file facet
    pub fn set_file(&mut self, file: FileFacet) {
        self.file = Some(file);
        self.folder = None; // Clear folder facet when setting file
    }

    /// Mark as deleted
    pub fn mark_deleted(&mut self) {
        self.deleted = Some(DeletedFacet {
            state: "deleted".to_string(),
        });
    }

    /// Clear deleted status
    pub fn clear_deleted(&mut self) {
        self.deleted = None;
    }
}

/// FolderFacet: Represents the folder facet of a drive item.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct FolderFacet {
    #[serde(rename = "childCount")]
    pub child_count: u32,
}

/// FileFacet: Represents the file facet of a drive item.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct FileFacet {
    #[serde(rename = "mimeType")]
    pub mime_type: Option<String>,
}

/// DeletedFacet: Represents the deleted facet of a drive item.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct DeletedFacet {
    pub state: String, // Usually "deleted"
}

/// DriveItemCollection: Represents a collection of drive items.
/// Used to get the collection of drive items.
#[derive(Debug, Deserialize, Serialize)]
pub struct DriveItemCollection {
    pub value: Vec<DriveItem>,
    #[serde(rename = "@odata.nextLink")]
    pub next_link: Option<String>,
    #[serde(rename = "@odata.deltaLink")]
    pub delta_link: Option<String>,
}

/// Represents the result of a file download operation
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct DownloadResult {
    pub file_data: Vec<u8>,
    pub file_name: String,
    pub onedrive_id: String,
    pub etag: Option<String>,
    pub mime_type: Option<String>,
    pub size: Option<u64>,
    pub last_modified: Option<String>,
}

/// Represents the result of a file upload operation
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct UploadResult {
    pub onedrive_id: String,
    pub etag: Option<String>,
    pub ctag: Option<String>,
    pub web_url: Option<String>,
    pub size: Option<u64>,
}

/// Represents the result of a folder creation operation
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct CreateFolderResult {
    pub onedrive_id: String,
    pub folder_name: String,
    pub web_url: Option<String>,
}

/// Represents the result of a delete operation
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct DeleteResult {
    pub success: bool,
    pub item_id: String,
    pub item_path: String,
}

/// User profile information from Microsoft Graph API
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct UserProfile {
    #[serde(default)]
    pub id: String,
    #[serde(rename = "displayName")]
    pub display_name: Option<String>,
    #[serde(rename = "givenName")]
    pub given_name: Option<String>,
    #[serde(rename = "surname")]
    pub surname: Option<String>,
    pub mail: Option<String>,
    #[serde(rename = "userPrincipalName")]
    pub user_principal_name: Option<String>,
    #[serde(rename = "jobTitle")]
    pub job_title: Option<String>,
    #[serde(rename = "businessPhones")]
    pub business_phones: Option<Vec<String>>,
    #[serde(rename = "mobilePhone")]
    pub mobile_phone: Option<String>,
    #[serde(rename = "officeLocation")]
    pub office_location: Option<String>,
    #[serde(rename = "preferredLanguage")]
    pub preferred_language: Option<String>,
}

/// Upload session response from Microsoft Graph API
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct UploadSessionResponse {
    pub upload_url: String,
    #[serde(rename = "expirationDateTime")]
    pub expiration_date_time: String,
}

/// Upload session status response
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct UploadSessionStatus {
    #[serde(rename = "expirationDateTime")]
    pub expiration_date_time: String,
    #[serde(rename = "nextExpectedRanges")]
    pub next_expected_ranges: Vec<String>,
}

/// Upload session request body
#[derive(Debug, Serialize)]
pub struct UploadSessionRequest {
    pub item: UploadSessionItem,
}

/// Upload session item properties
#[derive(Debug, Serialize)]
pub struct UploadSessionItem {
    #[serde(rename = "@microsoft.graph.conflictBehavior")]
    pub conflict_behavior: String,
    pub name: String,
}

/// Represents a file chunk for upload
#[derive(Debug, Clone)]
pub struct FileChunk {
    pub start: u64,
    pub end: u64,
    pub data: Vec<u8>,
}

/// Upload progress information
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct UploadProgress {
    pub bytes_uploaded: u64,
    pub total_bytes: u64,
    pub chunks_completed: usize,
    pub total_chunks: usize,
}

/// Upload session configuration
#[derive(Debug, Clone)]
pub struct UploadSessionConfig {
    pub chunk_size: u64,
    pub max_retries: u32,
    pub retry_delay_ms: u64,
}

impl Default for UploadSessionConfig {
    fn default() -> Self {
        Self {
            chunk_size: 10 * 1024 * 1024, // 10MB
            max_retries: 3,
            retry_delay_ms: 1000,
        }
    }
}
