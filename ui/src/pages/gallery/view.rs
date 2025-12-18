// SPDX-License-Identifier: MPL-2.0

use cosmic::iced::{Alignment, Length};
use cosmic::widget::{button, column, container, row, scrollable, text, image as image_widget};
use cosmic::iced::widget::image::Handle as ImageHandle;
use super::{message::Message, page::Page};

impl Page {
    pub fn view(&self) -> cosmic::Element<Message> {
        let spacing = cosmic::theme::active().cosmic().spacing.space_m;
        let header = text::title2("Gallery").size(24);
        
        // Filter card - collapsible
        let filter_toggle_text = if self.filter_card_expanded { "Hide Filters" } else { "Show Filters" };
        let mut filter_card_items: Vec<cosmic::Element<Message>> = vec![
            row()
                .spacing(spacing)
                .push(text::title3("Filter"))
                .push(
                    container(button::standard(filter_toggle_text).on_press(Message::ToggleFilterCard))
                        .align_x(Alignment::End)
                        .width(Length::Fill)
                )
                .into()
        ];
        
        if self.filter_card_expanded {
            filter_card_items.push(
                row()
                    .spacing(spacing)
                    .push(
                        button::standard(if self.start_date.is_empty() { "Select Start Date" } else { &self.start_date })
                            .on_press(Message::OpenStartDateCalendar)
                    )
                    .push(
                        button::standard(if self.end_date.is_empty() { "Select End Date" } else { &self.end_date })
                            .on_press(Message::OpenEndDateCalendar)
                    )
                    .push(button::suggested("Apply").on_press(Message::ApplyFilters))
                    .into()
            );
            
        }
        
        let filter_card = container(
            column()
                .spacing(spacing)
                .extend(filter_card_items)
        )
        .class(cosmic::style::Container::Card)
        .padding(spacing)
        .width(Length::Fill);

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

        let main_content = column()
            .spacing(spacing)
            .push(header)
            .push(filter_card)
            .push(status)
            .push(error)
            .push(list);

        main_content.into()
    }
}

