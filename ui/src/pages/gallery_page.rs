use crate::dbus_client::{with_dbus_client, DbusClient};
use cosmic::iced::{time, Alignment, Length, Subscription};
use cosmic::widget::{self, button, column, container, row, scrollable, text, text_input};
use cosmic::iced::widget::image;
use log::info;
use onedrive_sync_lib::dbus::types::MediaItem;
use std::collections::HashMap;
use std::time::Duration;

#[derive(Debug, Clone)]
pub enum Message {
    FetchPage,
    MediaLoaded(Result<Vec<MediaItem>, String>),
    LoadMore,
    DateStartChanged(String),
    DateEndChanged(String),
    ApplyFilters,
    AutoRefresh,
    ThumbRequested(u64),
    ThumbLoaded(u64, Result<String, String>),
}

pub struct Page {
    items: Vec<MediaItem>,
    offset: u32,
    limit: u32,
    loading: bool,
    error: Option<String>,
    start_date: String,
    end_date: String,
    thumb_paths: HashMap<u64, String>, // ino -> local path
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

        let mut grid = column().spacing(spacing);
        for chunk in self.items.chunks(5) {
            let mut roww = row().spacing(spacing);
            for item in chunk {
                let thumb_el = if let Some(path) = self.thumb_paths.get(&item.ino) {
                    let handle = image::Handle::from_path(path.clone());
                    image(handle)
                        .width(Length::Fixed(150.0))
                        .height(Length::Fixed(150.0))
                        .into()
                } else {
                    // Placeholder while loading thumbnail
                    container(text::body("Loading thumb...")).width(Length::Fixed(150.0)).height(Length::Fixed(150.0)).into()
                };
                let card = container(
                    column()
                        .spacing(spacing)
                        .push(thumb_el)
                        .push(text::body(item.name.clone()).size(12))
                )
                .class(cosmic::style::Container::Card)
                .padding(8)
                .width(Length::Fixed(170.0));
                roww = roww.push(card);
            }
            grid = grid.push(roww);
        }

        let list = scrollable(container(grid).width(Length::Fill)).height(Length::Fill);

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

    pub fn update(&mut self, message: Message) -> cosmic::Task<cosmic::Action<crate::app::Message>> {
        match message {
            Message::AutoRefresh => {
                // no-op or soft refresh current page
                cosmic::Task::none()
            }
            Message::FetchPage => {
                self.loading = true;
                self.error = None;
                let offset = self.offset;
                let limit = self.limit;
                let start = if self.start_date.trim().is_empty() { None } else { Some(self.start_date.clone()) };
                let end = if self.end_date.trim().is_empty() { None } else { Some(self.end_date.clone()) };
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
            Message::ThumbRequested(ino) => {
                // Fire request
                let fut = with_dbus_client(move |client| async move { client.fetch_thumbnail(ino).await });
                cosmic::task::future(fut).map(move |result| {
                    cosmic::Action::App(crate::app::Message::GalleryPage(Message::ThumbLoaded(ino, result)))
                })
            }
            Message::ThumbLoaded(ino, result) => {
                if let Ok(path) = result {
                    self.thumb_paths.insert(ino, path);
                }
                cosmic::Task::none()
            }
        }
    }
}