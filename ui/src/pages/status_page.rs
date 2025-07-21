use std::time::Duration;

use cosmic::iced::alignment::Horizontal;
use cosmic::iced_wgpu::graphics::image::image_rs::codecs::png;
use cosmic::widget::{self, button, column, container, row, svg, text };
use cosmic::iced::{time, Alignment, Length, Subscription};
use log::{error, info};
use onedrive_sync_lib::dbus::types::{DaemonStatus, UserProfile, SyncStatus};
use crate::dbus_client::DbusClient;
use cosmic::{cosmic_theme, iced_core, theme};

const ICON_ONLINE: &[u8] = include_bytes!("../../../resources/programfiles/icons/online.svg");
const ICON_SYNCING: &[u8] = include_bytes!("../../../resources/programfiles/icons/syncing.svg");
const ICON_ERROR: &[u8] = include_bytes!("../../../resources/programfiles/icons/error.png");
const ICON_CONFLICT: &[u8] = include_bytes!("../../../resources/programfiles/icons/conflict.png");
const ICON_TRUE: &[u8] = include_bytes!("../../../resources/programfiles/icons/ok.png");
const ICON_FALSE: &[u8] = include_bytes!("../../../resources/programfiles/icons/error.png");


#[derive(Debug, Clone)]
pub enum Message {
    FetchStatus,
    StatusLoaded(Result<DaemonStatus, String>),
    ProfileLoaded(Result<UserProfile, String>),
    Refresh,
    AutoRefresh,
}

pub struct Page {
    daemon_status: Option<DaemonStatus>,
    user_profile: Option<UserProfile>,
    loading: bool,
    error: Option<String>,
}

impl Page {
    pub fn new() -> Self {
        info!("Creating new StatusPage instance");
        Self {
            daemon_status: None,
            user_profile: None,
            loading: false,
            error: None,
        }
    }
    pub fn subscription(&self) -> Subscription<Message> {
        time::every(Duration::from_secs(5)).map(|_| Message::AutoRefresh)
    }


    pub fn view(&self) -> cosmic::Element<Message> {
        let spacing = cosmic::theme::active().cosmic().spacing.space_l;
        let mut content = column()
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
        let refresh_row = row()
            .width(Length::Fill)
            .push(
                column().push(header).push(
                    container(button::standard("Refresh")
                    .on_press(Message::Refresh))
                    .align_x(Alignment::End)
                    .width(Length::Fill)
                )
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
            container(
                text::body("Loading status...").size(16)
            ).padding(8).width(Length::Fill)
        } else {
            container(column()).width(Length::Fill)
        };

        // Error display
        let error_display = if let Some(error) = &self.error {
            container(
                text::body(format!("Error: {}", error)).size(14)
            ).padding(8).width(Length::Fill)
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
        
        let title = text::title3("Daemon Status")
            .size(18);

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
                .push(
                    text::body("No status data available")
                        .size(14)
                )
        };

        column()
            .spacing(spacing)
            .push(title)
            .push(status_content)
            .into()
    }

    fn create_profile_section(&self) -> cosmic::Element<Message> {
        let spacing = cosmic::theme::active().cosmic().spacing.space_m;
        
        let title = text::title3("User Profile")
            .size(18);

        let profile_content = if let Some(profile) = &self.user_profile {
            column()
                .spacing(spacing)
                .align_x(Horizontal::Left)
                
                .push(self.create_profile_row("Name", &profile.display_name))
                .push(self.create_profile_row("Email", &profile.mail))
        } else {
            column()
                .spacing(spacing)
                .push(
                    text::body("No profile data available")
                        .size(14)
                )
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
                    .width(Length::Fixed(120.0))
            )
            .push(
                icon.height(Length::Fixed(32.0)).width(Length::Fixed(32.0))
            )
            .into()
    }

   

    fn create_profile_row(&self, label: &str, value: &str) -> cosmic::Element<Message> {
        row()
            .spacing(cosmic::theme::active().cosmic().spacing.space_s)
            .align_y(Alignment::Center)
            .push(
                text::body(label.to_string())
                    .size(14)
                    .width(Length::Fixed(120.0))
            )
            .push(
                text::body(value.to_string())
                    .size(14)
            )
            .into()
    }
    
    pub fn update(&mut self, message: Message) -> cosmic::Task<cosmic::Action<crate::app::Message>> {
        match message {
            Message::AutoRefresh => {
             
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
                    cosmic::Action::App(crate::app::Message::StatusPage(Message::StatusLoaded(result)))
                });
                let b = cosmic::task::future(fetch_profile).map(|result| {
                    cosmic::Action::App(crate::app::Message::StatusPage(Message::ProfileLoaded(result)))
                });
                cosmic::task::batch(vec![a, b])
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
                    cosmic::Action::App(crate::app::Message::StatusPage(Message::StatusLoaded(result)))
                });
                let b = cosmic::task::future(fetch_profile).map(|result| {
                    cosmic::Action::App(crate::app::Message::StatusPage(Message::ProfileLoaded(result)))
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
                        Ok(client) => {
                            match client.get_daemon_status().await {
                                Ok(status) => Ok(status),
                                Err(e) => Err(format!("Failed to get daemon status: {}", e)),
                            }
                        }
                        Err(e) => Err(format!("Failed to connect to daemon: {}", e)),
                    }
                };

                cosmic::task::future(fetch_status).map(|result| {
                    cosmic::Action::App(crate::app::Message::StatusPage(Message::StatusLoaded(result)))
                })
            }
        }
    }
}
