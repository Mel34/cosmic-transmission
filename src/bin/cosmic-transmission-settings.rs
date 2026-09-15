fn main() -> cosmic::iced::Result {
    cosmic::app::run::<cosmic_transmission::settings::SettingsModel>(
        cosmic::app::Settings::default().size(cosmic::iced::Size::new(480.0, 420.0)),
        (),
    )
}
