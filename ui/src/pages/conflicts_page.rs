// SPDX-License-Identifier: MPL-2.0

use cosmic::{
    iced::{Alignment, Length, widget::{button, column, row, container, scrollable}},
    style,
};
use cosmic::widget::text;
use onedrive_sync_lib::dbus::types::{ConflictItem, UserChoice};

use crate::dbus_client::{self, DbusClient};

#[derive(Debug, Clone)]
pub enum Message {
    Resolve { db_id: i64, choice: UserChoice },
    Resolved(Result<(), String>),
    Loaded(Result<Vec<ConflictItem>, String>),
    Reload,
}

pub struct ConflictsPage {
    conflicts: Result<Vec<ConflictItem>, String>,
}

impl ConflictsPage {
    pub fn new() -> Self {
        Self {
            conflicts: Err("Loading...".into()),
        }
    }

    pub fn update(&mut self, message: Message) -> cosmic::Task<cosmic::Action<crate::app::Message>> {
        match message {
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
                eprintln!("Failed to resolve conflict: {}", e);
                cosmic::Task::none()
            }
            Message::Reload => {
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
        }
    }

    pub fn view(&self) -> cosmic::Element<Message> {
        let content: cosmic::Element<Message> = match &self.conflicts {
            Ok(conflicts) if conflicts.is_empty() => {
                column![
                    text::body("No conflicts found.").size(24),
                    button("Check Again").on_press(Message::Reload)
                ]
                .spacing(10)
                .align_x(Alignment::Center)
                .into()
            }
            Ok(conflicts) => {
                let list = conflicts.iter().fold(column![], |col, item| {
                    let conflict_row = row![
                        column![
                            text::body(&item.name),
                            text::body(&item.path).size(14),
                            text::body(&item.error_message).size(14),
                        ].spacing(4),
                        row![
                            button("Keep Local").on_press(Message::Resolve { db_id: item.db_id, choice: UserChoice::KeepLocal }),
                            button("Use Remote").on_press(Message::Resolve { db_id: item.db_id, choice: UserChoice::UseRemote })
                        ].spacing(10).align_y(Alignment::Center)
                    ]
                    .spacing(20)
                    .align_y(Alignment::Center);
                    
                    col.push(container(conflict_row).padding(10).class(style::Container::Card))
                });
                column![
                    text::body("The following conflicts require your attention:").size(20),
                    scrollable(list.spacing(10))
                ]
                .spacing(20)
                .into()
            }
            Err(e) => {
                column![
                    text::body("Failed to load conflicts:").size(24),
                    text::body(e),
                    button("Retry").on_press(Message::Reload)
                ]
                .spacing(10)
                .align_x(Alignment::Center)
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
