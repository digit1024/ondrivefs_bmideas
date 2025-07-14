//! Shared types and enums for persistency module 

#[derive(Debug, Clone)]
pub struct VirtualFile {
    pub ino: u64,                        // Inode number
    pub name: String,                    // File name
    pub virtual_path: String,            // Virtual path like "/Documents/file.txt"
    pub display_path: Option<String>,    // Display path with extensions like "/Documents/file.txt.onedrivedownload"
    pub parent_ino: Option<u64>,         // Parent inode number
    pub is_folder: bool,                 // Whether this is a folder
    pub size: u64,                       // File size in bytes
    pub mime_type: Option<String>,       // MIME type
    pub created_date: Option<String>,    // Creation date
    pub last_modified: Option<String>,   // Last modification date
    pub content_file_id: Option<String>, // Points to file in downloads/ or changes/
    pub source: FileSource,              // Where this file comes from
    pub sync_status: Option<String>,     // Sync status if applicable
}

#[derive(Debug, Clone, PartialEq, Copy)]
pub enum FileSource {
    Remote, // From OneDrive (DriveItems)
    Local,  // From local changes (LocalChanges)
    Merged, // Merged from both sources
}

impl FileSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            FileSource::Remote => "remote",
            FileSource::Local => "local",
            FileSource::Merged => "merged",
        }
    }
} 