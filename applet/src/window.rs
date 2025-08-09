use std::time::Duration;

use cosmic::app::{Core, Task};
use cosmic::iced::window::Id;
use cosmic::iced::{time, Alignment, Length, Rectangle, Subscription};
use cosmic::iced_runtime::core::window;
use cosmic::surface::action::{app_popup, destroy_popup};
use cosmic::widget::list_column;
use cosmic::widget::{self, column, row, text};
use cosmic::Element;
use onedrive_sync_lib::dbus::types::DaemonStatus;

use crate::dbus_client::with_dbus_client;
use crate::dbus_client::DbusClient;

const ID: &str = "com.github.com.bmideas.onedrive-sync.applet";
const ICON_TRUE: &[u8] = include_bytes!("../../resources/programfiles/icons/ok.png");
const ICON_FALSE: &[u8] = include_bytes!("../../resources/programfiles/icons/error.png");

pub struct Window {
    core: Core,
    popup: Option<Id>,

    daemon_status: Option<DaemonStatus>,
    subscribed: bool,
}

impl Default for Window {
    fn default() -> Self {
        Self {
            core: Core::default(),
            popup: None,

            daemon_status: None,
            subscribed: false,
        }
    }
}

#[derive(Clone, Debug)]
pub enum Message {
    PopupClosed(Id),

    Surface(cosmic::surface::Action),
    FetchStatus,
    //StatusSignal(DaemonStatus),
    StatusLoaded(Result<DaemonStatus, String>),
}

impl Window {
    fn create_status_section(&self) -> cosmic::Element<Message> {
        let spacing = cosmic::theme::active().cosmic().spacing.space_m;

        let title = text::title3("Daemon Status").size(18);

        let status_content = if let Some(status) = &self.daemon_status.clone() {
            column()
                .spacing(spacing)
                .push(self.create_status_row("Authentication", status.is_authenticated))
                .push(self.create_status_row("Connection", status.is_connected))
                .push(self.create_status_row("Conflicts", !status.has_conflicts))
                .push(self.create_status_row("Mounted", status.is_mounted))
        } else {
            column()
                .spacing(spacing)
                .push(text::body("No status data available").size(14))
        };

        column()
            .spacing(spacing)
            .push(title)
            .push(status_content)
            .into()
    }
    fn create_status_row(&self, label: &str, value: bool) -> cosmic::Element<Message> {
        let icon_data = if value { ICON_TRUE } else { ICON_FALSE };

        let icon = widget::icon::from_raster_bytes(icon_data).icon();

        row()
            .spacing(cosmic::theme::active().cosmic().spacing.space_s)
            .align_y(Alignment::Center)
            .height(Length::Fixed(32.0))
            .push(
                text::body(label.to_string())
                    .size(14)
                    .width(Length::Fixed(120.0)),
            )
            .push(icon.height(Length::Fixed(32.0)).width(Length::Fixed(32.0)))
            .into()
    }
}

impl cosmic::Application for Window {
    type Executor = cosmic::SingleThreadExecutor;
    type Flags = ();
    type Message = Message;
    const APP_ID: &'static str = ID;

    fn core(&self) -> &Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut Core {
        &mut self.core
    }

    fn init(core: Core, _flags: Self::Flags) -> (Self, Task<Message>) {
        let window = Window {
            core,
            ..Default::default()
        };
        (window, Task::none())
    }

    fn on_close_requested(&self, id: window::Id) -> Option<Message> {
        Some(Message::PopupClosed(id))
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::FetchStatus => {
                // One-time subscribe to status signal
                if !self.subscribed {
                    self.subscribed = true;
                    let subscribe = async move {
                        if let Ok(client) = DbusClient::new().await {
                            let _ = client
                                .subscribe_daemon_status(|_status| async move {
                                    // In this minimal applet, we keep polling fallback.
                                })
                                .await;
                        }
                    };
                    let _ = Task::perform(subscribe, |_| cosmic::Action::<Message>::None);
                }
                let fetch_status =
                    with_dbus_client(|client| async move { client.get_daemon_status().await });
                Task::perform(fetch_status, |result| match result {
                    Ok(status) => cosmic::Action::App(Message::StatusLoaded(Ok(status))),
                    Err(e) => cosmic::Action::App(Message::StatusLoaded(Err(e.to_string()))),
                })
            }
            Message::PopupClosed(id) => {
                if self.popup.as_ref() == Some(&id) {
                    self.popup = None;
                }
                Task::none()
            }

            Message::Surface(a) => {
                cosmic::task::message(cosmic::Action::Cosmic(cosmic::app::Action::Surface(a)))
            }

       

            Message::StatusLoaded(result) => {
                match result {
                    Ok(status) => {
                        self.daemon_status = Some(status);
                    }
                    Err(_e) => {
                        self.daemon_status = None;
                    }
                }
                Task::none()
            }
        }
    }
    fn subscription(&self) -> Subscription<Message> {
        // Keep a modest poll as fallback
        time::every(Duration::from_secs(15)).map(|_| Message::FetchStatus)
    }

    fn view(&self) -> Element<Message> {
        let have_popup = self.popup.clone();
        let icon = if let Some(status) = &self.daemon_status.clone() {
            if status.is_authenticated
                && status.is_connected
                && !status.has_conflicts
                && status.is_mounted
            {
                "open-onedrive-ok"
            } else if !status.is_connected {
                "open-onedrive-offline"
            } else {
                "open-onedrive-error"
            }
        } else {
            "open-onedrive-offline"
        };

        let btn =
            self.core
                .applet
                .icon_button(icon)
                .on_press_with_rectangle(move |offset, bounds| {
                    if let Some(id) = have_popup {
                        Message::Surface(destroy_popup(id))
                    } else {
                        Message::Surface(app_popup::<Window>(
                            move |state: &mut Window| {
                                let new_id = Id::unique();
                                state.popup = Some(new_id);
                                let mut popup_settings = state.core.applet.get_popup_settings(
                                    state.core.main_window_id().unwrap(),
                                    new_id,
                                    None,
                                    None,
                                    None,
                                );

                                popup_settings.positioner.anchor_rect = Rectangle {
                                    x: (bounds.x - offset.x) as i32,
                                    y: (bounds.y - offset.y) as i32,
                                    width: bounds.width as i32,
                                    height: bounds.height as i32,
                                };

                                popup_settings
                            },
                            Some(Box::new(|state: &Window| {
                                let status_section = state.create_status_section();
                                let content_list =
                                    list_column().padding(5).spacing(0).add(status_section);
                                Element::from(state.core.applet.popup_container(content_list))
                                    .map(cosmic::Action::App)
                            })),
                        ))
                    }
                });

        Element::from(self.core.applet.applet_tooltip::<Message>(
            btn,
            "Open OneDrive",
            self.popup.is_some(),
            |a| Message::Surface(a),
            None,
        ))
    }

    fn view_window(&self, _id: Id) -> Element<Message> {
        "oops".into()
    }

    fn style(&self) -> Option<cosmic::iced_runtime::Appearance> {
        Some(cosmic::applet::style())
    }
}
