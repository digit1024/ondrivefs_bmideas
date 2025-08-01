use std::time::Duration;

use cosmic::widget::{self, button, column, container, row, text, card, icon};
use cosmic::iced::{time, Alignment, Length, Subscription};
use cosmic::theme;
use log::{error, info};
use crate::dbus_client::DbusClient;
use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConflictItem {
    pub id: i64,
    pub file_name: String,
    pub path: String,
    pub local_modified: String,
    pub remote_modified: String,
    pub local_size: u64,
    pub remote_size: u64,
    pub conflict_type: String,
    pub is_downloaded: bool,
    pub resolution_status: Option<String>,
}

#[derive(Debug, Clone)]
pub enum Message {
    FetchConflicts,
    ConflictsLoaded(Result<Vec<ConflictItem>, String>),
    Refresh,
    ResolveConflict(i64, String), // item_id, resolution_type
    AutoRefresh,
    SelectItem(i64),
}

pub struct Page {
    pub conflicts: Vec<ConflictItem>,
    pub loading: bool,
    pub error: Option<String>,
    pub selected_item: Option<i64>,
}

impl Page {
    pub fn new() -> Self {
        Self {
            conflicts: Vec::new(),
            loading: false,
            error: None,
            selected_item: None,
        }
    }

    pub fn subscription(&self) -> Subscription<Message> {
        time::every(Duration::from_secs(10)).map(|_| Message::AutoRefresh)
    }

    pub fn view(&self) -> cosmic::Element<Message> {
        let spacing = theme::active().cosmic().spacing.space_m;
        let mut content = column()
            .spacing(spacing)
            .width(Length::Fill)
            .height(Length::Fill);

        // Header
        let header = row()
            .spacing(spacing)
            .push(text::title2("🔄 Conflict Resolution").size(24))
            .push(container(
                button::standard("Refresh")
                    .on_press(Message::Refresh)
            ).align_x(Alignment::End).width(Length::Fill));

        // Loading indicator
        if self.loading {
            content = content.push(container(text::body("Loading conflicts...").size(16)).padding(8));
        }

        // Error display
        if let Some(error) = &self.error {
            content = content.push(
                container(
                    text::body(format!("❌ Error: {}", error))
                        .size(14)
                        
                ).padding(8)
            );
        }

        // Conflicts list
        let conflicts_content = if self.conflicts.is_empty() {
            container(
                column()
                    .push(text::title3("✅ No conflicts detected").size(18))
                    .push(text::body("All files are synchronized").size(14))
                    
            )
            .padding(spacing * 4)
            .width(Length::Fill)
        } else {
            let mut conflicts_column = column().spacing(spacing);
            
            for conflict in &self.conflicts {
                conflicts_column = conflicts_column.push(self.conflict_card(conflict));
            }
            
            container(cosmic::widget::scrollable::vertical(conflicts_column))
                .width(Length::Fill)
                .height(Length::Fill)
        };

        content
            .push(header)
            .push(conflicts_content)
            .into()
    }

    fn conflict_card<'a>(&self, conflict: &'a ConflictItem) -> cosmic::Element<'a, Message> {
        let spacing = theme::active().cosmic().spacing.space_s;
        let is_selected = self.selected_item == Some(conflict.id);
        
        let mut card_content = column().spacing(spacing);

        // File info header
        let header = row()
            .spacing(spacing)
            .push(icon::from_name("document").size(24))
            .push(column()
                .push(text::title4(&conflict.file_name).size(16))
                .push(text::body(&conflict.path).size(12))
            );
        
        card_content = card_content.push(header);
        card_content = card_content.push(widget::divider::horizontal::default());

        // Conflict details
        let details = row()
            .spacing(spacing * 2)
            .push(
                column()
                    .push(text::heading("📁 Local Version").size(14))
                    .push(text::body(format!("Modified: {}", conflict.local_modified)).size(12))
                    .push(text::body(format!("Size: {} bytes", conflict.local_size)).size(12))
                    .width(Length::FillPortion(1))
            )
            .push(
                container(text::heading("⚔️").size(20))
                    .align_x(Alignment::Center)
                    .width(Length::Shrink)
            )
            .push(
                column()
                    .push(text::heading("☁️ Remote Version").size(14))
                    .push(text::body(format!("Modified: {}", conflict.remote_modified)).size(12))
                    .push(text::body(format!("Size: {} bytes", conflict.remote_size)).size(12))
                    .width(Length::FillPortion(1))
            );
        
        card_content = card_content.push(details);

        // Status indicator
        let status_text = if conflict.is_downloaded {
            "📥 File is downloaded locally"
        } else {
            "☁️ File not downloaded (placeholder only)"
        };
        card_content = card_content.push(text::body(status_text).size(12));

        // Resolution buttons
        if is_selected {
            card_content = card_content.push(widget::divider::horizontal::default());
            
            let resolution_buttons = row()
                .spacing(spacing)
                .push(
                    button::suggested("Use Local")
                        .on_press(Message::ResolveConflict(conflict.id, "use_local".to_string()))
                )
                .push(
                    button::standard("Use Remote")
                        .on_press(Message::ResolveConflict(conflict.id, "use_remote".to_string()))
                )
                .push(
                    button::standard("Keep Both")
                        .on_press(Message::ResolveConflict(conflict.id, "keep_both".to_string()))
                )
                .push(
                    button::standard("Skip")
                        .on_press(Message::ResolveConflict(conflict.id, "skip".to_string()))
                );
            
            card_content = card_content.push(resolution_buttons);
        }

        // Wrap in a clickable card
        let card = container(card_content)
            .class(cosmic::style::Container::Card)
            .padding(16);
            //.on_press(Message::SelectItem(conflict.id));

        container(card)
            .width(Length::Fill)
            .into()
    }

    pub fn update(&mut self, message: Message) -> cosmic::Task<cosmic::Action<crate::app::Message>> {
        match message {
            Message::FetchConflicts | Message::AutoRefresh => {
                self.loading = true;
                let fut = async {
                    match DbusClient::new().await {
                        Ok(_client) => {
                            // TODO: Implement actual D-Bus method for getting conflicts
                            // For now, return mock data
                            Ok(vec![])
                        }
                        Err(e) => Err(e.to_string()),
                    }
                };
                cosmic::task::future(fut).map(|result| {
                    cosmic::Action::App(crate::app::Message::ConflictsPage(
                        Message::ConflictsLoaded(result),
                    ))
                })
            }
            Message::ConflictsLoaded(result) => {
                self.loading = false;
                match result {
                    Ok(conflicts) => {
                        self.conflicts = conflicts;
                        self.error = None;
                    }
                    Err(e) => {
                        self.error = Some(e);
                    }
                }
                cosmic::Task::none()
            }
            Message::Refresh => {
                self.update(Message::FetchConflicts)
            }
            Message::ResolveConflict(item_id, resolution) => {
                info!("Resolving conflict {} with strategy: {}", item_id, resolution);
                let fut = async move {
                    match DbusClient::new().await {
                        Ok(_client) => {
                            // TODO: Implement actual D-Bus method for resolving conflicts
                            // client.resolve_conflict(item_id, resolution).await
                            Ok(())
                        }
                        Err(e) => Err(e.to_string()),
                    }
                };
                cosmic::task::future(fut).map(|_: Result<(), String>| {
                    cosmic::Action::App(crate::app::Message::ConflictsPage(Message::Refresh))
                })
            }
            Message::SelectItem(id) => {
                if self.selected_item == Some(id) {
                    self.selected_item = None;
                } else {
                    self.selected_item = Some(id);
                }
                cosmic::Task::none()
            }
        }
    }
}
