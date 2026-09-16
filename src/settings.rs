use cosmic::{
    app::Application,
    iced::{Subscription, window::Id},
    prelude::*,
    theme, widget,
};

use crate::config::{AppConfig, ConnectionConfig, PollInterval, ServiceScope};
use cosmic_config::CosmicConfigEntry;

#[derive(Debug, Clone)]
pub enum Message {
    HostChanged(String),
    RpcPortChanged(String),
    ServiceScopeChanged(ServiceScope),
    PollIntervalChanged(PollInterval),
    Close,
}
pub struct SettingsModel {
    core: cosmic::Core,
    config: ConnectionConfig,
    app_config: AppConfig,
}

impl Application for SettingsModel {
    type Executor = cosmic::executor::Default;
    type Flags = ();
    type Message = Message;

    const APP_ID: &'static str = "io.github.cosmic.Transmission.Settings";

    fn core(&self) -> &cosmic::Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut cosmic::Core {
        &mut self.core
    }

    fn init(
        mut core: cosmic::Core,
        _flags: Self::Flags,
    ) -> (Self, cosmic::Task<cosmic::Action<Self::Message>>) {
        core.set_header_title("Transmission daemon settings".to_string());

        let config = cosmic::cosmic_config::Config::new(
            "io.github.cosmic.Transmission",
            ConnectionConfig::VERSION,
        )
        .ok()
        .map(|config| ConnectionConfig::get_entry(&config).unwrap_or_else(|(_, config)| config))
        .unwrap_or_default();

        let app_config =
            cosmic::cosmic_config::Config::new("io.github.cosmic.Transmission", AppConfig::VERSION)
                .ok()
                .map(|config| AppConfig::get_entry(&config).unwrap_or_else(|(_, config)| config))
                .unwrap_or_default();

        (
            Self {
                core,
                config,
                app_config,
            },
            cosmic::Task::none(),
        )
    }
    fn on_close_requested(&self, _id: Id) -> Option<Self::Message> {
        Some(Message::Close)
    }

    fn update(&mut self, message: Self::Message) -> cosmic::Task<cosmic::Action<Self::Message>> {
        match message {
            Message::HostChanged(host) => {
                self.config.host = host;
            }

            Message::RpcPortChanged(port) => {
                if let Ok(port) = port.parse::<u16>() {
                    self.config.rpc_port = port;
                }
            }

            Message::ServiceScopeChanged(scope) => {
                self.config.service_scope = scope;
            }

            Message::PollIntervalChanged(interval) => {
                self.app_config.poll_interval = interval;
            }

            Message::Close => {
                self.save_config();
                return cosmic::iced::window::close(self.core.main_window_id().unwrap());
            }
        }

        cosmic::Task::none()
    }

    fn view(&self) -> Element<'_, Self::Message> {
        self.settings_view()
    }

    fn subscription(&self) -> Subscription<Self::Message> {
        Subscription::none()
    }
}

impl SettingsModel {
    fn settings_view(&self) -> Element<'_, Message> {
        static AUTOSIZE_ID: std::sync::LazyLock<cosmic::widget::Id> =
            std::sync::LazyLock::new(|| {
                cosmic::widget::Id::new("io.github.cosmic.Transmission.Settings.autosize")
            });

        let host = widget::settings::item(
            "Host",
            widget::text_input("localhost", &self.config.host)
                .on_input(Message::HostChanged)
                .width(cosmic::iced::Length::Fixed(220.0)),
        );

        let rpc_port = widget::settings::item(
            "RPC port",
            widget::text_input("9091", self.config.rpc_port.to_string())
                .on_input(Message::RpcPortChanged)
                .width(cosmic::iced::Length::Fixed(120.0)),
        );

        let service_scope = widget::settings::item(
            "Scope",
            cosmic::iced::widget::pick_list(
                [ServiceScope::User, ServiceScope::System],
                Some(self.config.service_scope),
                Message::ServiceScopeChanged,
            )
            .width(cosmic::iced::Length::Fixed(120.0)),
        );

        let poll_interval = widget::settings::item(
            "Polling interval",
            cosmic::iced::widget::pick_list(
                [
                    PollInterval::OneSecond,
                    PollInterval::TwoSeconds,
                    PollInterval::FiveSeconds,
                    PollInterval::TenSeconds,
                    PollInterval::ThirtySeconds,
                ],
                Some(self.app_config.poll_interval),
                Message::PollIntervalChanged,
            )
            .width(cosmic::iced::Length::Fixed(140.0)),
        );

        let settings = widget::settings::view_column(vec![
            widget::settings::section()
                .title("Connection")
                .add(host)
                .add(rpc_port)
                .into(),
            widget::settings::section()
                .title("Service")
                .add(service_scope)
                .into(),
            widget::settings::section()
                .title("Updates")
                .add(poll_interval)
                .into(),
        ]);

        let spacing = theme::active().cosmic().spacing;

        let content = widget::container(settings)
            .class(theme::Container::WindowBackground)
            .padding([
                spacing.space_s,
                spacing.space_s,
                spacing.space_xxl,
                spacing.space_s,
            ])
            .height(cosmic::iced::Length::Shrink);

        cosmic::widget::autosize::autosize(content, AUTOSIZE_ID.clone())
            .limits(
                cosmic::iced::Limits::NONE
                    .min_height(1.0)
                    .min_width(500.0)
                    .max_width(500.0),
            )
            .into()
    }
    fn save_config(&self) {
        let Ok(config) = cosmic::cosmic_config::Config::new(
            "io.github.cosmic.Transmission",
            ConnectionConfig::VERSION,
        ) else {
            return;
        };

        let _ = self.config.write_entry(&config);

        let Ok(app_config) =
            cosmic::cosmic_config::Config::new("io.github.cosmic.Transmission", AppConfig::VERSION)
        else {
            return;
        };

        let _ = self.app_config.write_entry(&app_config);
    }
}
