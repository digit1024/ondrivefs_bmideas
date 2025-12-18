// SPDX-License-Identifier: MPL-2.0

#[derive(Debug, Clone)]
pub enum Message {
    FetchFolders,
    FoldersLoaded(Result<Vec<String>, String>),
    AddFolder,
    DeleteFolder(String),
    FolderNameChanged(String),
    FolderAdded(Result<bool, String>),
    FolderDeleted(Result<bool, String>),
}

