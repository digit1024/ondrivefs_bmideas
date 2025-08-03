use std::time::Duration;

use cosmic::widget::segmented_button::SingleSelect;
use cosmic::widget::{button, column, container, row, text, segmented_control, segmented_button};
use cosmic::iced::{time, Alignment, Length, Subscription};
use log::info;
use onedrive_sync_lib::dbus::types::SyncQueueItem;
use crate::dbus_client::DbusClient;

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
    pub selectionModel: segmented_button::Model<SingleSelect>,
    pub selected_queue: String,
    


}

impl Page {
    pub fn new() -> Self {
        let mut selectionModel = segmented_button::Model::<SingleSelect>::builder()
        .insert(|b| b.text("Download"))
        .insert(|b| b.text("Upload"))
        .build();
        selectionModel.activate_position(0);
        Self {
            download_queue: Vec::new(),
            upload_queue: Vec::new(),
            loading: false,
            error: None,
            selectionModel : selectionModel,
            selected_queue: "Download".to_string()
        }
    }

    pub fn subscription(&self) -> Subscription<Message> {
        time::every(Duration::from_secs(5)).map(|_| Message::AutoRefresh)
    }



    pub fn view(&self) -> cosmic::Element<Message> {
        let spacing = cosmic::theme::active().cosmic().spacing.space_m;
        let content = column()
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
        let selected_queue = self.selected_queue.clone();

        let queues_row = if selected_queue == "Download" {
             row()
            .push(container(self.queue_column("Download Queue", &self.download_queue)).class(cosmic::theme::Container::Card))
            
        } else {
            row()
            .push(container(self.queue_column("Upload Queue", &self.upload_queue)).class(cosmic::theme::Container::Card))
            
        };

        content
            .push(header)
            .push(refresh_row)
            .push(loading_indicator)
            .push(error_display)
            .push(self.horizontal_selection())
            .push(queues_row)
            .into()
    }
    fn horizontal_selection<'a>(&'a self) -> cosmic::Element<'a, Message> {
        segmented_control::horizontal(&self.selectionModel).on_activate(|id| Message::QueSelected(id)).into()
            
    }


     fn queue_column<'a>(&self, title: &'a str, queue: &'a Vec<SyncQueueItem>) -> cosmic::Element<'a, Message> {
        //let collumn = cosmic::widget::text::title3(title).size(18);
          let spacing = cosmic::theme::active().cosmic().spacing.space_m;
          
          let mut columnheader = column().spacing(spacing).width(Length::Fill).padding(spacing);
        
          columnheader = columnheader.push(text::title3(title).size(18));
          
          columnheader = columnheader.push(cosmic::widget::divider::horizontal::default());
          let mut column = column().spacing(spacing).width(Length::Fill).padding(spacing);
         if queue.is_empty() {
             column = column.push(text::body("(empty)").size(14));
        }else {
            //let mut c = 0;
             for item in queue {
        
                 column = column.push(text::body(format!("{}/{}"  , item.path ,   item.name)).size(14));
                 //if c> 10 {break};
                 //c += 1;
             }
        }
        let scrolable =cosmic::widget::scrollable::vertical(column);
        let container_col = cosmic::widget::column::column().spacing(spacing).width(Length::Fill).padding(spacing)
        .push(columnheader)
        .push(scrolable)
        .into();
        container_col

        //container(column).width(Length::Fill).into()
        //column.into()
    }

    pub fn update(&mut self, message: Message) -> cosmic::Task<cosmic::Action<crate::app::Message>> {
        match message {
            Message::QueSelected(id) => {
                
                self.selectionModel.activate(id);
                let textQue = self.selectionModel.text(id);
                info!("QueSelected: {}", textQue.unwrap_or(""));
                
                self.selected_queue = textQue.unwrap_or("Download").to_string();
                cosmic::Task::none()
            }
            Message::AutoRefresh => {
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