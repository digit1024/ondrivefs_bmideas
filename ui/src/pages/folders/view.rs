// SPDX-License-Identifier: MPL-2.0

use cosmic::iced::Length;
use cosmic::widget::{button, column, container, row, text, text_input};
use super::{message::Message, page::Page};

impl Page {
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

        // Header section
        content = content.push(header_section);

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
}

