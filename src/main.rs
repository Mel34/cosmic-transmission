mod app;
mod config;
mod service;

fn main() -> cosmic::iced::Result {
    cosmic::applet::run::<app::AppModel>(())
}
