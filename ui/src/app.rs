// SPDX-License-Identifier: MPL-2.0

use crate::config::Config;
use crate::fl;
use cosmic::app::context_drawer;
use cosmic::cosmic_config::{self, CosmicConfigEntry};

use cosmic::iced::{Alignment, Length, Subscription};
use crate::notifications::{NotificationSender, NotificationUrgency};

use cosmic::prelude::*;
use cosmic::widget::{self, button, column, icon, menu, nav_bar, row, text, Column, Row};


use cosmic::{cosmic_theme, theme};
use std::collections::HashMap;

const REPOSITORY: &str = env!("CARGO_PKG_REPOSITORY");
const APP_ICON: &[u8] = include_bytes!("../resources/icons/hicolor/scalable/apps/icon.svg");

/// The application model stores app-specific state used to describe its interface and
/// drive its logic.
pub struct AppModel {
    /// Application state which is managed by the COSMIC runtime.
    core: cosmic::Core,
    /// Display a context drawer with the designated page if defined.
    context_page: ContextPage,
    /// Contains items assigned to the nav bar panel.
    nav: nav_bar::Model,
    /// Key bindings for the application's menu bar.
    key_binds: HashMap<menu::KeyBind, MenuAction>,
    /// Configuration data that persists between application runs.
    config: Config,
    /// Current sync status
    sync_status: String,
    /// List of sync folders
    sync_folders: Vec<String>,
    /// Sync progress (current, total)
    sync_progress: (u32, u32),
    /// Download queue size
    download_queue_size: u32,
    /// Last sync time
    last_sync_time: String,
    /// Mount point
    mount_point: String,
    /// Error message if any
    error_message: Option<String>,
    /// Notification sender for desktop notifications
    notification_sender: Option<NotificationSender>,
}

/// Messages emitted by the application and its widgets.
#[derive(Debug, Clone)]
pub enum Message {
    // Navigation and UI
    OpenRepositoryUrl,
    ToggleContextPage(ContextPage),
    UpdateConfig(Config),
    LaunchUrl(String),
    
    // Simple UI actions
    PauseSync,
    ResumeSync,
    RefreshStatus,
    AddSyncFolder,
    RemoveSyncFolder(String),
    DisplayNotification,
    
    // Notification system
    NotificationSenderInitialized(Result<NotificationSender, String>),
}

/// Create a COSMIC application from the app model
impl cosmic::Application for AppModel {
    /// The async executor that will be used to run your application's commands.
    type Executor = cosmic::executor::Default;

    /// Data that your application receives to its init method.
    type Flags = ();

    /// Messages which the application and its widgets will emit.
    type Message = Message;

    /// Unique identifier in RDNN (reverse domain name notation) format.
    const APP_ID: &'static str = "com.github.bmideas.onedrive-sync";

    fn core(&self) -> &cosmic::Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut cosmic::Core {
        &mut self.core
    }

    /// Initializes the application with any given flags and startup commands.
    fn init(
        core: cosmic::Core,
        _flags: Self::Flags,
    ) -> (Self, Task<cosmic::Action<Self::Message>>) {
        // Create a nav bar with three page items.
        let mut nav = nav_bar::Model::default();
        

        nav.insert()
            .text("Status")
            .data::<Page>(Page::Status)
            .icon(icon::from_name("applications-system-symbolic"))
            .activate();

        nav.insert()
            .text("Folders")
            .data::<Page>(Page::Folders)
            .icon(icon::from_name("folder-symbolic"));

        nav.insert()
            .text("Settings")
            .data::<Page>(Page::Settings)
            .icon(icon::from_name("applications-science-symbolic"));

        // Construct the app model with the runtime's core.
        let mut app = AppModel {
            core,
            context_page: ContextPage::default(),
            nav,
            key_binds: HashMap::new(),
            config: cosmic_config::Config::new(Self::APP_ID, Config::VERSION)
                .map(|context| match Config::get_entry(&context) {
                    Ok(config) => config,
                    Err((_errors, config)) => config
                })
                .unwrap_or_default(),
            sync_status: "Unknown".to_string(),
            sync_folders: vec!["/home/user/Documents".to_string(), "/home/user/Pictures".to_string()],
            sync_progress: (0, 0),
            download_queue_size: 0,
            last_sync_time: "Never".to_string(),
            mount_point: "/tmp/onedrive".to_string(),
            error_message: None,
            notification_sender: None,
        };

        // Create startup commands: set window title and initialize notification sender
        let title_command = app.update_title();
        let notification_command = Task::perform(
            async move {
                match NotificationSender::new().await {
                    Ok(sender) => Message::NotificationSenderInitialized(Ok(sender)),
                    Err(e) => Message::NotificationSenderInitialized(Err(e.to_string())),
                }
            },
            |result| cosmic::Action::App(result),
        );

        (app, Task::batch(vec![title_command, notification_command]))
    }

    /// Elements to pack at the start of the header bar.
    fn header_start(&self) -> Vec<Element<Self::Message>> {
        let menu_bar = menu::bar(vec![menu::Tree::with_children(
            menu::root(fl!("view")).apply(Element::from),
            menu::items(
                &self.key_binds,
                vec![menu::Item::Button(fl!("about"), None, MenuAction::About)],
            ),
        )]);

        vec![menu_bar.into()]
    }

    /// Enables the COSMIC application to create a nav bar with this model.
    fn nav_model(&self) -> Option<&nav_bar::Model> {
        Some(&self.nav)
    }

    /// Display a context drawer if the context page is requested.
    fn context_drawer(&self) -> Option<context_drawer::ContextDrawer<Self::Message>> {
        if !self.core.window.show_context {
            return None;
        }

        Some(match self.context_page {
            ContextPage::About => context_drawer::context_drawer(
                self.about(),
                Message::ToggleContextPage(ContextPage::About),
            )
            .title(fl!("about")),
        })
    }

    /// Describes the interface based on the current state of the application model.
    fn view(&self) -> Element<Self::Message> {
        let page = self.nav.active_data::<Page>().unwrap_or(&Page::Status);
        
        let content = match page {
            Page::Status => self.status_page(),
            Page::Folders => self.folders_page(),
            Page::Settings => self.settings_page(),
        };

        widget::container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(20)
            .into()
    }

    /// Register subscriptions for this application.
    fn subscription(&self) -> Subscription<Self::Message> {
        Subscription::batch(vec![
            // Watch for application configuration changes.
            self.core()
                .watch_config::<Config>(Self::APP_ID)
                .map(|update| Message::UpdateConfig(update.config)),
        ])
    }

    /// Handles messages emitted by the application and its widgets.
    fn update(&mut self, message: Self::Message) -> Task<cosmic::Action<Self::Message>> {
        match message {
            Message::OpenRepositoryUrl => {
                _ = open::that_detached(REPOSITORY);
            }

            Message::ToggleContextPage(context_page) => {
                if self.context_page == context_page {
                    self.core.window.show_context = !self.core.window.show_context;
                } else {
                    self.context_page = context_page;
                    self.core.window.show_context = true;
                }
            }

            Message::UpdateConfig(config) => {
                self.config = config;
            }

            Message::LaunchUrl(url) => match open::that_detached(&url) {
                Ok(()) => {}
                Err(err) => {
                    eprintln!("failed to open {url:?}: {err}");
                }
            },

            // Simple UI actions
            Message::PauseSync => {
                self.sync_status = "Paused".to_string();
                self.error_message = None;
            }

            Message::ResumeSync => {
                self.sync_status = "Running".to_string();
                self.error_message = None;
            }

            Message::RefreshStatus => {
                self.sync_status = "Running".to_string();
                self.sync_progress = (5, 10);
                self.download_queue_size = 3;
                self.last_sync_time = "2024-01-15 14:30:00".to_string();
                self.error_message = None;
            }

            Message::AddSyncFolder => {
                self.sync_folders.push("/home/user/NewFolder".to_string());
            }

            Message::RemoveSyncFolder(folder) => {
                self.sync_folders.retain(|f| f != &folder);
            }

            Message::DisplayNotification => {
                if let Some(sender) = &self.notification_sender {
                    let sender_clone = sender.clone();
                    return Task::perform(
                        async move {
                            match sender_clone.send_simple_notification("summary", "body", NotificationUrgency::Low).await {
                                Ok(()) => Message::RefreshStatus, // Return a message to indicate completion
                                Err(e) => {
                                    eprintln!("Failed to send notification: {}", e);
                                    Message::RefreshStatus
                                }
                            }
                        },
                        |result| cosmic::Action::App(result),
                    );
                }
            }

            Message::NotificationSenderInitialized(result) => {
                match result {
                    Ok(sender) => self.notification_sender = Some(sender),
                    Err(e) => eprintln!("Failed to initialize notification sender: {}", e),
                }
            }
        }
        Task::none()
    }

    /// Called when a nav item is selected.
    fn on_nav_select(&mut self, id: nav_bar::Id) -> Task<cosmic::Action<Self::Message>> {
        self.nav.activate(id);
        self.update_title()
    }
}

impl AppModel {
    /// Status page showing sync information
    fn status_page(&self) -> Element<Message> {
        let cosmic_theme::Spacing { space_s, space_m, .. } = theme::active().cosmic().spacing;
        
        let status_text = widget::text::title2(&self.sync_status);
        
        let progress_text = if self.sync_progress.1 > 0 {
            format!("Progress: {}/{} files", self.sync_progress.0, self.sync_progress.1)
        } else {
            "No sync in progress".to_string()
        };
        
        let queue_text = format!("Download queue: {} files", self.download_queue_size);
        let last_sync_text = format!("Last sync: {}", self.last_sync_time);

        let control_buttons = widget::row()
            .push(button::suggested("Pause")
                    .on_press(Message::PauseSync)
            )
            .push(
                widget::button::suggested("Resume")
                    .on_press(Message::ResumeSync)
            )
            .push(
                widget::button::suggested("Refresh")
                    .on_press(Message::RefreshStatus)
            ).push(button::suggested("Display Notification")
                    .on_press(Message::DisplayNotification)
            )
            .spacing(space_s);

        let error_widget = if let Some(error) = &self.error_message {
            widget::text::body(format!("Error: {}", error))
        } else {
            widget::text::body("")
        };
        
        
        let column =widget::column()
            .push(status_text)
            // .push(widget::text::body(progress_text))
            // .push(widget::text::body(queue_text))
            // .push(widget::text::body(last_sync_text))
            .push(control_buttons)
            .push(error_widget)
            .spacing(space_m)
            .align_x(Alignment::Center);
        let container = widget::container(column)
            .padding(20)
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(800)
            .center_y(800);

        let row: Row<Message>= row::with_children(vec![
            
            
            widget::text::body("test").into(),
            widget::divider::vertical::default().into(),
            widget::text::body("Donloading 🟡").into(),
            widget::divider::vertical::default().into(),
            widget::text::body("Uploading 🟢").into(),
            widget::divider::vertical::default().into(),
            widget::text::body("Syncing 🟢").into(),
        ]).spacing(5).align_y(Alignment::Center).width(Length::Fill).height(Length::Fixed(30.0));

        widget::column::with_children(vec![row.into(),
            container.into(),
        ]).into()
    }

    /// Folders page showing sync folders
    fn folders_page(&self) -> Element<Message> {
        let cosmic_theme::Spacing { space_s, space_m, .. } = theme::active().cosmic().spacing;

        let title = widget::text::title2("Sync Folders");
        
        let folders_list :Element<Message>= if self.sync_folders.is_empty() {
            widget::text::body("No sync folders configured").into()  // Convert to Element
        } else {
            let mut column = widget::column().spacing(space_s);
            
            for folder in &self.sync_folders {
                let row = widget::row()
                    .push(widget::text::body(folder))
                    .push(
                        widget::button::destructive("Remove")
                            .on_press(Message::RemoveSyncFolder(folder.clone()))
                    )
                    .spacing(space_s);
                column = column.push(row);
            }
            
            column.into()  // Both branches now return Element
        };

        let add_button = widget::button::suggested("Add Folder")
            .on_press(Message::AddSyncFolder);

        widget::column()
            .push(title)
            .push(folders_list)
            .push(add_button)
            .spacing(space_m)
            .align_x(Alignment::Center)
            .into()
    }

    /// Settings page
    fn settings_page(&self) -> Element<Message> {
        let cosmic_theme::Spacing { space_m, .. } = theme::active().cosmic().spacing;

        let title = widget::text::title2("Settings");
        let mount_point_text = format!("Mount point: {}", self.mount_point);

        widget::column()
            .push(title)
            .push(widget::text::body(mount_point_text))
            .spacing(space_m)
            .align_x(Alignment::Center)
            .into()
    }

    /// The about page for this app.
    pub fn about(&self) -> Element<Message> {
        let cosmic_theme::Spacing { space_xxs, .. } = theme::active().cosmic().spacing;

        let icon = widget::svg(widget::svg::Handle::from_memory(APP_ICON));
        let title = widget::text::title3(fl!("app-title"));

        let hash = env!("VERGEN_GIT_SHA");
        let short_hash: String = hash.chars().take(7).collect();
        let date = env!("VERGEN_GIT_COMMIT_DATE");

        let link = widget::button::link(REPOSITORY)
            .on_press(Message::OpenRepositoryUrl)
            .padding(0);

        widget::column()
            .push(icon)
            .push(title)
            .push(link)
            .push(
                widget::button::link(fl!(
                    "git-description",
                    hash = short_hash.as_str(),
                    date = date
                ))
                .on_press(Message::LaunchUrl(format!("{REPOSITORY}/commits/{hash}")))
                .padding(0),
            )
            .align_x(Alignment::Center)
            .spacing(space_xxs)
            .into()
    }

    /// Updates the header and window titles.
    pub fn update_title(&mut self) -> Task<cosmic::Action<Message>> {
        let mut window_title = fl!("app-title");

        if let Some(page) = self.nav.text(self.nav.active()) {
            window_title.push_str(" — ");
            window_title.push_str(page);
        }

        if let Some(id) = self.core.main_window_id() {
            self.set_window_title(window_title, id)
        } else {
            Task::none()
        }
    }
}

/// The page to display in the application.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Status,
    Folders,
    Settings,
}

/// The context page to display in the context drawer.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub enum ContextPage {
    #[default]
    About,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MenuAction {
    About,
}

impl menu::action::MenuAction for MenuAction {
    type Message = Message;

    fn message(&self) -> Self::Message {
        match self {
            MenuAction::About => Message::ToggleContextPage(ContextPage::About),
        }
    }
}
