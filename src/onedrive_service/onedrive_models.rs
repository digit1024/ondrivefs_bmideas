use serde::Deserialize;



/// ParentReference: Represents the parent reference of a drive item. 
/// Used to get the  actual path of the item.
#[derive(Debug, Deserialize)]
pub struct ParentReference {
    pub id: String,
    pub path: Option<String>,
}

/// DriveItem: Represents a drive item.
/// Used to get the metadata of the item.
#[derive(Debug, Deserialize)]
pub struct DriveItem {
    pub id: String,
    pub name: Option<String>,
    #[serde(rename = "eTag")]
    pub etag: Option<String>,
    #[serde(rename = "lastModifiedDateTime")]
    pub last_modified: Option<String>,
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
#[derive(Debug, Deserialize)]
pub struct FolderFacet {
    #[serde(rename = "childCount")]
    pub child_count: u32,
}

/// FileFacet: Represents the file facet of a drive item.
#[derive(Debug, Deserialize)]
pub struct FileFacet {
    #[serde(rename = "mimeType")]
    pub mime_type: Option<String>,  
}

/// DriveItemCollection: Represents a collection of drive items.
/// Used to get the collection of drive items.
#[derive(Debug, Deserialize)]
pub struct DriveItemCollection {
    pub value: Vec<DriveItem>,
    #[serde(rename = "@odata.nextLink")]
    pub next_link: Option<String>,
    #[serde(rename = "@odata.deltaLink")]
    pub delta_link: Option<String>,
}

