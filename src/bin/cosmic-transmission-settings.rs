fn main() -> cosmic::iced::Result {
    cosmic::app::run::<cosmic_transmission::settings::SettingsModel>(
        cosmic::app::Settings::default(),
        (),
    )
}
