mod app;
mod config;

fn main() -> cosmic::iced::Result {
    cosmic::applet::run::<app::AppModel>(())
}
