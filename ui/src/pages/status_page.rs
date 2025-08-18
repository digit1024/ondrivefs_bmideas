use std::time::Duration;

use crate::dbus_client::{with_dbus_client, DbusClient, take_latest_status};
use cosmic::iced::{time, Alignment, Length, Subscription};
use cosmic::widget::{self, button, column, container, row, text};
use log::{error, info};
use onedrive_sync_lib::dbus::types::{DaemonStatus, UserProfile};

const ICON_TRUE: &[u8] = include_bytes!("../../../resources/programfiles/icons/ok.png");
const ICON_FALSE: &[u8] = include_bytes!("../../../resources/programfiles/icons/error.png");

#[derive(Debug, Clone)]
pub enum Message {
    FetchStatus,
    StatusSignal(DaemonStatus),
    StatusLoaded(Result<DaemonStatus, String>),
    ProfileLoaded(Result<UserProfile, String>),
    Refresh,
    AutoRefresh,
    FullReset,
    ToggleSyncPause,
}

pub struct Page {
    daemon_status: Option<DaemonStatus>,
    user_profile: Option<UserProfile>,
    loading: bool,
    error: Option<String>,
    subscribed: bool,
}

impl Page {
    pub fn new() -> Self {
        info!("Creating new StatusPage instance");
        Self {
            daemon_status: None,
            user_profile: None,
            loading: false,
            error: None,
            subscribed: false,
        }
    }
    pub fn subscription(&self) -> Subscription<Message> {
        // Periodic refresh for fallback and to flush latest signal
        time::every(Duration::from_secs(5)).map(|_| Message::AutoRefresh)
    }

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

    pub fn update(
        &mut self,
        message: Message,
    ) -> cosmic::Task<cosmic::Action<crate::app::Message>> {
        match message {
            Message::StatusSignal(status) => {
                self.daemon_status = Some(status);
                cosmic::Task::none()
            }
            Message::AutoRefresh => {
                // One-time subscription setup
                if !self.subscribed {
                    self.subscribed = true;
                    let subscribe_task = async move {
                        let _ = with_dbus_client(|client| async move {
                            let _ = client
                                .subscribe_daemon_status(|status| async move {
                                    let _ = status; // broadcast handled inside client
                                })
                                .await;
                            Ok::<(), String>(())
                        })
                        .await;
                    };
                    let _t: cosmic::Task<cosmic::Action<crate::app::Message>> = cosmic::task::future(subscribe_task).map(|_: ()| cosmic::Action::None);
                }
                // Try to flush latest status from broadcast first (no DBus call)
                let flush_latest = cosmic::task::future(async move { take_latest_status().await }).map(
                    |maybe_status| {
                        if let Some(status) = maybe_status {
                            cosmic::Action::App(crate::app::Message::StatusPage(Message::StatusSignal(status)))
                        } else {
                            cosmic::Action::None
                        }
                    },
                );

                let fetch_status =
                    with_dbus_client(|client| async move { client.get_daemon_status().await });
                let fetch_profile =
                    with_dbus_client(|client| async move { client.get_user_profile().await });

                let a = cosmic::task::future(fetch_status).map(|result| {
                    cosmic::Action::App(crate::app::Message::StatusPage(Message::StatusLoaded(
                        result,
                    )))
                });
                let b = cosmic::task::future(fetch_profile).map(|result| {
                    cosmic::Action::App(crate::app::Message::StatusPage(Message::ProfileLoaded(
                        result,
                    )))
                });
                cosmic::task::batch(vec![flush_latest, a, b])
            }
            Message::FullReset => {
                info!("StatusPage: Full reset requested");
                self.loading = true;
                self.error = None;
                let reset = async move {
                    match DbusClient::new().await {
                        Ok(client) => {
                            info!("StatusPage: Successfully created DbusClient");
                            match client.get_daemon_status().await {
                                Ok(status) => Ok(status),
                                Err(e) => Err(format!("Failed to get daemon status: {}", e)),
                            }
                        }
                        Err(e) => {
                            error!("StatusPage: Failed to create DbusClient - {}", e);
                            Err(format!("Failed to connect to daemon: {}", e))
                        }
                    }
                };
                cosmic::task::future(reset).map(|result| {
                    cosmic::Action::App(crate::app::Message::StatusPage(Message::StatusLoaded(
                        result,
                    )))
                })
            }
            Message::FetchStatus => {
                info!("StatusPage: Fetching status from daemon");
                self.loading = true;
                self.error = None;

                let fetch_status = async move {
                    match DbusClient::new().await {
                        Ok(client) => {
                            info!("StatusPage: Successfully created DbusClient");
                            match client.get_daemon_status().await {
                                Ok(status) => Ok(status),
                                Err(e) => Err(format!("Failed to get daemon status: {}", e)),
                            }
                        }
                        Err(e) => {
                            error!("StatusPage: Failed to create DbusClient - {}", e);
                            Err(format!("Failed to connect to daemon: {}", e))
                        }
                    }
                };
                let fetch_profile = async move {
                    match DbusClient::new().await {
                        Ok(client) => {
                            info!("StatusPage: Successfully created DbusClient");
                            match client.get_user_profile().await {
                                Ok(profile) => Ok(profile),
                                Err(e) => Err(format!("Failed to get user profile: {}", e)),
                            }
                        }
                        Err(e) => {
                            error!("StatusPage: Failed to create DbusClient - {}", e);
                            Err(format!("Failed to connect to daemon: {}", e))
                        }
                    }
                };

                let a = cosmic::task::future(fetch_status).map(|result| {
                    cosmic::Action::App(crate::app::Message::StatusPage(Message::StatusLoaded(
                        result,
                    )))
                });
                let b = cosmic::task::future(fetch_profile).map(|result| {
                    cosmic::Action::App(crate::app::Message::StatusPage(Message::ProfileLoaded(
                        result,
                    )))
                });
                cosmic::task::batch(vec![a, b])
            }

            Message::ProfileLoaded(result) => {
                self.loading = false;
                match result {
                    Ok(profile) => {
                        self.user_profile = Some(profile);
                        self.error = None;
                    }
                    Err(e) => {
                        self.user_profile = None;
                        self.error = Some(e);
                    }
                }
                cosmic::Task::none()
            }

            Message::StatusLoaded(result) => {
                self.loading = false;
                match result {
                    Ok(status) => {
                        info!("StatusPage: Successfully loaded daemon status - authenticated={}, connected={}, sync_status={:?}", 
                              status.is_authenticated, status.is_connected, status.sync_status);
                        self.daemon_status = Some(status);
                        self.error = None;
                    }
                    Err(e) => {
                        self.daemon_status = None;
                        self.error = Some(e);
                    }
                }
                cosmic::Task::none()
            }

            Message::Refresh => {
                info!("StatusPage: Manual refresh requested");
                self.loading = true;
                self.error = None;

                let fetch_status = async move {
                    match DbusClient::new().await {
                        Ok(client) => match client.get_daemon_status().await {
                            Ok(status) => Ok(status),
                            Err(e) => Err(format!("Failed to get daemon status: {}", e)),
                        },
                        Err(e) => Err(format!("Failed to connect to daemon: {}", e)),
                    }
                };

                cosmic::task::future(fetch_status).map(|result| {
                    cosmic::Action::App(crate::app::Message::StatusPage(Message::StatusLoaded(
                        result,
                    )))
                })
            }

            Message::ToggleSyncPause => {
                info!("StatusPage: Toggle sync pause requested");
                self.loading = true;
                self.error = None;

                let toggle_pause = async move {
                    match DbusClient::new().await {
                        Ok(client) => match client.toggle_sync_pause().await {
                            Ok(is_paused) => {
                                info!("StatusPage: Sync pause toggled: {}", if is_paused { "paused" } else { "resumed" });
                                // After toggling, fetch the updated status
                                match client.get_daemon_status().await {
                                    Ok(status) => Ok(status),
                                    Err(e) => Err(format!("Failed to get daemon status after toggle: {}", e)),
                                }
                            },
                            Err(e) => Err(format!("Failed to toggle sync pause: {}", e)),
                        },
                        Err(e) => Err(format!("Failed to connect to daemon: {}", e)),
                    }
                };

                cosmic::task::future(toggle_pause).map(|result| {
                    cosmic::Action::App(crate::app::Message::StatusPage(Message::StatusLoaded(
                        result,
                    )))
                })
            }
        }
    }
}
