// SPDX-License-Identifier: MPL-2.0


mod config;
mod i18n;
mod dbus_client;


use crate::window::Window;

mod window;

fn main() -> cosmic::iced::Result {
    let env = env_logger::Env::default()
        .filter_or("MY_LOG_LEVEL", "warn")
        .write_style_or("MY_LOG_STYLE", "always");

    env_logger::init_from_env(env);
    cosmic::applet::run::<Window>(())
}