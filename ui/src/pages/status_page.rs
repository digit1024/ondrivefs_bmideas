use cosmic::widget::{button, column, row, text};
use cosmic::iced::{Alignment, Length};
use log::{error, info};
use onedrive_sync_lib::dbus::types::{DaemonStatus, UserProfile, SyncStatus};
use crate::dbus_client::DbusClient;


#[derive(Debug, Clone)]
pub enum Message {
    FetchStatus,
    StatusLoaded(DaemonStatus),
   // FetchProfile,
    ProfileLoaded(UserProfile),
    Refresh,
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

    pub fn view(&self) -> cosmic::Element<Message> {
        let spacing = cosmic::theme::active().cosmic().spacing.space_l;
        
        // Main content column
        let content = column()
            .spacing(spacing)
            .width(Length::Fill)
            .height(Length::Fill);

        // Header section
        let header = column()
            .spacing(cosmic::theme::active().cosmic().spacing.space_s)
            .push(
                text::title2("OneDrive Sync Status")
                    .size(24)
            )
            .push(
                button::standard("Refresh")
                    .on_press(Message::Refresh)
            );

        // Status cards
        let status_section = self.create_status_section();
        let profile_section = self.create_profile_section();

        // Loading indicator
        let loading_indicator = if self.loading {
            column()
                .spacing(cosmic::theme::active().cosmic().spacing.space_s)
                .push(
                    text::body("Loading status...")
                        .size(16)
                )
        } else {
            column()
        };

        // Error display
        let error_display = if let Some(error) = &self.error {
            column()
                .spacing(cosmic::theme::active().cosmic().spacing.space_s)
                .push(
                    text::body(format!("Error: {}", error))
                        .size(14)
                )
        } else {
            column()
        };

        content
            .push(header)
            .push(loading_indicator)
            .push(error_display)
            .push(status_section)
            .push(profile_section)
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
                .push(self.create_sync_status_row(&status.sync_status))
                .push(self.create_status_row("Conflicts", status.has_conflicts))
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
                .push(self.create_profile_row("Display Name", &profile.display_name))
                .push(self.create_profile_row("Given Name", &profile.given_name))
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
        let status_text = if value { "Connected" } else { "Disconnected" };

        row()
            .spacing(cosmic::theme::active().cosmic().spacing.space_s)
            .align_y(Alignment::Center)
            .push(
                text::body(label.to_string())
                    .size(14)
                    .width(Length::Fixed(120.0))
            )
            .push(
                text::body(status_text.to_string())
                    .size(14)
            )
            .into()
    }

    fn create_sync_status_row(&self, sync_status: &SyncStatus) -> cosmic::Element<Message> {
        let status_text = match sync_status {
            SyncStatus::Running => "Running",
            SyncStatus::Paused => "Paused",
            SyncStatus::Error => "Error",
        };

        row()
            .spacing(cosmic::theme::active().cosmic().spacing.space_s)
            .align_y(Alignment::Center)
            .push(
                text::body("Sync Status".to_string())
                    .size(14)
                    .width(Length::Fixed(120.0))
            )
            .push(
                text::body(status_text.to_string())
                    .size(14)
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
            Message::FetchStatus => {
                info!("StatusPage: Fetching status from daemon");
                self.loading = true;
                self.error = None;

                
                let fetch_status = async move {
                    
        
                    match DbusClient::new().await {
                        Ok(client) => {
                            info!("StatusPage: Successfully created DbusClient");
                            client.get_daemon_status().await
                        }
                        Err(e) => {
                            error!("StatusPage: Failed to create DbusClient - {}", e);
                            Err(e)
                        }
                    }
                };
                let fetch_profile = async move {
                    match DbusClient::new().await {
                        Ok(client) => {
                            info!("StatusPage: Successfully created DbusClient");
                            client.get_user_profile().await
                        }
                        Err(e) => {
                            error!("StatusPage: Failed to create DbusClient - {}", e);
                            Err(e)
                        }
                    }
                };

                
                
                 let a =  cosmic::task::future(fetch_status).map(|status: Result<DaemonStatus, _>| {
                     cosmic::Action::App(crate::app::Message::StatusPage(Message::StatusLoaded(status.unwrap())))
                 });
                 let b = cosmic::task::future(fetch_profile).map(|profile: Result<UserProfile, _>| {
                    cosmic::Action::App(crate::app::Message::StatusPage(Message::ProfileLoaded(profile.unwrap())))
                 });
                 cosmic::task::batch(vec![a, b])
            }

            Message::ProfileLoaded(profile) => {
                self.user_profile = Some(profile);
                self.loading = false;
                self.error = None;
                cosmic::Task::none()
            }

            Message::StatusLoaded(status) => {
                self.loading = false;
                        info!("StatusPage: Successfully loaded daemon status - authenticated={}, connected={}, sync_status={:?}", 
                              status.is_authenticated, status.is_connected, status.sync_status);
                        self.daemon_status = Some(status);
                        self.error = None;
                
                
                cosmic::Task::none()
            }

            Message::Refresh => {
                info!("StatusPage: Manual refresh requested");
                self.loading = true;
                self.error = None;
                
                let fetch_status = async move {
                    match DbusClient::new().await {
                        Ok(client) => {
                            client.get_daemon_status().await
                        }
                        Err(e) => {
                            error!("StatusPage: Failed to load daemon status - {}", e);
                            Err(e)
                        }
                    }
                };
                

                 cosmic::task::future(fetch_status).map(|status: Result<DaemonStatus, _>| {
                     cosmic::Action::App(crate::app::Message::StatusPage(Message::StatusLoaded(status.unwrap())))
                 })
            }
        }
    }
}
