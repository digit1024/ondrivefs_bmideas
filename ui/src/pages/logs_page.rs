use cosmic::widget::{self, button, column, container, row, text, scrollable};
use cosmic::iced::{Alignment, Length, Subscription};
use log::info;
use crate::dbus_client::DbusClient;
use std::time::Duration;
use cosmic::iced::time;

#[derive(Debug, Clone)]
pub enum Message {
    FetchLogs,
    LogsLoaded(Result<Vec<String>, String>),
    Refresh,
    AutoRefresh,
}

pub struct Page {
    pub logs: Vec<String>,
    pub loading: bool,
    pub error: Option<String>,
}

impl Page {
    pub fn new() -> Self {
        Self {
            logs: Vec::new(),
            loading: false,
            error: None,
        }
    }

    pub fn subscription(&self) -> Subscription<Message> {
        time::every(Duration::from_secs(5)).map(|_| Message::AutoRefresh)
    }

    pub fn view(&self) -> cosmic::Element<Message> {
        let spacing = cosmic::theme::active().cosmic().spacing.space_m;
        let mut content = column()
            .spacing(spacing)
            .width(Length::Fill)
            .height(Length::Fill);

        let header = text::title2("Daemon Logs").size(24);
        let refresh_row = row()
            .width(Length::Fill)
            .push(
                container(button::standard("Refresh")
                    .on_press(Message::Refresh))
                    .align_x(Alignment::End)
                    .width(Length::Fill)
            );

        let loading_indicator = if self.loading {
            container(text::body("Loading logs...").size(16)).padding(8).width(Length::Fill)
        } else {
            container(column()).width(Length::Fill)
        };

        let error_display = if let Some(error) = &self.error {
            container(text::body(format!("Error: {}", error)).size(14)).padding(8).width(Length::Fill)
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

        let log_scroll = scrollable(log_lines).height(Length::Fill).width(Length::Fill);

        content
            .push(header)
            .push(refresh_row)
            .push(loading_indicator)
            .push(error_display)
            .push(log_scroll)
            .into()
    }

    pub fn update(&mut self, message: Message) -> cosmic::Task<cosmic::Action<crate::app::Message>> {
        match message {
            Message::AutoRefresh | Message::FetchLogs | Message::Refresh => {
                self.loading = true;
                self.error = None;
                let fetch_logs = async move {
                    match DbusClient::new().await {
                        Ok(client) => {
                            match client.get_recent_logs().await {
                                Ok(logs) => Ok(logs),
                                Err(e) => Err(format!("Failed to get logs: {}", e)),
                            }
                        }
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