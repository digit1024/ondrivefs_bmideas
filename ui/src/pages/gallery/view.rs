// SPDX-License-Identifier: MPL-2.0

use cosmic::iced::{Alignment, Length};
use cosmic::widget::{button, column, container, row, scrollable, text, text_input, image as image_widget};
use cosmic::iced::widget::image::Handle as ImageHandle;
use super::{message::Message, page::Page};

impl Page {
    pub fn view(&self) -> cosmic::Element<Message> {
        let spacing = cosmic::theme::active().cosmic().spacing.space_m;
        let header = row()
            .spacing(spacing)
            .push(text::title2("Gallery").size(24))
            .push(
                row()
                    .spacing(spacing)
                    .push(text_input::inline_input("Start date (YYYY-MM-DD)", &self.start_date).on_input(Message::DateStartChanged))
                    .push(text_input::inline_input("End date (YYYY-MM-DD)", &self.end_date).on_input(Message::DateEndChanged))
                    .push(button::standard("Apply").on_press(Message::ApplyFilters))
                    .push(button::standard("Load more").on_press(Message::LoadMore)),
            );

        // Create responsive grid with dynamic column calculation
        let mut grid = column().spacing(spacing);
        
        // Use available width to determine columns per row
        let columns_per_row = if self.items.len() < 3 {
            self.items.len().max(1)
        } else {
            6
        };
        
        for chunk in self.items.chunks(columns_per_row) {
            let mut roww = row().spacing(spacing).width(Length::Fill);
            for item in chunk {
                let thumb_el: cosmic::Element<Message> = if let Some(path) = self.thumb_paths.get(&item.ino) {
                    let handle = ImageHandle::from_path(path.clone());
                    let img = image_widget(handle);
                    let clickable = button::custom(img).class(cosmic::style::Button::Image).on_press(Message::OpenItem(item.virtual_path.clone()));
                    let image_container = container(clickable)
                        .width(Length::Fill)
                        .height(Length::Fixed(176.0))
                        .center_x(Length::Fill)
                        .center_y(Length::Fill);
                    image_container.into()
                } else {
                    // Placeholder while loading thumbnail
                    container(text::body("Loading thumb..."))
                        .width(Length::Fill)
                        .height(Length::Fixed(176.0))
                        .center_x(Length::Fill)
                        .center_y(Length::Fill)
                        .into()
                };
                let card = container(
                    column()
                        .spacing(spacing)
                        .align_x(Alignment::Center)
                        .height(Length::Fixed(176.0))
                        .push(thumb_el)
                        .push(text::body(item.name.clone()).size(12))
                )
                .class(cosmic::style::Container::Card)
                .padding(8)
                .width(Length::FillPortion(1));
                roww = roww.push(card);
            }
            grid = grid.push(roww);
        }

        let list = scrollable(container(grid).width(Length::Fill)).height(Length::Fill).on_scroll(|vp| {
            let abs = vp.absolute_offset();
            let bounds = vp.bounds();
            let content = vp.content_bounds();
    
            let remaining_y_px = (content.height - (abs.y + bounds.height)).max(0.0);
    
            if remaining_y_px <= 20.0 && !self.loading {
                Message::LoadMore
            } else {
                Message::Noop
            }
        });
        let status = if self.loading {
            container(text::body("Loading...")).width(Length::Fill)
        } else {
            container(text::body("")).width(Length::Fill)
        };

        let error = if let Some(err) = &self.error {
            container(text::body(format!("Error: {}", err)))
        } else { container(text::body("")) };

        column()
            .spacing(spacing)
            .push(header)
            .push(status)
            .push(error)
            .push(list)
            .into()
    }
}

