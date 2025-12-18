// SPDX-License-Identifier: MPL-2.0

use std::time::Duration;
use crate::dbus_client::{with_dbus_client, DbusClient, take_latest_status};
use cosmic::iced::{time, Subscription};
use log::{error, info};
use onedrive_sync_lib::dbus::types::{DaemonStatus, UserProfile};
use super::message::Message;

pub struct Page {
    pub daemon_status: Option<DaemonStatus>,
    pub user_profile: Option<UserProfile>,
    pub loading: bool,
    pub error: Option<String>,
    pub subscribed: bool,
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

