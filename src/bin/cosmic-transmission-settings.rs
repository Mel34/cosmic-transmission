fn main() -> cosmic::iced::Result {
    cosmic::app::run::<cosmic_transmission::settings::SettingsModel>(
        cosmic::app::Settings::default().size(cosmic::iced::Size::new(900.0, 600.0)),
        (),
    )
}
