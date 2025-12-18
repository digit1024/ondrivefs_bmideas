// SPDX-License-Identifier: MPL-2.0

#![allow(dead_code)]

use crate::dbus_client::DbusClient;
use cosmic::iced::time;
use cosmic::iced::{Alignment, Length, Subscription};
use cosmic::widget::{button, column, container, row, scrollable, text};
use std::time::Duration;
use super::message::Message;

pub struct Page {
    pub logs: Vec<String>,
    pub loading: bool,
    pub error: Option<String>,
    pub paused: bool,
}

impl Page {
    pub fn new() -> Self {
        Self {
            logs: Vec::new(),
            loading: false,
            error: None,
            paused: false,
        }
    }

    pub fn subscription(&self) -> Subscription<Message> {
        if self.paused {
            Subscription::none()
        } else {
            time::every(Duration::from_secs(5)).map(|_| Message::AutoRefresh)
        }
    }

    pub fn view(&self) -> cosmic::Element<Message> {
        let spacing = cosmic::theme::active().cosmic().spacing.space_m;
        let content = column()
            .spacing(spacing)
            .width(Length::Fill)
            .height(Length::Fill);

        let header = text::title2("Daemon Logs").size(24);
        let pause_button_text = if self.paused { "Resume" } else { "Pause" };
        let pause_row = row().width(Length::Fill).push(
            container(button::standard(pause_button_text).on_press(Message::TogglePause))
                .align_x(Alignment::End)
                .width(Length::Fill),
        );

        let loading_indicator = if self.loading {
            container(text::body("Loading logs...").size(16))
                .padding(8)
                .width(Length::Fill)
        } else {
            container(column()).width(Length::Fill)
        };

        let error_display = if let Some(error) = &self.error {
            container(text::body(format!("Error: {}", error)).size(14))
                .padding(8)
                .width(Length::Fill)
        } else {
            container(column()).width(Length::Fill)
        };

        let log_lines = if self.logs.is_empty() {
            column().push(text::body("No logs available").size(14))
        } else {
            let mut col = column();
            for line in &self.logs {
                col = col.push(text::body(line).size(13));
            }
            col
        };

        let log_scroll = scrollable(log_lines)
            .height(Length::Fill)
            .width(Length::Fill);

        content
            .push(header)
            .push(pause_row)
            .push(loading_indicator)
            .push(error_display)
            .push(log_scroll)
            .into()
    }

    pub fn update(
        &mut self,
        message: Message,
    ) -> cosmic::Task<cosmic::Action<crate::app::Message>> {
        match message {
            Message::TogglePause => {
                self.paused = !self.paused;
                if !self.paused {
                    // Auto-fetch when resuming
                    return self.update(Message::FetchLogs);
                }
                cosmic::Task::none()
            }
            Message::AutoRefresh | Message::FetchLogs | Message::Refresh => {
                if self.paused {
                    return cosmic::Task::none();
                }
                self.loading = true;
                self.error = None;
                let fetch_logs = async move {
                    match DbusClient::new().await {
                        Ok(client) => match client.get_recent_logs().await {
                            Ok(logs) => Ok(logs),
                            Err(e) => Err(format!("Failed to get logs: {}", e)),
                        },
                        Err(e) => Err(format!("Failed to connect to daemon: {}", e)),
                    }
                };
                cosmic::task::future(fetch_logs).map(|result| {
                    cosmic::Action::App(crate::app::Message::LogsPage(Message::LogsLoaded(result)))
                })
            }
            Message::LogsLoaded(result) => {
                self.loading = false;
                match result {
                    Ok(logs) => {
                        self.logs = logs;
                        self.error = None;
                    }
                    Err(e) => {
                        self.logs.clear();
                        self.error = Some(e);
                    }
                }
                cosmic::Task::none()
            }
        }
    }
}

