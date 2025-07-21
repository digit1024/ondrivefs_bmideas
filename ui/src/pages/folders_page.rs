use cosmic::widget::{self, button, column, container, row, text, text_input};
use cosmic::iced::{Alignment, Length};

#[derive(Debug, Clone)]
pub enum Message {
    AddFolder,
    DeleteFolder(String),
    FolderNameChanged(String),
}

pub struct Page {
    pub folders: Vec<String>,
    pub new_folder: String,
    pub error: Option<String>,
}

impl Page {
    pub fn new() -> Self {
        Self {
            folders: vec!["Documents".to_string(), "Pictures".to_string()],
            new_folder: String::new(),
            error: None,
        }
    }

    pub fn view(&self) -> cosmic::Element<Message> {
        let spacing = cosmic::theme::active().cosmic().spacing.space_l;
        let mut content = column()
            .spacing(spacing)
            .width(Length::Fill)
            .height(Length::Fill);

        let header = text::title2("Folders Management").size(24);
        let info = text::body("Folder management is not available. No DBus API for folders.")
            .size(16);

        // Add folder input row (disabled)
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

        // Folder list (mock, delete disabled)
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
                    // .push(text::body(folder).size(14).width(Length::Fixed(200.0)))
                    // .push(
                    //     button::standard("Delete")
                    //         .on_press(Message::DeleteFolder(folder.clone()))
                    
                    // )
            }));

        content
            .push(header)
            .push(info)
            .push(add_row)
            .push(folder_list)
            .into()
    }

    pub fn update(&mut self, _message: Message) {
        // All actions are disabled in this placeholder
    }
} 