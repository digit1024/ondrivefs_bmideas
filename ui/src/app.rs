// SPDX-License-Identifier: MPL-2.0

use crate::config::Config;
use crate::fl;
use cosmic::cosmic_config::{self, CosmicConfigEntry};

use cosmic::iced::{Length, Subscription};

use cosmic::prelude::*;
use cosmic::widget::{self, icon, menu, nav_bar};

use crate::pages::{self, about_element, conflicts_page, folders_page, queues_page, status_page, gallery_page};
use log::info;
use std::collections::HashMap;

#[derive(Copy, Clone, Debug, Eq, PartialEq, Default)]
enum PageId {
    #[default]
    Gallery,
    Status,
    Folders,
    Queues,
    Conflicts,
    Logs,

}
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub enum ContextPage {
    #[default]
    About,
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub enum MenuAction {
    #[default]
    About,
}

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
    #[allow(dead_code)]
    config: Config,

    #[allow(dead_code)]
    active_page: PageId,
    status_page: pages::status_page::Page,
    folders_page: pages::folders_page::Page,
    queues_page: pages::queues_page::Page,
    conflicts_page: pages::conflicts_page::ConflictsPage,
    logs_page: pages::logs_page::Page,
    gallery_page: pages::gallery_page::Page,
}

/// Messages emitted by the application and its widgets.
#[derive(Debug, Clone)]
pub enum Message {
    // Navigation and UI
    //OpenRepositoryUrl,
    ToggleContextPage(ContextPage),
    StatusPage(status_page::Message),
    FoldersPage(folders_page::Message),
    QueuesPage(queues_page::Message),
    ConflictsPage(conflicts_page::Message),
    AboutElement(about_element::Message),
    LogsPage(pages::logs_page::Message),
    GalleryPage(gallery_page::Message),
}
// impl From<status_page::Message> for Message {
//     fn from(message: status_page::Message) -> Self {
//         Self::StatusPage(message)
//     }
// }
impl From<status_page::Message> for Message {
    fn from(message: status_page::Message) -> Self {
        Self::StatusPage(message)
    }
}

impl From<folders_page::Message> for Message {
    fn from(message: folders_page::Message) -> Self {
        Self::FoldersPage(message)
    }
}

impl From<queues_page::Message> for Message {
    fn from(message: queues_page::Message) -> Self {
        Self::QueuesPage(message)
    }
}

impl From<about_element::Message> for Message {
    fn from(message: about_element::Message) -> Self {
        Self::AboutElement(message)
    }
}

impl From<pages::logs_page::Message> for Message {
    fn from(message: pages::logs_page::Message) -> Self {
        Self::LogsPage(message)
    }
}
impl From<gallery_page::Message> for Message {
    fn from(message: gallery_page::Message) -> Self { Self::GalleryPage(message) }
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
            .text("Gallery")
            .data::<PageId>(PageId::Gallery)
            .icon(icon::from_name("image-x-generic-symbolic"))
            .activate();

        nav.insert()
            .text("Status")
            .data::<PageId>(PageId::Status)
            .icon(icon::from_name("applications-system-symbolic"));

        nav.insert()
            .text("Folders")
            .data::<PageId>(PageId::Folders)
            .icon(icon::from_name("folder-symbolic"));

        nav.insert()
            .text("Queues")
            .data::<PageId>(PageId::Queues)
            .icon(icon::from_name("view-refresh-symbolic"));

        nav.insert()
            .text("Conflicts")
            .data::<PageId>(PageId::Conflicts)
            .icon(icon::from_name("dialog-warning-symbolic"));

        nav.insert()
            .text("Logs")
            .data::<PageId>(PageId::Logs)
            .icon(icon::from_name("text-x-generic-symbolic"));





        // Construct the app model with the runtime's core.
        let mut app = AppModel {
            core,
            context_page: ContextPage::default(),
            nav,
            key_binds: HashMap::new(),
            config: cosmic_config::Config::new(Self::APP_ID, Config::VERSION)
                .map(|context| match Config::get_entry(&context) {
                    Ok(config) => config,
                    Err((_errors, config)) => config,
                })
                .unwrap_or_default(),
            active_page: PageId::Gallery,
            status_page: pages::status_page::Page::new(),
            folders_page: pages::folders_page::Page::new(),
            queues_page: pages::queues_page::Page::new(),
            conflicts_page: pages::conflicts_page::ConflictsPage::new(),
            logs_page: pages::logs_page::Page::new(),
            gallery_page: pages::gallery_page::Page::new(),
        };

        // Create startup commands: set window title and fetch initial data for pages
        let title_command = app.update_title();
        let fetch_status_command = cosmic::task::future(async move {
            info!("App: Initializing StatusPage with fetch command");
            Message::StatusPage(status_page::Message::FetchStatus)
        });
        let fetch_queues_command =
            cosmic::task::future(
                async move { Message::QueuesPage(queues_page::Message::FetchQueues) },
            );
        let fetch_folders_command = cosmic::task::future(async move {
            Message::FoldersPage(folders_page::Message::FetchFolders)
        });
        let fetch_gallery_command = cosmic::task::future(async move {
            Message::GalleryPage(gallery_page::Message::FetchPage)
        });

        (
            app,
            Task::batch(vec![
                title_command,
                fetch_status_command,
                fetch_queues_command,
                fetch_folders_command,
                fetch_gallery_command,
            ]),
        )
    }

    fn subscription(&self) -> Subscription<Message> {
        match self.nav.active_data::<PageId>() {
            Some(PageId::Status) => self.status_page.subscription().map(Message::StatusPage),
            Some(PageId::Queues) => self.queues_page.subscription().map(Message::QueuesPage),
            Some(PageId::Gallery) => self.gallery_page.subscription().map(Message::GalleryPage),
            _ => Subscription::none(),
        }
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
    // fn context_drawer(&self) -> Option<context_drawer::ContextDrawer<Self::Message>> {
    //     if !self.core.window.show_context {
    //         return None;
    //     }

    //     // Some(match self.context_page {
    //     //     ContextPage::About => context_drawer::context_drawer(
    //     //         about_element::about(),
    //     //         Message::ToggleContextPage(ContextPage::About),
    //     //     )
    //     //     .title(fl!("about")),
    //     // })
    // }

    /// Describes the interface based on the current state of the application model.
    fn view(&self) -> Element<Self::Message> {
        let page = self.nav.active_data::<PageId>().unwrap_or(&PageId::Gallery);

        let content = match page {
            PageId::Status => self.status_page.view().map(Message::StatusPage),
            PageId::Folders => self.folders_page.view().map(Message::FoldersPage),
            PageId::Queues => self.queues_page.view().map(Message::QueuesPage),
            PageId::Conflicts => self.conflicts_page.view().map(Message::ConflictsPage),
            PageId::Logs => self.logs_page.view().map(Message::LogsPage),
            PageId::Gallery => self.gallery_page.view().map(Message::GalleryPage),
        };

        widget::container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(20)
            .into()
    }

    /// Handles messages emitted by the application and its widgets.
    fn update(&mut self, message: Self::Message) -> cosmic::Task<cosmic::Action<Message>> {
        match message {
            // Message::OpenRepositoryUrl => {
            //     _ = open::that_detached(REPOSITORY);
            // }
            Message::ToggleContextPage(context_page) => {
                if self.context_page == context_page {
                    self.core.window.show_context = !self.core.window.show_context;
                } else {
                    self.context_page = context_page;
                    self.core.window.show_context = true;
                }
                Task::none()
            }

            Message::StatusPage(status_message) => {
                info!("App: Processing StatusPage message: {:?}", status_message);
                self.status_page.update(status_message)
            }
            Message::FoldersPage(folders_message) => self.folders_page.update(folders_message),
            Message::QueuesPage(queues_message) => self.queues_page.update(queues_message),
            Message::ConflictsPage(conflicts_message) => self.conflicts_page.update(conflicts_message),
            Message::AboutElement(about_element::Message::OpenRepositoryUrl) => {
                _ = open::that_detached("REPOITORY");
                Task::none()
            }
            Message::AboutElement(about_element::Message::LaunchUrl(url)) => {
                _ = open::that_detached(url);
                Task::none()
            }
            Message::LogsPage(logs_message) => self.logs_page.update(logs_message),
            Message::GalleryPage(gallery_message) => self.gallery_page.update(gallery_message),
        }
    }

    /// Called when a nav item is selected.
    fn on_nav_select(&mut self, id: nav_bar::Id) -> Task<cosmic::Action<Self::Message>> {
        self.nav.activate(id);
        let mut tasks = vec![self.update_title()];
        if self.nav.active_data::<PageId>() == Some(&PageId::Folders) {
            tasks.push(cosmic::task::future(async move {
                Message::FoldersPage(folders_page::Message::FetchFolders)
            }));
        } else if self.nav.active_data::<PageId>() == Some(&PageId::Conflicts) {
            tasks.push(cosmic::task::future(async move {
                Message::ConflictsPage(conflicts_page::Message::Reload)
            }));
        } else if self.nav.active_data::<PageId>() == Some(&PageId::Gallery) {
            tasks.push(cosmic::task::future(async move { Message::GalleryPage(gallery_page::Message::FetchPage) }));
        }
        Task::batch(tasks)
    }
}

impl AppModel {
    /// Status page showing sync information

    /// The about page for this app.

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

impl menu::action::MenuAction for MenuAction {
    type Message = Message;

    fn message(&self) -> Self::Message {
        match self {
            MenuAction::About => Message::ToggleContextPage(ContextPage::About),
        }
    }
}
