// SPDX-License-Identifier: MPL-2.0

use cosmic::{
    iced::{Alignment, Length, Subscription, time, widget::{button, column, row, container, scrollable}},
    style,
};
use cosmic::widget::text;
use onedrive_sync_lib::dbus::types::{ConflictItem, UserChoice};
use crate::dbus_client::{self, DbusClient};
use std::time::Duration;
use super::message::Message;

pub struct ConflictsPage {
    conflicts: Result<Vec<ConflictItem>, String>,
}

impl ConflictsPage {
    pub fn new() -> Self {
        Self {
            conflicts: Err("Loading...".into()),
        }
    }

    pub fn subscription(&self) -> Subscription<Message> {
        time::every(Duration::from_secs(5)).map(|_| Message::AutoRefresh)
    }

    pub fn update(&mut self, message: Message) -> cosmic::Task<cosmic::Action<crate::app::Message>> {
        match message {
            Message::AutoRefresh | Message::Reload => {
                let load_conflicts = async {
                    dbus_client::with_dbus_client(|client: DbusClient| async move {
                        client.get_conflicts().await
                    })
                    .await
                };
                cosmic::task::future(load_conflicts).map(|result| {
                    cosmic::Action::App(crate::app::Message::ConflictsPage(Message::Loaded(result)))
                })
            }
            Message::Loaded(conflicts) => {
                self.conflicts = conflicts;
                cosmic::Task::none()
            }
            Message::Resolve { db_id, choice } => {
                let resolve_conflict = async move {
                    dbus_client::with_dbus_client(move |client: DbusClient| async move {
                        client.resolve_conflict(db_id, choice).await
                    })
                    .await
                };
                cosmic::task::future(resolve_conflict).map(|result| {
                    cosmic::Action::App(crate::app::Message::ConflictsPage(Message::Resolved(result)))
                })
            }
            Message::Resolved(Ok(_)) => {
                // Reload the list of conflicts after a successful resolution
                let load_conflicts = async {
                    dbus_client::with_dbus_client(|client: DbusClient| async move {
                        client.get_conflicts().await
                    })
                    .await
                };
                cosmic::task::future(load_conflicts).map(|result| {
                    cosmic::Action::App(crate::app::Message::ConflictsPage(Message::Loaded(result)))
                })
            }
            Message::Resolved(Err(e)) => {
                self.conflicts = Err(e);
                cosmic::Task::none()
            }
        }
    }

    pub fn view(&self) -> cosmic::Element<Message> {
        let content: cosmic::Element<Message> = match &self.conflicts {
            Ok(conflicts) if conflicts.is_empty() => {
                column(vec![])
                    .spacing(10)
                    .align_x(Alignment::Center)
                    .push(text::body("No conflicts found.").size(24))
                    .into()
            }
            Ok(conflicts) => {
                // Group conflicts by onedrive_id
                let mut grouped_conflicts: std::collections::HashMap<String, Vec<&ConflictItem>> = std::collections::HashMap::new();
                for conflict in conflicts {
                    grouped_conflicts.entry(conflict.onedrive_id.clone()).or_insert_with(Vec::new).push(conflict);
                }

                let list = grouped_conflicts.iter().fold(column(vec![]), |col, (onedrive_id, conflict_items)| {
                    // Find local and remote changes
                    let local_change = conflict_items.iter().find(|item| item.change_type == "local" && item.onedrive_id.as_str() ==  onedrive_id.as_str());
                    let remote_change = conflict_items.iter().find(|item| item.change_type == "remote" && item.onedrive_id.as_str() ==  onedrive_id.as_str());
                    
                    // Use the first item for the buttons (they all have the same onedrive_id)
                    let first_item = conflict_items.first().unwrap();
                    
                    let mut conflict_group = column(vec![])
                        .spacing(15)
                        .align_x(Alignment::Start)
                        .push(
                            // Header with onedrive_id
                            text::body(format!("Conflict ID: {}", onedrive_id)).size(16)
                        );
                    
                    // Local change section
                    if let Some(local) = local_change {
                        conflict_group = conflict_group.push(
                            column(vec![])
                                .spacing(4)
                                .push(text::body("Local Change:").size(14))
                                .push(text::body(&local.name).size(16))
                                .push(text::body(&local.path).size(14))
                                .push(text::body(&local.error_message).size(14))
                        );
                    }
                    
                    // Remote change section
                    if let Some(remote) = remote_change {
                        conflict_group = conflict_group.push(
                            column(vec![])
                                .spacing(4)
                                .push(text::body("Remote Change:").size(14))
                                .push(text::body(&remote.name).size(16))
                                .push(text::body(&remote.path).size(14))
                                .push(text::body(&remote.error_message).size(14))
                        );
                    }
                    
                    // Action buttons (only one set per group)
                    conflict_group = conflict_group.push(
                        row(vec![])
                            .spacing(10)
                            .align_y(Alignment::Center)
                            .push(
                                button("Keep Local").on_press(Message::Resolve { 
                                    db_id: first_item.db_id, 
                                    choice: UserChoice::KeepLocal 
                                })
                            )
                            .push(
                                button("Use Remote").on_press(Message::Resolve { 
                                    db_id: first_item.db_id, 
                                    choice: UserChoice::UseRemote 
                                })
                            )
                    );
                    
                    col.push(container(conflict_group).padding(15).class(style::Container::Card))
                });
                
                column(vec![])
                    .spacing(20)
                    .push(text::body("The following conflicts require your attention:").size(20))
                    .push(scrollable(list.spacing(15)))
                    .into()
            }
            Err(e) => {
                column(vec![])
                    .spacing(10)
                    .align_x(Alignment::Center)
                    .push(text::body("Failed to load conflicts:").size(24))
                    .push(text::body(e.clone()))
                    .into()
            }
        };

        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .padding(20)
            .into()
    }
}
