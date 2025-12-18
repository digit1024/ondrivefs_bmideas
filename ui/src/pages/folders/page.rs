// SPDX-License-Identifier: MPL-2.0

use crate::dbus_client::{with_dbus_client, DbusClient};
use cosmic::iced::{time, Subscription};
use std::time::Duration;
use super::message::Message;

pub struct Page {
    pub folders: Vec<String>,
    pub new_folder: String,
    pub error: Option<String>,
    pub loading: bool,
}

impl Page {
    pub fn new() -> Self {
        Self {
            folders: Vec::new(),
            new_folder: String::new(),
            error: None,
            loading: false,
        }
    }

    pub fn subscription(&self) -> Subscription<Message> {
        time::every(Duration::from_secs(5)).map(|_| Message::AutoRefresh)
    }

    pub fn update(
        &mut self,
        message: Message,
    ) -> cosmic::Task<cosmic::Action<crate::app::Message>> {
        match message {
            Message::AutoRefresh | Message::FetchFolders => {
                self.loading = true;
                self.error = None;
                let fetch_folders =
                    with_dbus_client(|client| async move { client.list_sync_folders().await });
                log::info!("Fetching folders");

                cosmic::task::future(fetch_folders).map(|result| {
                    cosmic::Action::App(crate::app::Message::FoldersPage(Message::FoldersLoaded(
                        result,
                    )))
                })
            }
            Message::FoldersLoaded(result) => {
                self.loading = false;
                match result {
                    Ok(folders) => {
                        self.folders = folders;
                        self.error = None;
                    }
                    Err(e) => {
                        self.folders.clear();
                        self.error = Some(e);
                    }
                }
                cosmic::Task::none()
            }
            Message::AddFolder => {
                self.loading = true;
                self.error = None;
                let folder = self.new_folder.trim().to_string();
                if folder.is_empty() {
                    self.loading = false;
                    self.error = Some("Folder name cannot be empty".to_string());
                    return cosmic::Task::none();
                }
                let add_folder = async move {
                    match DbusClient::new().await {
                        Ok(client) => match client.add_sync_folder(folder).await {
                            Ok(result) => Ok(result),
                            Err(e) => Err(format!("Failed to add sync folder: {}", e)),
                        },
                        Err(e) => Err(format!("Failed to connect to daemon: {}", e)),
                    }
                };
                cosmic::task::future(add_folder).map(|result| {
                    cosmic::Action::App(crate::app::Message::FoldersPage(Message::FolderAdded(
                        result,
                    )))
                })
            }
            Message::FolderAdded(result) => {
                self.loading = false;
                match result {
                    Ok(true) => {
                        self.new_folder.clear();
                        // Refresh folder list
                        return self.update(Message::FetchFolders);
                    }
                    Ok(false) => {
                        self.error = Some("Folder already exists or failed to add.".to_string());
                    }
                    Err(e) => {
                        self.error = Some(e);
                    }
                }
                cosmic::Task::none()
            }
            Message::DeleteFolder(folder) => {
                self.loading = true;
                self.error = None;
                let delete_folder = async move {
                    match DbusClient::new().await {
                        Ok(client) => match client.remove_sync_folder(folder).await {
                            Ok(result) => Ok(result),
                            Err(e) => Err(format!("Failed to remove sync folder: {}", e)),
                        },
                        Err(e) => Err(format!("Failed to connect to daemon: {}", e)),
                    }
                };
                cosmic::task::future(delete_folder).map(|result| {
                    cosmic::Action::App(crate::app::Message::FoldersPage(Message::FolderDeleted(
                        result,
                    )))
                })
            }
            Message::FolderDeleted(result) => {
                self.loading = false;
                match result {
                    Ok(true) => {
                        // Refresh folder list
                        return self.update(Message::FetchFolders);
                    }
                    Ok(false) => {
                        self.error = Some("Folder not found or failed to delete.".to_string());
                    }
                    Err(e) => {
                        self.error = Some(e);
                    }
                }
                cosmic::Task::none()
            }
            Message::FolderNameChanged(name) => {
                self.new_folder = name;
                cosmic::Task::none()
            }
        }
    }
}

