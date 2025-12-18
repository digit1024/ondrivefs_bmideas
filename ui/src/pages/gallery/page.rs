// SPDX-License-Identifier: MPL-2.0

use crate::dbus_client::with_dbus_client;
use cosmic::iced::{time, Subscription};
use std::collections::HashMap;
use std::time::Duration;
use super::message::Message;

pub struct Page {
    pub items: Vec<onedrive_sync_lib::dbus::types::MediaItem>,
    pub offset: u32,
    pub limit: u32,
    pub loading: bool,
    pub error: Option<String>,
    pub start_date: String,
    pub end_date: String,
    pub thumb_paths: HashMap<u64, String>, // ino -> local path
}

impl Page {
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            offset: 0,
            limit: 50,
            loading: false,
            error: None,
            start_date: String::new(),
            end_date: String::new(),
            thumb_paths: HashMap::new(),
        }
    }

    pub fn subscription(&self) -> Subscription<Message> {
        time::every(Duration::from_secs(30)).map(|_| Message::AutoRefresh)
    }

    pub fn update(&mut self, message: Message) -> cosmic::Task<cosmic::Action<crate::app::Message>> {
        match message {
            Message::AutoRefresh => {
                // no-op or soft refresh current page
                cosmic::Task::none()
            }
            Message::Noop => cosmic::Task::none(),
            Message::FetchPage => {
                self.loading = true;
                self.error = None;
                let offset = self.offset;
                let limit = self.limit;
                let start = self.start_date.clone();
                let end = self.end_date.clone();
                let fut = with_dbus_client(move |client| async move { client.list_media(offset, limit, start, end).await });
                cosmic::task::future(fut).map(|result| {
                    cosmic::Action::App(crate::app::Message::GalleryPage(Message::MediaLoaded(result)))
                })
            }
            Message::MediaLoaded(result) => {
                self.loading = false;
                match result {
                    Ok(mut items) => {
                        if self.offset == 0 { self.items.clear(); }
                        let start_index = self.items.len();
                        self.items.append(&mut items);
                        self.error = None;
                        // Auto-fetch thumbnails for the newly appended page
                        let newly_loaded: Vec<u64> = self.items
                            .iter()
                            .skip(start_index)
                            .take(self.limit as usize)
                            .map(|it| it.ino)
                            .collect();
                        let tasks: Vec<cosmic::Task<cosmic::Action<crate::app::Message>>> = newly_loaded
                            .into_iter()
                            .map(|ino| {
                                let fut = with_dbus_client(move |client| async move { client.fetch_thumbnail(ino).await });
                                cosmic::task::future(fut).map(move |result| {
                                    cosmic::Action::App(crate::app::Message::GalleryPage(Message::ThumbLoaded(ino, result)))
                                })
                            })
                            .collect();
                        return cosmic::task::batch(tasks);
                    }
                    Err(e) => { self.error = Some(e); }
                }
                cosmic::Task::none()
            }
            Message::LoadMore => {
                self.offset += self.limit;
                self.update(Message::FetchPage)
            }
            Message::DateStartChanged(s) => { self.start_date = s; cosmic::Task::none() }
            Message::DateEndChanged(s) => { self.end_date = s; cosmic::Task::none() }
            Message::ApplyFilters => {
                self.offset = 0;
                self.update(Message::FetchPage)
            }
            Message::ThumbLoaded(ino, result) => {
                if let Ok(path) = result {
                    self.thumb_paths.insert(ino, path);
                }
                cosmic::Task::none()
            }
            Message::OpenItem(virtual_path) => {
                // Construct the OneDrive mount path: ~/OneDrive{virtual_path}
                let home_dir = std::env::var("HOME").unwrap_or_else(|_| "~".to_string());
                let mount_path = format!("{}/OneDrive{}", home_dir, virtual_path);
                cosmic::task::future(async move { Ok(mount_path) }).map(|result| {
                    cosmic::Action::App(crate::app::Message::GalleryPage(Message::Opened(result)))
                })
            }
            Message::Opened(result) => {
                if let Ok(path) = result {
                    // Attempt to open with system default app
                    let _ = open::that_detached(path);
                }
                cosmic::Task::none()
            }
        }
    }
}

