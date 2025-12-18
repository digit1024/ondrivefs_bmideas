// SPDX-License-Identifier: MPL-2.0

use cosmic::iced::{Alignment, Length};
use cosmic::widget::{self, button, column, container, row, text};
use super::{message::Message, page::Page};

const ICON_TRUE: &[u8] = include_bytes!("../../../../resources/programfiles/icons/ok.png");
const ICON_FALSE: &[u8] = include_bytes!("../../../../resources/programfiles/icons/error.png");

impl Page {
    pub fn view(&self) -> cosmic::Element<Message> {
        let spacing = cosmic::theme::active().cosmic().spacing;
        
        // Main content with proper spacing
        let mut content = column()
            .spacing(spacing.space_l)
            .padding([spacing.space_none, spacing.space_l])
            .max_width(1000.0)
            .width(Length::Fill);

        // Page header with welcome message
        let header = if let Some(profile) = &self.user_profile {
            column()
                .spacing(spacing.space_xs)
                .push(text::title1(format!("Welcome {}", profile.given_name)))
                .push(text::body("OneDrive Client Status"))
        } else {
            column()
                .spacing(spacing.space_xs)
                .push(text::title1("OneDrive Client"))
                .push(text::body("Sync Status Dashboard"))
        };

        // Action buttons row with better styling
        let action_buttons = row()
            .spacing(spacing.space_s)
            .push(button::standard("Refresh").on_press(Message::Refresh))
            .push(button::standard("Pause/Resume Sync").on_press(Message::ToggleSyncPause))
            .push(button::destructive("Full Reset").on_press(Message::FullReset));

        // Header section with title and actions
        let header_section = row()
            .spacing(spacing.space_l)
            .align_y(cosmic::iced::Alignment::Center)
            .push(header)
            .push(
                container(action_buttons)
                    .align_x(cosmic::iced::alignment::Horizontal::Right)
                    .width(Length::Fill)
            );

        content = content.push(header_section);

        // Loading and error states
        if self.loading {
            let loading_card = container(
                row()
                    .spacing(spacing.space_s)
                    .align_y(cosmic::iced::Alignment::Center)
                    .push(text::body("Loading status..."))
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

        // Main content cards in a responsive grid
        let cards_row = row()
            .spacing(spacing.space_l)
            .push(
                container(self.create_profile_section())
                    .class(cosmic::style::Container::Card)
                    .padding(spacing.space_l)
                    .width(Length::FillPortion(1))
            )
            .push(
                container(self.create_status_section())
                    .class(cosmic::style::Container::Card)
                    .padding(spacing.space_l)
                    .width(Length::FillPortion(1))
            );

        container(content.push(cards_row))
            .center_x(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn create_status_section(&self) -> cosmic::Element<Message> {
        let spacing = cosmic::theme::active().cosmic().spacing;

        let title = text::title3("Daemon Status");

        let status_content = if let Some(status) = &self.daemon_status {
            column()
                .spacing(spacing.space_s)
                .push(self.create_status_row("Authentication", status.is_authenticated))
                .push(widget::divider::horizontal::default())
                .push(self.create_status_row("Connection", status.is_connected))
                .push(widget::divider::horizontal::default())
                .push(self.create_status_row("Conflicts", !status.has_conflicts))
                .push(widget::divider::horizontal::default())
                .push(self.create_status_row("Mounted", status.is_mounted))
        } else {
            column()
                .spacing(spacing.space_s)
                .push(text::body("No status data available"))
                .into()
        };

        column()
            .spacing(spacing.space_m)
            .push(title)
            .push(status_content)
            .into()
    }

    fn create_profile_section(&self) -> cosmic::Element<Message> {
        let spacing = cosmic::theme::active().cosmic().spacing;

        let title = text::title3("User Profile");

        let profile_content = if let Some(profile) = &self.user_profile {
            column()
                .spacing(spacing.space_s)
                .push(self.create_profile_row("Name", &profile.display_name))
                .push(widget::divider::horizontal::default())
                .push(self.create_profile_row("Email", &profile.mail))
        } else {
            column()
                .spacing(spacing.space_s)
                .push(text::body("No profile data available"))
                .into()
        };

        column()
            .spacing(spacing.space_m)
            .push(title)
            .push(profile_content)
            .into()
    }

    fn create_status_row<'a>(&self, label: &'a str, value: bool) -> cosmic::Element<'a, Message> {
        let spacing = cosmic::theme::active().cosmic().spacing;
        let icon_data = if value { ICON_TRUE } else { ICON_FALSE };
        let icon = widget::icon::from_raster_bytes(icon_data).icon();

        // Status badge for better visual feedback
        let status_text = if value { "Active" } else { "Inactive" };

        row()
            .spacing(spacing.space_s)
            .align_y(Alignment::Center)
            .padding([spacing.space_xs, spacing.space_none])
            .push(
                text::body(label)
                    .width(Length::Fixed(120.0))
            )
            .push(
                row()
                    .spacing(spacing.space_xs)
                    .align_y(Alignment::Center)
                    .push(icon.height(Length::Fixed(20.0)).width(Length::Fixed(20.0)))
                    .push(text::caption(status_text))
            )
            .into()
    }

    fn create_profile_row<'a>(&self, label: &'a str, value: &'a str) -> cosmic::Element<'a, Message> {
        let spacing = cosmic::theme::active().cosmic().spacing;
        
        row()
            .spacing(spacing.space_s)
            .align_y(Alignment::Center)
            .padding([spacing.space_xs, spacing.space_none])
            .push(
                text::body(label)
                    .width(Length::Fixed(120.0))
            )
            .push(text::body(value))
            .into()
    }
}

