// SPDX-License-Identifier: MPL-2.0

use cosmic::iced::{Alignment, Length};
use cosmic::widget::{column, container, row, segmented_control, text};
use onedrive_sync_lib::dbus::types::SyncQueueItem;
use super::{message::Message, page::Page};

impl Page {
    pub fn view(&self) -> cosmic::Element<Message> {
        let spacing = cosmic::theme::active().cosmic().spacing;

        // Main content with proper spacing and max width
        let mut content = column()
            .spacing(spacing.space_l)
            .padding([spacing.space_none, spacing.space_l])
            .max_width(1000.0)
            .width(Length::Fill);

        // Page header
        let header_section = column()
            .spacing(spacing.space_s)
            .push(text::title1("Download Queue"));

        // Header section
        content = content.push(header_section);

        // Loading and error states as cards
        if self.loading {
            let loading_card = container(
                row()
                    .spacing(spacing.space_s)
                    .align_y(cosmic::iced::Alignment::Center)
                    .push(text::body("Loading queues..."))
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

        // Queue content - only Downloads
        let queue_content = self.create_enhanced_queue_card("Download Queue", &self.download_queue);

        container(content.push(queue_content))
            .center_x(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn horizontal_selection<'a>(&'a self) -> cosmic::Element<'a, Message> {
        segmented_control::horizontal(&self.selection_model)
            .on_activate(|id| Message::QueSelected(id))
            .into()
    }

    fn create_enhanced_queue_card<'a>(
        &self,
        title: &'a str,
        queue: &'a Vec<SyncQueueItem>,
    ) -> cosmic::Element<'a, Message> {
        let spacing = cosmic::theme::active().cosmic().spacing;
        container(
            column()
                .spacing(spacing.space_m)
                .push(
                    row()
                        .spacing(spacing.space_s)
                        .align_y(cosmic::iced::Alignment::Center)
                        .push(text::title3(title))
                        .push(
                            container(
                                text::caption(format!("{} items", queue.len()))
                            )
                            .padding([spacing.space_xs, spacing.space_s])
                            .class(cosmic::style::Container::Card)
                        )
                )
                .push(cosmic::widget::divider::horizontal::default())
                .push(
                    if queue.is_empty() {
                        column()
                            .spacing(spacing.space_m)
                            .align_x(cosmic::iced::Alignment::Center)
                            .push(text::body("No items in queue"))
                            .push(text::caption("Files will appear here when being transferred"))
                            .padding(spacing.space_xl)
                    } else {
                        column()
                            .extend(queue.iter().enumerate().map(|(index, item)| {
                                self.create_queue_item(item, index)
                            }))
                            .padding(spacing.space_s)
                            .spacing(spacing.space_s)
                    }
                )
        )
        .class(cosmic::style::Container::Card)
        .padding(spacing.space_l)
        .width(Length::Fill)
        .into()
    }

    fn create_queue_item<'a>(
        &self, 
        item: &'a SyncQueueItem, 
        index: usize,
    ) -> cosmic::Element<'a, Message> {
        let spacing = cosmic::theme::active().cosmic().spacing;
        container(
            row()
                .spacing(spacing.space_s)
                .align_y(Alignment::Center)
                .push(
                    container(text::caption(format!("{}", index + 1)))
                        .padding([spacing.space_xs, spacing.space_s])
                        .class(cosmic::style::Container::Card)
                        .width(Length::Fixed(40.0))
                )
                .push(
                    column()
                        .spacing(spacing.space_xs)
                        .push(text::body(&item.name))
                        .push(text::caption(&item.path))
                        .width(Length::Fill)
                )
        )
        .padding([spacing.space_s, spacing.space_xs])
        .width(Length::Fill)
        .into()
    }
}

