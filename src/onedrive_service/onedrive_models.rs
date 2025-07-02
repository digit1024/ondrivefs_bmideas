use serde::{Deserialize, Serialize};

/// ParentReference: Represents the parent reference of a drive item.
/// Used to get the  actual path of the item.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ParentReference {
    #[serde(default)]
    pub id: String,
    pub path: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DeltaResponse {
    pub next_link: Option<String>,
    pub delta_link: Option<String>,
    pub items: Option<Vec<DriveItem>>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
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
    #[serde(rename = "lastModifiedDateTime")]
    pub last_modified: Option<String>,
    #[serde(rename = "createdDateTime")]
    pub created_date: Option<String>,
    pub size: Option<u64>,
    pub folder: Option<FolderFacet>,
    pub file: Option<FileFacet>,
    #[serde(rename = "@microsoft.graph.downloadUrl")]
    pub download_url: Option<String>,
    pub deleted: Option<serde_json::Value>,
    #[serde(rename = "parentReference")]
    pub parent_reference: Option<ParentReference>,
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
pub struct UploadResult {
    pub onedrive_id: String,
    pub etag: Option<String>,
    pub web_url: Option<String>,
    pub size: Option<u64>,
}

/// Represents the result of a folder creation operation
#[derive(Debug, Clone)]
pub struct CreateFolderResult {
    pub onedrive_id: String,
    pub folder_name: String,
    pub web_url: Option<String>,
}

/// Represents the result of a delete operation
#[derive(Debug, Clone)]
pub struct DeleteResult {
    pub success: bool,
    pub item_id: String,
    pub item_path: String,
}
