use cosmic::{
    app::Application,
    iced::{Length, Subscription, window::Id},
    prelude::*,
    theme, widget,
};

use crate::config::{
    AppConfig, Connection, ConnectionsConfig, LOCAL_SYSTEM_ID, LOCAL_USER_ID, PollInterval,
    ServiceScope,
};
use cosmic_config::CosmicConfigEntry;

#[derive(Debug, Clone)]
pub enum Message {
    SelectConnection(uuid::Uuid),
    AddConnection,
    DeleteConnection,
    MoveConnectionUp,
    MoveConnectionDown,
    SetActiveConnection,
    NameChanged(String),
    HostChanged(String),
    RpcPortChanged(String),
    ServiceScopeChanged(ServiceScope),
    PollIntervalChanged(PollInterval),
    Close,
}

pub struct SettingsModel {
    core: cosmic::Core,
    connections_config: ConnectionsConfig,
    selected_connection: uuid::Uuid,
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

        let connections_config = cosmic::cosmic_config::Config::new(
            "io.github.cosmic.Transmission",
            ConnectionsConfig::VERSION,
        )
        .ok()
        .map(|config| ConnectionsConfig::get_entry(&config).unwrap_or_else(|(_, config)| config))
        .unwrap_or_default();

        let app_config =
            cosmic::cosmic_config::Config::new("io.github.cosmic.Transmission", AppConfig::VERSION)
                .ok()
                .map(|config| AppConfig::get_entry(&config).unwrap_or_else(|(_, config)| config))
                .unwrap_or_default();

        (
            Self {
                core,
                selected_connection: connections_config.active_connection,
                connections_config,
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
            Message::SelectConnection(id) => {
                if self
                    .connections_config
                    .connections
                    .iter()
                    .any(|connection| connection.id == id)
                {
                    self.selected_connection = id;
                }
            }

            Message::AddConnection => {
                let id = uuid::Uuid::new_v4();

                self.connections_config.connections.push(Connection {
                    id,
                    name: "New Connection".to_string(),
                    host: "localhost".to_string(),
                    rpc_port: 9091,
                    service_scope: None,
                });

                self.selected_connection = id;
            }

            Message::DeleteConnection => {
                if self.selected_connection == LOCAL_USER_ID
                    || self.selected_connection == LOCAL_SYSTEM_ID
                {
                    return cosmic::Task::none();
                }

                if let Some(index) = self
                    .connections_config
                    .connections
                    .iter()
                    .position(|connection| connection.id == self.selected_connection)
                {
                    self.connections_config.connections.remove(index);

                    if self.connections_config.active_connection == self.selected_connection {
                        self.connections_config.active_connection = LOCAL_USER_ID;
                    }

                    self.selected_connection = self.connections_config.active_connection;
                }
            }

            Message::MoveConnectionUp => {
                if let Some(index) = self
                    .connections_config
                    .connections
                    .iter()
                    .position(|connection| connection.id == self.selected_connection)
                    && index > 2
                {
                    self.connections_config.connections.swap(index, index - 1);
                }
            }

            Message::MoveConnectionDown => {
                if let Some(index) = self
                    .connections_config
                    .connections
                    .iter()
                    .position(|connection| connection.id == self.selected_connection)
                    && index >= 2
                    && index + 1 < self.connections_config.connections.len()
                {
                    self.connections_config.connections.swap(index, index + 1);
                }
            }

            Message::SetActiveConnection => {
                if self
                    .connections_config
                    .connections
                    .iter()
                    .any(|connection| connection.id == self.selected_connection)
                {
                    self.connections_config.active_connection = self.selected_connection;
                }
            }

            Message::NameChanged(name) => {
                if let Some(connection) = self.selected_connection_mut()
                    && connection.service_scope.is_none()
                {
                    connection.name = name;
                }
            }

            Message::HostChanged(host) => {
                if let Some(connection) = self.selected_connection_mut() {
                    connection.host = host;
                }
            }

            Message::RpcPortChanged(port) => {
                if let Ok(port) = port.parse::<u16>()
                    && let Some(connection) = self.selected_connection_mut()
                {
                    connection.rpc_port = port;
                }
            }

            Message::ServiceScopeChanged(scope) => {
                if let Some(connection) = self.selected_connection_mut() {
                    if connection.id == LOCAL_USER_ID {
                        connection.service_scope = Some(ServiceScope::User);
                    } else if connection.id == LOCAL_SYSTEM_ID {
                        connection.service_scope = Some(ServiceScope::System);
                    } else {
                        connection.service_scope = Some(scope);
                    }
                }
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
    fn selected_connection(&self) -> &Connection {
        self.connections_config
            .connections
            .iter()
            .find(|connection| connection.id == self.selected_connection)
            .expect("Selected connection must exist")
    }

    fn selected_connection_mut(&mut self) -> Option<&mut Connection> {
        self.connections_config
            .connections
            .iter_mut()
            .find(|connection| connection.id == self.selected_connection)
    }

    fn settings_view(&self) -> Element<'_, Message> {
        static AUTOSIZE_ID: std::sync::LazyLock<cosmic::widget::Id> =
            std::sync::LazyLock::new(|| {
                cosmic::widget::Id::new("io.github.cosmic.Transmission.Settings.autosize")
            });

        let spacing = theme::active().cosmic().spacing;
        let connection = self.selected_connection();

        let connection_buttons = self
            .connections_config
            .connections
            .iter()
            .map(|connection| {
                let selected = connection.id == self.selected_connection;
                let active = connection.id == self.connections_config.active_connection;

                let label = if active {
                    format!("{}  •", connection.name)
                } else {
                    connection.name.clone()
                };

                widget::button::custom(widget::text(label))
                    .width(Length::Fill)
                    .on_press(Message::SelectConnection(connection.id))
                    .class(if selected {
                        theme::Button::Link
                    } else {
                        theme::Button::MenuRoot
                    })
                    .into()
            })
            .collect::<Vec<Element<'_, Message>>>();

        let connection_list = widget::column::with_children(connection_buttons)
            .spacing(spacing.space_xxs)
            .width(Length::Fixed(220.0));

        let selected_is_local = connection.service_scope.is_some();

        let name = if selected_is_local {
            widget::settings::item(
                "Name",
                widget::text(connection.name.clone()).width(Length::Fixed(220.0)),
            )
        } else {
            widget::settings::item(
                "Name",
                widget::text_input("", &connection.name)
                    .on_input(Message::NameChanged)
                    .width(Length::Fixed(220.0)),
            )
        };

        let host = widget::settings::item(
            "Host",
            widget::text_input("localhost", &connection.host)
                .on_input(Message::HostChanged)
                .width(Length::Fixed(220.0)),
        );

        let rpc_port = widget::settings::item(
            "RPC port",
            widget::text_input("9091", connection.rpc_port.to_string())
                .on_input(Message::RpcPortChanged)
                .width(Length::Fixed(120.0)),
        );

        let details = if selected_is_local {
            let scope = widget::text(
                connection
                    .service_scope
                    .expect("Local connection must have a service scope")
                    .to_string(),
            );

            let active_button = if connection.id == self.connections_config.active_connection {
                widget::button::standard("Active")
            } else {
                widget::button::suggested("Use as active").on_press(Message::SetActiveConnection)
            };

            let move_up = widget::button::standard("Move up").on_press_maybe(
                (connection.id != LOCAL_USER_ID && connection.id != LOCAL_SYSTEM_ID)
                    .then_some(Message::MoveConnectionUp),
            );

            let move_down = widget::button::standard("Move down").on_press_maybe(
                (connection.id != LOCAL_USER_ID && connection.id != LOCAL_SYSTEM_ID)
                    .then_some(Message::MoveConnectionDown),
            );

            widget::column::with_children(vec![
                name.into(),
                host.into(),
                rpc_port.into(),
                widget::settings::item("Scope", scope).into(),
                widget::row::with_children(vec![
                    active_button.into(),
                    move_up.into(),
                    move_down.into(),
                ])
                .spacing(spacing.space_xxs)
                .into(),
            ])
            .spacing(spacing.space_s)
        } else {
            let active_button = if connection.id == self.connections_config.active_connection {
                widget::button::standard("Active")
            } else {
                widget::button::suggested("Use as active").on_press(Message::SetActiveConnection)
            };

            let delete_button =
                widget::button::destructive("Delete").on_press(Message::DeleteConnection);

            let move_up = widget::button::standard("Move up").on_press(Message::MoveConnectionUp);

            let move_down =
                widget::button::standard("Move down").on_press(Message::MoveConnectionDown);

            widget::column::with_children(vec![
                name.into(),
                host.into(),
                rpc_port.into(),
                widget::row::with_children(vec![active_button.into(), delete_button.into()])
                    .spacing(spacing.space_xxs)
                    .into(),
                widget::row::with_children(vec![move_up.into(), move_down.into()])
                    .spacing(spacing.space_xxs)
                    .into(),
            ])
            .spacing(spacing.space_s)
        };

        let connection_section = widget::column::with_children(vec![
            widget::text::heading("Connections").into(),
            widget::row::with_children(vec![
                widget::scrollable(connection_list)
                    .height(Length::Fill)
                    .into(),
                widget::divider::vertical::default().into(),
                details.width(Length::Fill).into(),
            ])
            .spacing(spacing.space_s)
            .height(Length::Fixed(300.0))
            .into(),
            widget::button::suggested("Add Connection")
                .on_press(Message::AddConnection)
                .into(),
        ])
        .spacing(spacing.space_s);

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
            .width(Length::Fixed(140.0)),
        );

        let updates_section = widget::settings::section()
            .title("Updates")
            .add(poll_interval);

        let settings =
            widget::column::with_children(vec![connection_section.into(), updates_section.into()])
                .spacing(spacing.space_l);

        let content = widget::container(settings)
            .class(theme::Container::WindowBackground)
            .padding([
                spacing.space_s,
                spacing.space_s,
                spacing.space_xxl,
                spacing.space_s,
            ])
            .height(Length::Shrink);

        cosmic::widget::autosize::autosize(content, AUTOSIZE_ID.clone())
            .limits(
                cosmic::iced::Limits::NONE
                    .min_height(1.0)
                    .min_width(700.0)
                    .max_width(700.0),
            )
            .into()
    }

    fn save_config(&self) {
        let Ok(config) = cosmic::cosmic_config::Config::new(
            "io.github.cosmic.Transmission",
            ConnectionsConfig::VERSION,
        ) else {
            return;
        };

        let _ = self.connections_config.write_entry(&config);

        let Ok(app_config) =
            cosmic::cosmic_config::Config::new("io.github.cosmic.Transmission", AppConfig::VERSION)
        else {
            return;
        };

        let _ = self.app_config.write_entry(&app_config);
    }
}
