use std::time::Duration;

use crate::dbus_client::DbusClient;
use cosmic::iced::{time, Length, Subscription};
use cosmic::widget::segmented_button::SingleSelect;
use cosmic::widget::{button, column, container, row, segmented_button, segmented_control, text};
use log::info;
use onedrive_sync_lib::dbus::types::SyncQueueItem;

#[derive(Debug, Clone)]
pub enum Message {
    FetchQueues,
    DownloadQueueLoaded(Result<Vec<SyncQueueItem>, String>),
    UploadQueueLoaded(Result<Vec<SyncQueueItem>, String>),
    Refresh,
    QueSelected(segmented_button::Entity),
    AutoRefresh,
}

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
            .push(text::title1("Download & Upload Queues"))
            .push(text::body("Monitor active transfers and queue status"));

        // Refresh button
        let action_buttons = row()
            .spacing(spacing.space_s)
            .push(button::standard("Refresh").on_press(Message::Refresh));

        // Header with actions
        let header_row = row()
            .spacing(spacing.space_l)
            .align_y(cosmic::iced::Alignment::Center)
            .push(header_section)
            .push(
                container(action_buttons)
                    .align_x(cosmic::iced::alignment::Horizontal::Right)
                    .width(Length::Fill)
            );

        content = content.push(header_row);

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

        // Queue selection tabs
        let queue_selector = container(self.horizontal_selection())
            .padding([spacing.space_none, spacing.space_s])
            .width(Length::Fill);

        content = content.push(queue_selector);

        // Queue content based on selection  
        let selected_queue = self.selected_queue.clone();
        let queue_content = if selected_queue == "Download" {
            self.create_enhanced_queue_card("Download Queue", &self.download_queue)
        } else {
            self.create_enhanced_queue_card("Upload Queue", &self.upload_queue)
        };

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
                .align_y(cosmic::iced::Alignment::Center)
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

    #[allow(dead_code)]
    fn queue_column<'a>(
        &self,
        title: &'a str,
        queue: &'a Vec<SyncQueueItem>,
    ) -> cosmic::Element<'a, Message> {
        let spacing = cosmic::theme::active().cosmic().spacing.space_m;

        let mut columnheader = column()
            .spacing(spacing)
            .width(Length::Fill)
            .padding(spacing);

        columnheader = columnheader.push(text::title3(title));
        columnheader = columnheader.push(cosmic::widget::divider::horizontal::default());
        
        let mut content_column = column()
            .spacing(spacing)
            .width(Length::Fill)
            .padding(spacing);
            
        if queue.is_empty() {
            content_column = content_column.push(text::body("(empty)"));
        } else {
            for item in queue {
                content_column = content_column.push(text::body(format!("{}/{}", item.path, item.name)));
            }
        }
        
        let scrollable = cosmic::widget::scrollable::vertical(content_column);
        
        cosmic::widget::column::column()
            .spacing(spacing)
            .width(Length::Fill)
            .padding(spacing)
            .push(columnheader)
            .push(scrollable)
            .into()
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
                let fetch_upload = async move {
                    match DbusClient::new().await {
                        Ok(client) => match client.get_upload_queue().await {
                            Ok(items) => Ok(items),
                            Err(e) => Err(format!("Failed to get upload queue: {}", e)),
                        },
                        Err(e) => Err(format!("Failed to connect to daemon: {}", e)),
                    }
                };
                let a = cosmic::task::future(fetch_download).map(|result| {
                    cosmic::Action::App(crate::app::Message::QueuesPage(
                        Message::DownloadQueueLoaded(result),
                    ))
                });
                let b = cosmic::task::future(fetch_upload).map(|result| {
                    cosmic::Action::App(crate::app::Message::QueuesPage(
                        Message::UploadQueueLoaded(result),
                    ))
                });
                cosmic::task::batch(vec![a, b])
            }
            Message::FetchQueues => {
                info!("QueuesPage: Fetching download and upload queues");
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
                let fetch_upload = async move {
                    match DbusClient::new().await {
                        Ok(client) => match client.get_upload_queue().await {
                            Ok(items) => Ok(items),
                            Err(e) => Err(format!("Failed to get upload queue: {}", e)),
                        },
                        Err(e) => Err(format!("Failed to connect to daemon: {}", e)),
                    }
                };
                let a = cosmic::task::future(fetch_download).map(|result| {
                    cosmic::Action::App(crate::app::Message::QueuesPage(
                        Message::DownloadQueueLoaded(result),
                    ))
                });
                let b = cosmic::task::future(fetch_upload).map(|result| {
                    cosmic::Action::App(crate::app::Message::QueuesPage(
                        Message::UploadQueueLoaded(result),
                    ))
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
                        Ok(client) => match client.get_download_queue().await {
                            Ok(items) => Ok(items),
                            Err(e) => Err(format!("Failed to get download queue: {}", e)),
                        },
                        Err(e) => Err(format!("Failed to connect to daemon: {}", e)),
                    }
                };
                let fetch_upload = async move {
                    match DbusClient::new().await {
                        Ok(client) => match client.get_upload_queue().await {
                            Ok(items) => Ok(items),
                            Err(e) => Err(format!("Failed to get upload queue: {}", e)),
                        },
                        Err(e) => Err(format!("Failed to connect to daemon: {}", e)),
                    }
                };
                let a = cosmic::task::future(fetch_download).map(|result| {
                    cosmic::Action::App(crate::app::Message::QueuesPage(
                        Message::DownloadQueueLoaded(result),
                    ))
                });
                let b = cosmic::task::future(fetch_upload).map(|result| {
                    cosmic::Action::App(crate::app::Message::QueuesPage(
                        Message::UploadQueueLoaded(result),
                    ))
                });
                cosmic::task::batch(vec![a, b])
            }
        }
    }
}
