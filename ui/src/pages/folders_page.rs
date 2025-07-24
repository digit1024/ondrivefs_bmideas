use cosmic::widget::{self, button, column, container, row, text, text_input};
use cosmic::iced::{Alignment, Length};
use log::info;
use crate::dbus_client::{DbusClient, with_dbus_client};


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

    pub fn view(&self) -> cosmic::Element<Message> {
        let spacing = cosmic::theme::active().cosmic().spacing.space_l;
        let mut content = column()
            .spacing(spacing)
            .width(Length::Fill)
            .height(Length::Fill);

        let header = text::title2("Folders Management").size(24);
        let info = text::body("Manage which OneDrive folders are synchronized. Add or remove folders below.")
            .size(16);

        let add_row = row()
            .spacing(spacing)
            .push(
                text_input::inline_input("Folder name", &self.new_folder)
                    .on_input(Message::FolderNameChanged)
                    .width(Length::Fixed(200.0))
            )
            .push(
                button::standard("Add")
                    .on_press(Message::AddFolder)
            );

        let folder_list = column()
            .spacing(spacing)
            .push(text::title3("Folders").size(18))
            .extend(self.folders.iter().map(|folder| {
                row()
                    .spacing(spacing)
                    .push(text::body(folder).size(14).width(Length::Fixed(200.0)))
                    .push(
                        button::destructive("Delete")
                            .on_press(Message::DeleteFolder(folder.clone()))
                    )
                    .into()
            }));

        let loading_indicator = if self.loading {
            container(text::body("Loading folders...").size(16)).padding(8).width(Length::Fill)
        } else {
            container(column()).width(Length::Fill)
        };

        let error_display = if let Some(error) = &self.error {
            container(text::body(format!("Error: {}", error)).size(14)).padding(8).width(Length::Fill)
        } else {
            container(column()).width(Length::Fill)
        };
        let get_button = button::standard("Get Folders").on_press(Message::FetchFolders);

        content
            .push(header)
            .push(info)
            .push(get_button)
            .push(loading_indicator)
            .push(error_display)
            .push(add_row)
            .push(folder_list)
            .into()
    }

    pub fn update(&mut self, message: Message) -> cosmic::Task<cosmic::Action<crate::app::Message>> {
        match message {
            Message::FetchFolders => {
                self.loading = true;
                self.error = None;
                let fetch_folders= with_dbus_client(|client| 
                    async move {client.list_sync_folders().await}
                );
                info!("Fetching folders");

                cosmic::task::future(fetch_folders).map(|result| {
                    cosmic::Action::App(crate::app::Message::FoldersPage(Message::FoldersLoaded(result)))
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
                        Ok(client) => {
                            match client.add_sync_folder(folder).await {
                                Ok(result) => Ok(result),
                                Err(e) => Err(format!("Failed to add sync folder: {}", e)),
                            }
                        }
                        Err(e) => Err(format!("Failed to connect to daemon: {}", e)),
                    }
                };
                cosmic::task::future(add_folder).map(|result| {
                    cosmic::Action::App(crate::app::Message::FoldersPage(Message::FolderAdded(result)))
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
                        Ok(client) => {
                            match client.remove_sync_folder(folder).await {
                                Ok(result) => Ok(result),
                                Err(e) => Err(format!("Failed to remove sync folder: {}", e)),
                            }
                        }
                        Err(e) => Err(format!("Failed to connect to daemon: {}", e)),
                    }
                };
                cosmic::task::future(delete_folder).map(|result| {
                    cosmic::Action::App(crate::app::Message::FoldersPage(Message::FolderDeleted(result)))
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