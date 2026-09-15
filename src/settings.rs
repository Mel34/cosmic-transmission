use cosmic::{
    app::Application,
    iced::{
        window::Id,
        Subscription,
    },
    prelude::*,
    theme,
    widget,
};

use crate::config::{ConnectionConfig, ServiceScope};
use cosmic_config::CosmicConfigEntry;

#[derive(Debug, Clone)]
pub enum Message {
    HostChanged(String),
    RpcPortChanged(String),
    ServiceScopeChanged(ServiceScope),
    Close,
}

pub struct SettingsModel {
    core: cosmic::Core,
    config: ConnectionConfig,
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
        .map(|config| {
            ConnectionConfig::get_entry(&config)
                .unwrap_or_else(|(_, config)| config)
        })
        .unwrap_or_default();

        (
            Self { core, config },
            cosmic::Task::none(),
        )
    }

    fn on_close_requested(&self, _id: Id) -> Option<Self::Message> {
        Some(Message::Close)
    }

    fn update(
         &mut self,
         message: Self::Message,
     ) -> cosmic::Task<cosmic::Action<Self::Message>> {
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

            Message::Close => {
                self.save_config();
                return cosmic::iced::window::close(
                    self.core.main_window_id().unwrap(),
                );
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
        ]);

        widget::container(
            widget::scrollable(settings)
                .width(cosmic::iced::Length::Fill)
                .height(cosmic::iced::Length::Fill),
        )
        .class(theme::Container::WindowBackground)
        .width(cosmic::iced::Length::Fill)
        .height(cosmic::iced::Length::Fill)
        .padding(theme::active().cosmic().spacing.space_l)
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
    }
}
