use std::time::Duration;

use cosmic::app::{Core, Task};
use cosmic::iced::{Length, Subscription, time};
use cosmic::widget::container;
use onedrive_sync_lib::dbus::types::DaemonStatus;

use crate::dbus_client::with_dbus_client;
use crate::dbus_client::DbusClient;

const APP_ID: &str = "com.github.com.bmideas.onedrive-sync.applet";

pub fn run() -> cosmic::iced::Result {
    cosmic::applet::run::<OneDriveApplet>(())
}

struct OneDriveApplet {
    core: Core,
    daemon_status: Option<DaemonStatus>,
    subscribed: bool,
}

impl Default for OneDriveApplet {
    fn default() -> Self {
        Self {
            core: Core::default(),
            daemon_status: None,
            subscribed: false,
        }
    }
}

#[derive(Clone, Debug)]
pub enum Message {
    FetchStatus,
    StatusLoaded(Result<DaemonStatus, String>),
    IconClicked,
}

impl OneDriveApplet {
    fn get_icon_name(&self) -> &'static str {
        match &self.daemon_status {
            Some(status) => {
                if status.is_authenticated && status.is_connected && !status.has_conflicts && status.is_mounted {
                    "open-onedrive-ok"
                } else if !status.is_connected {
                    "open-onedrive-offline"
                } else if status.has_conflicts {
                    "open-onedrive-conflict"
                } else {
                    "open-onedrive-error"
                }
            }
            None => "open-onedrive-offline"
        }
    }


}

impl cosmic::Application for OneDriveApplet {
    type Executor = cosmic::SingleThreadExecutor;
    type Flags = ();
    type Message = Message;
    const APP_ID: &'static str = APP_ID;

    fn core(&self) -> &Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut Core {
        &mut self.core
    }

    fn init(core: Core, _flags: Self::Flags) -> (Self, Task<Message>) {
        let applet = OneDriveApplet {
            core,
            ..Default::default()
        };
        (applet, Task::none())
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::FetchStatus => {
                // One-time subscribe to status signal
                if !self.subscribed {
                    self.subscribed = true;
                    let subscribe = async move {
                        if let Ok(client) = DbusClient::new().await {
                            let _ = client
                                .subscribe_daemon_status(|_status| async move {
                                    // Keep minimal subscription for status updates
                                })
                                .await;
                        }
                    };
                    let _ = Task::perform(subscribe, |_| cosmic::Action::<Message>::None);
                }
                
                let fetch_status = with_dbus_client(|client| async move { 
                    client.get_daemon_status().await 
                });
                
                Task::perform(fetch_status, |result| match result {
                    Ok(status) => cosmic::Action::App(Message::StatusLoaded(Ok(status))),
                    Err(e) => cosmic::Action::App(Message::StatusLoaded(Err(e.to_string()))),
                })
            }

            Message::StatusLoaded(result) => {
                match result {
                    Ok(status) => {
                        self.daemon_status = Some(status);
                    }
                    Err(_e) => {
                        self.daemon_status = None;
                    }
                }
                Task::none()
            }
            
            Message::IconClicked => {
                // Launch main UI application on click
                let _ = std::process::Command::new("onedrive-sync-ui").spawn();
                Task::none()
            }
        }
    }

    fn subscription(&self) -> Subscription<Message> {
        // Poll status every 15 seconds as fallback
        time::every(Duration::from_secs(15)).map(|_| Message::FetchStatus)
    }

    fn view(&self) -> cosmic::Element<Message> {
        let icon_name = self.get_icon_name();
        
        // Create a clickable button with the icon
        let icon = cosmic::widget::icon::from_name(icon_name)
            .symbolic(false)
            .size(32); // Set icon size to fill applet area
        
        let button = cosmic::widget::button::custom(cosmic::Element::from(icon))
            .class(cosmic::theme::Button::AppletIcon)
            .on_press(Message::IconClicked);

        // Wrap button in container that fills the entire applet area
        container(button)
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .into()
    }

    fn view_window(&self, _id: cosmic::iced::window::Id) -> cosmic::Element<Message> {
        "No window view needed".into()
    }
}
