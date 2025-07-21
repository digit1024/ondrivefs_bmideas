use cosmic::widget::{self, button, column, container, row, text};
use cosmic::iced::{Alignment, Length};
use log::{error, info};
use onedrive_sync_lib::dbus::types::SyncQueueItem;
use crate::dbus_client::DbusClient;

#[derive(Debug, Clone)]
pub enum Message {
    FetchQueues,
    DownloadQueueLoaded(Result<Vec<SyncQueueItem>, String>),
    UploadQueueLoaded(Result<Vec<SyncQueueItem>, String>),
    Refresh,
}

pub struct Page {
    pub download_queue: Vec<SyncQueueItem>,
    pub upload_queue: Vec<SyncQueueItem>,
    pub loading: bool,
    pub error: Option<String>,
}

impl Page {
    pub fn new() -> Self {
        Self {
            download_queue: Vec::new(),
            upload_queue: Vec::new(),
            loading: false,
            error: None,
        }
    }

    pub fn view(&self) -> cosmic::Element<Message> {
        let spacing = cosmic::theme::active().cosmic().spacing.space_l;
        let mut content = column()
            .spacing(spacing)
            .width(Length::Fill)
            .height(Length::Fill);

        let header = text::title2("Download & Upload Queues").size(24);
        let refresh_row = row()
            .width(Length::Fill)
            .push(
                container(button::standard("Refresh")
                    .on_press(Message::Refresh))
                    .align_x(Alignment::End)
                    .width(Length::Fill)
            );

        let loading_indicator = if self.loading {
            container(text::body("Loading queues...").size(16)).padding(8).width(Length::Fill)
        } else {
            container(column()).width(Length::Fill)
        };

        let error_display = if let Some(error) = &self.error {
            container(text::body(format!("Error: {}", error)).size(14)).padding(8).width(Length::Fill)
        } else {
            container(column()).width(Length::Fill)
        };

        let queues_row = row()
            .spacing(spacing)
            .push(self.queue_column("Download Queue", &self.download_queue))
            .push(self.queue_column("Upload Queue", &self.upload_queue));

        content
            .push(header)
            .push(refresh_row)
            .push(loading_indicator)
            .push(error_display)
            .push(queues_row)
            .into()
    }

    fn queue_column<'a>(&self, title: &'a str, queue: &'a Vec<SyncQueueItem>) -> cosmic::Element<'a, Message> {
        let spacing = cosmic::theme::active().cosmic().spacing.space_m;
        let mut col = column().spacing(spacing).width(Length::Fill);
        col = col.push(text::title3(title).size(18));
        if queue.is_empty() {
            col = col.push(text::body("(empty)").size(14));
        } else {
            for item in queue {
                col = col.push(text::body(format!("{}/{}"  , item.path ,   item.name)).size(14));
            }
        }
        container(col).width(Length::Fill).into()
    }

    pub fn update(&mut self, message: Message) -> cosmic::Task<cosmic::Action<crate::app::Message>> {
        match message {
            Message::FetchQueues => {
                info!("QueuesPage: Fetching download and upload queues");
                self.loading = true;
                self.error = None;
                let fetch_download = async move {
                    match DbusClient::new().await {
                        Ok(client) => {
                            match client.get_download_queue().await {
                                Ok(items) => Ok(items),
                                Err(e) => Err(format!("Failed to get download queue: {}", e)),
                            }
                        }
                        Err(e) => Err(format!("Failed to connect to daemon: {}", e)),
                    }
                };
                let fetch_upload = async move {
                    match DbusClient::new().await {
                        Ok(client) => {
                            match client.get_upload_queue().await {
                                Ok(items) => Ok(items),
                                Err(e) => Err(format!("Failed to get upload queue: {}", e)),
                            }
                        }
                        Err(e) => Err(format!("Failed to connect to daemon: {}", e)),
                    }
                };
                let a = cosmic::task::future(fetch_download).map(|result| {
                    cosmic::Action::App(crate::app::Message::QueuesPage(Message::DownloadQueueLoaded(result)))
                });
                let b = cosmic::task::future(fetch_upload).map(|result| {
                    cosmic::Action::App(crate::app::Message::QueuesPage(Message::UploadQueueLoaded(result)))
                });
                cosmic::task::batch(vec![a, b])
            }
            Message::DownloadQueueLoaded(result) => {
                self.loading = false;
                match result {
                    Ok(items) => {
                        self.download_queue = items;
                        self.error = None;
                    }
                    Err(e) => {
                        self.download_queue.clear();
                        self.error = Some(e);
                    }
                }
                cosmic::Task::none()
            }
            Message::UploadQueueLoaded(result) => {
                self.loading = false;
                match result {
                    Ok(items) => {
                        self.upload_queue = items;
                        self.error = None;
                    }
                    Err(e) => {
                        self.upload_queue.clear();
                        self.error = Some(e);
                    }
                }
                cosmic::Task::none()
            }
            Message::Refresh => {
                self.loading = true;
                self.error = None;
                let fetch_download = async move {
                    match DbusClient::new().await {
                        Ok(client) => {
                            match client.get_download_queue().await {
                                Ok(items) => Ok(items),
                                Err(e) => Err(format!("Failed to get download queue: {}", e)),
                            }
                        }
                        Err(e) => Err(format!("Failed to connect to daemon: {}", e)),
                    }
                };
                let fetch_upload = async move {
                    match DbusClient::new().await {
                        Ok(client) => {
                            match client.get_upload_queue().await {
                                Ok(items) => Ok(items),
                                Err(e) => Err(format!("Failed to get upload queue: {}", e)),
                            }
                        }
                        Err(e) => Err(format!("Failed to connect to daemon: {}", e)),
                    }
                };
                let a = cosmic::task::future(fetch_download).map(|result| {
                    cosmic::Action::App(crate::app::Message::QueuesPage(Message::DownloadQueueLoaded(result)))
                });
                let b = cosmic::task::future(fetch_upload).map(|result| {
                    cosmic::Action::App(crate::app::Message::QueuesPage(Message::UploadQueueLoaded(result)))
                });
                cosmic::task::batch(vec![a, b])
            }
        }
    }
} 