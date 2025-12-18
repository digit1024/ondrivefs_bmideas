// SPDX-License-Identifier: MPL-2.0

use std::time::Duration;
use crate::dbus_client::DbusClient;
use cosmic::iced::{time, Subscription};
use cosmic::widget::segmented_button::SingleSelect;
use cosmic::widget::segmented_button;
use log::info;
use onedrive_sync_lib::dbus::types::SyncQueueItem;
use super::message::Message;

pub struct Page {
    pub download_queue: Vec<SyncQueueItem>,
    pub upload_queue: Vec<SyncQueueItem>,
    pub loading: bool,
    pub error: Option<String>,
    pub selection_model: segmented_button::Model<SingleSelect>,
    pub selected_queue: String,
}

impl Page {
    pub fn new() -> Self {
        let mut selection_model = segmented_button::Model::<SingleSelect>::builder()
            .insert(|b| b.text("Download"))
            .insert(|b| b.text("Upload"))
            .build();
        selection_model.activate_position(0);
        Self {
            download_queue: Vec::new(),
            upload_queue: Vec::new(),
            loading: false,
            error: None,
            selection_model,
            selected_queue: "Download".to_string(),
        }
    }

    pub fn subscription(&self) -> Subscription<Message> {
        time::every(Duration::from_secs(5)).map(|_| Message::AutoRefresh)
    }

    pub fn update(
        &mut self,
        message: Message,
    ) -> cosmic::Task<cosmic::Action<crate::app::Message>> {
        match message {
            Message::QueSelected(id) => {
                self.selection_model.activate(id);
                let text_que = self.selection_model.text(id);
                info!("QueSelected: {}", text_que.unwrap_or(""));

                self.selected_queue = text_que.unwrap_or("Download").to_string();
                cosmic::Task::none()
            }
            Message::AutoRefresh => {
                let fetch_download = async move {
                    match DbusClient::new().await {
                        Ok(client) => match client.get_download_queue().await {
                            Ok(items) => Ok(items),
                            Err(e) => Err(format!("Failed to get download queue: {}", e)),
                        },
                        Err(e) => Err(format!("Failed to connect to daemon: {}", e)),
                    }
                };
                cosmic::task::future(fetch_download).map(|result| {
                    cosmic::Action::App(crate::app::Message::QueuesPage(
                        Message::DownloadQueueLoaded(result),
                    ))
                })
            }
            Message::FetchQueues => {
                info!("QueuesPage: Fetching download queue");
                self.loading = true;
                self.error = None;
                let fetch_download = async move {
                    match DbusClient::new().await {
                        Ok(client) => match client.get_download_queue().await {
                            Ok(items) => Ok(items),
                            Err(e) => Err(format!("Failed to get download queue: {}", e)),
                        },
                        Err(e) => Err(format!("Failed to connect to daemon: {}", e)),
                    }
                };
                cosmic::task::future(fetch_download).map(|result| {
                    cosmic::Action::App(crate::app::Message::QueuesPage(
                        Message::DownloadQueueLoaded(result),
                    ))
                })
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
                        Ok(client) => match client.get_download_queue().await {
                            Ok(items) => Ok(items),
                            Err(e) => Err(format!("Failed to get download queue: {}", e)),
                        },
                        Err(e) => Err(format!("Failed to connect to daemon: {}", e)),
                    }
                };
                cosmic::task::future(fetch_download).map(|result| {
                    cosmic::Action::App(crate::app::Message::QueuesPage(
                        Message::DownloadQueueLoaded(result),
                    ))
                })
            }
        }
    }
}

