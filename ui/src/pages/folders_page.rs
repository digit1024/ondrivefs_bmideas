use crate::dbus_client::{with_dbus_client, DbusClient};
use cosmic::iced::Length;
use cosmic::widget::{button, column, container, row, text, text_input};
use log::info;

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
        let spacing = cosmic::theme::active().cosmic().spacing;

        // Main content with proper spacing and max width
        let mut content = column()
            .spacing(spacing.space_l)
            .padding([spacing.space_none, spacing.space_l])
            .max_width(800.0)
            .width(Length::Fill);

        // Page header with description
        let header_section = column()
            .spacing(spacing.space_s)
            .push(text::title1("Folders Management"))
            .push(text::body("Manage which OneDrive folders are synchronized. Add or remove folders below."));

        // Action buttons row
        let action_buttons = row()
            .spacing(spacing.space_s)
            .push(button::standard("Get Folders").on_press(Message::FetchFolders));

        // Header with actions
        let header_row = row()
            .spacing(spacing.space_l)
            .align_y(cosmic::iced::Alignment::Center)
            .push(header_section)
            .push(
                container(action_buttons)
                    .align_x(cosmic::iced::alignment::Horizontal::Right)
                    .width(Length::Fill)
            );

        content = content.push(header_row);

        // Loading and error states as cards
        if self.loading {
            let loading_card = container(
                row()
                    .spacing(spacing.space_s)
                    .align_y(cosmic::iced::Alignment::Center)
                    .push(text::body("Loading folders..."))
            )
            .class(cosmic::style::Container::Card)
            .padding(spacing.space_l)
            .width(Length::Fill);
            
            content = content.push(loading_card);
        }

        if let Some(error) = &self.error {
            let error_card = container(
                column()
                    .spacing(spacing.space_s)
                    .push(text::title4("Error"))
                    .push(text::body(error))
            )
            .class(cosmic::style::Container::Card)
            .padding(spacing.space_l)
            .width(Length::Fill);
            
            content = content.push(error_card);
        }

        // Add folder form as a card
        let add_folder_card = container(
            column()
                .spacing(spacing.space_m)
                .push(text::title3("Add New Folder"))
                .push(
                    row()
                        .spacing(spacing.space_s)
                        .align_y(cosmic::iced::Alignment::Center)
                        .push(
                            text_input::inline_input("Folder name", &self.new_folder)
                                .on_input(Message::FolderNameChanged)
                                .width(Length::Fill)
                        )
                        .push(button::suggested("Add").on_press(Message::AddFolder))
                )
        )
        .class(cosmic::style::Container::Card)
        .padding(spacing.space_l)
        .width(Length::Fill);

        // Folders list as a card
        let folders_card = container(
            column()
                .spacing(spacing.space_m)
                .push(text::title3("Synchronized Folders"))
                .push(cosmic::widget::divider::horizontal::default())
                .extend(
                    if self.folders.is_empty() {
                        vec![container(
                            text::body("No folders configured")
                        )
                        .padding(spacing.space_l)
                        .center_x(Length::Fill)
                        .width(Length::Fill)
                        .into()]
                    } else {
                        self.folders.iter().map(|folder| {
                            container(
                                row()
                                    .spacing(spacing.space_s)
                                    .align_y(cosmic::iced::Alignment::Center)
                                    .push(
                                        text::body(folder)
                                            .width(Length::Fill)
                                    )
                                    .push(
                                        button::destructive("Delete")
                                            .on_press(Message::DeleteFolder(folder.clone()))
                                    )
                            )
                            .padding([spacing.space_s, spacing.space_none])
                            .width(Length::Fill)
                            .into()
                        }).collect()
                    }
                )
        )
        .class(cosmic::style::Container::Card)
        .padding(spacing.space_l)
        .width(Length::Fill);

        container(
            content
                .push(add_folder_card)
                .push(folders_card)
        )
        .center_x(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    pub fn update(
        &mut self,
        message: Message,
    ) -> cosmic::Task<cosmic::Action<crate::app::Message>> {
        match message {
            Message::FetchFolders => {
                self.loading = true;
                self.error = None;
                let fetch_folders =
                    with_dbus_client(|client| async move { client.list_sync_folders().await });
                info!("Fetching folders");

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
