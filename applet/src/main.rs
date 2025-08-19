// SPDX-License-Identifier: MPL-2.0

mod applet;
mod dbus_client;

fn main() -> cosmic::iced::Result {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    tracing::info!("Starting OneDrive sync applet");

    applet::run()
}
