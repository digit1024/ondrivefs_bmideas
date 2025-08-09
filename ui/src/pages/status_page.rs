use std::time::Duration;

use crate::dbus_client::{with_dbus_client, DbusClient};
use cosmic::iced::alignment::Horizontal;
use cosmic::iced::{time, Alignment, Length, Subscription};
use cosmic::widget::{self, button, column, container, row, text};
use log::{error, info};
use onedrive_sync_lib::dbus::types::{DaemonStatus, UserProfile};

const ICON_TRUE: &[u8] = include_bytes!("../../../resources/programfiles/icons/ok.png");
const ICON_FALSE: &[u8] = include_bytes!("../../../resources/programfiles/icons/error.png");

#[derive(Debug, Clone)]
pub enum Message {
    FetchStatus,
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
        // Periodic refresh for fallback
        time::every(Duration::from_secs(30)).map(|_| Message::AutoRefresh)
    }

    pub fn view(&self) -> cosmic::Element<Message> {
        let spacing = cosmic::theme::active().cosmic().spacing.space_l;
        let content = column()
            .spacing(spacing)
            .width(Length::Fill)
            .height(Length::Fill);

        // Header: Welcome message or fallback
        let header = if let Some(profile) = &self.user_profile {
            text::title2(format!("Welcome {} in OneDrive Client", profile.given_name)).size(24)
        } else {
            text::title2("OneDrive Sync Status").size(24)
        };

        // Refresh button aligned right
        let refresh_row = row().width(Length::Fill).push(
            column().push(header).push(
                row()
                    .push(button::standard("Refresh").on_press(Message::Refresh))
                    .push(button::standard("Pause/Resume Sync").on_press(Message::ToggleSyncPause))
                    .push(button::destructive("Full Reset").on_press(Message::FullReset))
                    .width(Length::Fill),
            ),
        );

        // Status and profile as cards
        let status_card = container(self.create_status_section())
            .class(cosmic::style::Container::Card)
            .padding(16)
            .width(Length::Fill);

        let profile_card = container(self.create_profile_section())
            .class(cosmic::style::Container::Card)
            .padding(16)
            .width(Length::Fill);

        // Loading indicator
        let loading_indicator = if self.loading {
            container(text::body("Loading status...").size(16))
                .padding(8)
                .width(Length::Fill)
        } else {
            container(column()).width(Length::Fill)
        };

        // Error display
        let error_display = if let Some(error) = &self.error {
            container(text::body(format!("Error: {}", error)).size(14))
                .padding(8)
                .width(Length::Fill)
        } else {
            container(column()).width(Length::Fill)
        };

        content
            .push(refresh_row)
            .push(loading_indicator)
            .push(error_display)
            .push(profile_card)
            .push(status_card)
            .into()
    }

    fn create_status_section(&self) -> cosmic::Element<Message> {
        let spacing = cosmic::theme::active().cosmic().spacing.space_m;

        let title = text::title3("Daemon Status").size(18);

        let status_content = if let Some(status) = &self.daemon_status {
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

    fn create_profile_section(&self) -> cosmic::Element<Message> {
        let spacing = cosmic::theme::active().cosmic().spacing.space_m;

        let title = text::title3("User Profile").size(18);

        let profile_content = if let Some(profile) = &self.user_profile {
            column()
                .spacing(spacing)
                .align_x(Horizontal::Left)
                .push(self.create_profile_row("Name", &profile.display_name))
                .push(self.create_profile_row("Email", &profile.mail))
        } else {
            column()
                .spacing(spacing)
                .push(text::body("No profile data available").size(14))
        };

        column()
            .spacing(spacing)
            .push(title)
            .push(profile_content)
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

    fn create_profile_row(&self, label: &str, value: &str) -> cosmic::Element<Message> {
        row()
            .spacing(cosmic::theme::active().cosmic().spacing.space_s)
            .align_y(Alignment::Center)
            .push(
                text::body(label.to_string())
                    .size(14)
                    .width(Length::Fixed(120.0)),
            )
            .push(text::body(value.to_string()).size(14))
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
                                    // This callback cannot directly send a Message; use polling fallback
                                    let _ = status; // no-op
                                })
                                .await;
                            Ok::<(), String>(())
                        })
                        .await;
                    };
                    let _t: cosmic::Task<cosmic::Action<crate::app::Message>> = cosmic::task::future(subscribe_task).map(|_: ()| cosmic::Action::None);
                }
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
                cosmic::task::batch(vec![a, b])
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
