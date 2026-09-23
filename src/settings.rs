use cosmic::{
    app::Application,
    iced::{Length, Subscription, window::Id},
    prelude::*,
    theme, widget,
};

use crate::config::{
    Connection, ConnectionsConfig, LOCAL_SYSTEM_ID, LOCAL_USER_ID, PollInterval, ServiceScope,
};
use crate::credentials;
use crate::service::{
    ServiceAction, ServiceConfiguration, ServiceController, ServiceSetupResult, ServiceState,
};
use cosmic::iced::advanced::Renderer;
use cosmic::iced::core::widget::{Operation, Tree, tree};
use cosmic::iced::core::{Clipboard, Shell, Widget, layout, overlay, renderer};
use cosmic::iced::{Alignment, Point, Rectangle, Size, Vector, event, mouse, touch};
use cosmic_config::CosmicConfigEntry;
use secrecy::{ExposeSecret, SecretString};

const DRAG_START_DISTANCE_SQUARED: f32 = 64.0;

#[derive(Debug, Clone)]
pub enum Message {
    SelectConnection(uuid::Uuid),
    PasswordLoaded(Option<String>),
    ServiceConfigurationLoaded(ServiceConfiguration),
    ServiceStateLoaded(ServiceState),
    ConfigurationStateLoaded(uuid::Uuid, ServiceState),
    ServiceAction(ServiceAction),
    SetupService,
    ServiceActionFinished(ServiceState),
    ServiceSetupFinished(ServiceConfiguration, ServiceState),
    ApplyConfiguration,
    ConfigurationApplied(uuid::Uuid, ServiceScope, Result<(), String>),
    UndoName,
    UndoHost,
    UndoUsername,
    UndoRpcPort,
    TogglePasswordVisibility,
    AddConnection,
    DeleteConnection(uuid::Uuid),
    ReorderConnections(Vec<uuid::Uuid>),
    NameChanged(String),
    HostChanged(String),
    UsernameChanged(String),
    PasswordChanged(String),
    SavePassword,
    ClearPassword,
    RpcPortChanged(String),
    ServiceScopeChanged(ServiceScope),
    PollIntervalChanged(PollInterval),
    Close,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AppliedConfiguration {
    name: String,
    host: String,
    username: String,
    rpc_port: u16,
}

impl AppliedConfiguration {
    fn from_connection(connection: &Connection) -> Self {
        Self {
            name: connection.name.clone(),
            host: connection.host.clone(),
            username: connection.username.clone(),
            rpc_port: connection.rpc_port,
        }
    }

    fn matches(&self, connection: &Connection) -> bool {
        self.name == connection.name
            && self.host == connection.host
            && self.username == connection.username
            && self.rpc_port == connection.rpc_port
    }

    fn rpc_matches(&self, connection: &Connection) -> bool {
        self.username == connection.username && self.rpc_port == connection.rpc_port
    }
}

pub struct SettingsModel {
    core: cosmic::Core,
    connections_config: ConnectionsConfig,
    selected_connection: Connection,
    applied_configuration: AppliedConfiguration,
    password: Option<String>,
    saved_password: Option<String>,
    service_configuration: Option<ServiceConfiguration>,
    service_state: Option<ServiceState>,
    service_action: Option<ServiceAction>,
    configuration_applying: bool,
    password_hidden: bool,
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

        let selected_connection = std::env::args()
            .skip_while(|arg| arg != "--connection")
            .nth(1)
            .and_then(|id| uuid::Uuid::parse_str(&id).ok())
            .and_then(|id| {
                connections_config
                    .connections
                    .iter()
                    .find(|connection| connection.id == id)
                    .cloned()
            })
            .or_else(|| {
                connections_config
                    .connections
                    .iter()
                    .find(|connection| connection.id == connections_config.active_connection)
                    .cloned()
            })
            .expect("Active connection must exist");

        let applied_configuration = AppliedConfiguration::from_connection(&selected_connection);

        let password_task = cosmic::Task::perform(
            credentials::get_password(selected_connection.id),
            |password| {
                cosmic::Action::App(Message::PasswordLoaded(
                    password.map(|password| password.expose_secret().to_string()),
                ))
            },
        );

        let service_configuration_task = if let Some(scope) = selected_connection.service_scope {
            cosmic::Task::perform(
                ServiceController::new(scope).configuration(),
                |configuration| {
                    cosmic::Action::App(Message::ServiceConfigurationLoaded(configuration))
                },
            )
        } else {
            cosmic::Task::perform(
                async { ServiceConfiguration::Unconfigured },
                |configuration| {
                    cosmic::Action::App(Message::ServiceConfigurationLoaded(configuration))
                },
            )
        };

        let service_state_task = if let Some(scope) = selected_connection.service_scope {
            cosmic::Task::perform(ServiceController::new(scope).status(), |state| {
                cosmic::Action::App(Message::ServiceStateLoaded(state))
            })
        } else {
            cosmic::Task::perform(async { ServiceState::Stopped }, |state| {
                cosmic::Action::App(Message::ServiceStateLoaded(state))
            })
        };

        (
            Self {
                core,
                connections_config,
                selected_connection,
                applied_configuration,
                password: None,
                saved_password: None,
                service_configuration: None,
                service_state: None,
                service_action: None,
                configuration_applying: false,
                password_hidden: true,
            },
            cosmic::Task::batch([
                password_task,
                service_configuration_task,
                service_state_task,
            ]),
        )
    }

    fn on_close_requested(&self, _id: Id) -> Option<Self::Message> {
        Some(Message::Close)
    }

    fn update(&mut self, message: Self::Message) -> cosmic::Task<cosmic::Action<Self::Message>> {
        match message {
            Message::SelectConnection(id) => {
                let Some(connection) = self
                    .connections_config
                    .connections
                    .iter()
                    .find(|connection| connection.id == id)
                    .cloned()
                else {
                    return cosmic::Task::none();
                };

                self.selected_connection = connection.clone();
                self.applied_configuration = AppliedConfiguration::from_connection(&connection);
                self.password = None;
                self.saved_password = None;
                self.service_configuration = None;
                self.service_state = None;
                self.service_action = None;
                self.configuration_applying = false;

                let password_task =
                    cosmic::Task::perform(credentials::get_password(connection.id), |password| {
                        cosmic::Action::App(Message::PasswordLoaded(
                            password.map(|password| password.expose_secret().to_string()),
                        ))
                    });

                let service_configuration_task = if let Some(scope) = connection.service_scope {
                    cosmic::Task::perform(
                        ServiceController::new(scope).configuration(),
                        |configuration| {
                            cosmic::Action::App(Message::ServiceConfigurationLoaded(configuration))
                        },
                    )
                } else {
                    cosmic::Task::perform(
                        async { ServiceConfiguration::Unconfigured },
                        |configuration| {
                            cosmic::Action::App(Message::ServiceConfigurationLoaded(configuration))
                        },
                    )
                };

                let service_state_task = if let Some(scope) = connection.service_scope {
                    cosmic::Task::perform(ServiceController::new(scope).status(), |state| {
                        cosmic::Action::App(Message::ServiceStateLoaded(state))
                    })
                } else {
                    cosmic::Task::perform(async { ServiceState::Stopped }, |state| {
                        cosmic::Action::App(Message::ServiceStateLoaded(state))
                    })
                };

                return cosmic::Task::batch([
                    password_task,
                    service_configuration_task,
                    service_state_task,
                ]);
            }

            Message::PasswordLoaded(password) => {
                self.password = password.clone();
                self.saved_password = password;
            }

            Message::ServiceConfigurationLoaded(configuration) => {
                if self.selected_connection.service_scope.is_some() {
                    self.service_configuration = Some(configuration);
                }
            }

            Message::ServiceStateLoaded(state) => {
                if self.selected_connection.service_scope.is_some() {
                    self.service_state = Some(state);
                }
            }

            Message::ConfigurationStateLoaded(id, state) => {
                if self.selected_connection.id == id {
                    self.service_state = Some(state);
                }
            }

            Message::SetupService => {
                let Some(scope) = self.selected_connection.service_scope else {
                    return Task::none();
                };

                return Task::perform(
                    async move {
                        let controller = ServiceController::new(scope);
                        let username = controller.username().await;
                        let result = controller.setup(username).await;
                        let configuration = controller.configuration().await;
                        let state = controller.status().await;

                        (result, configuration, state)
                    },
                    |(result, configuration, state)| {
                        cosmic::Action::App(match result {
                            ServiceSetupResult::Success => {
                                Message::ServiceSetupFinished(configuration, state)
                            }
                            ServiceSetupResult::UnmanagedConfiguration
                            | ServiceSetupResult::Failed => Message::ServiceConfigurationLoaded(
                                ServiceConfiguration::Unconfigured,
                            ),
                        })
                    },
                );
            }

            Message::ServiceAction(action) => {
                let Some(scope) = self.selected_connection.service_scope else {
                    return cosmic::Task::none();
                };

                self.service_state = Some(ServiceState::Checking);
                self.service_action = Some(action);

                return cosmic::Task::perform(
                    ServiceController::new(scope).action(action),
                    |state| cosmic::Action::App(Message::ServiceActionFinished(state)),
                );
            }

            Message::ServiceActionFinished(state) => {
                self.service_state = Some(state);
                self.service_action = None;
            }

            Message::ServiceSetupFinished(configuration, state) => {
                self.service_configuration = Some(configuration);
                self.service_state = Some(state);
            }

            Message::ApplyConfiguration => {
                if self
                    .applied_configuration
                    .matches(&self.selected_connection)
                    || self.configuration_applying
                {
                    return cosmic::Task::none();
                }

                let connection = self.selected_connection.clone();

                if let Some(scope) = connection.service_scope {
                    if self.applied_configuration.rpc_matches(&connection) {
                        self.save_config();
                        self.applied_configuration =
                            AppliedConfiguration::from_connection(&connection);
                        return cosmic::Task::none();
                    }

                    self.configuration_applying = true;

                    let id = connection.id;
                    let rpc_port = connection.rpc_port;
                    let rpc_username = connection.username;

                    return cosmic::Task::perform(
                        async move {
                            ServiceController::new(scope)
                                .apply_configuration(rpc_port, &rpc_username)
                                .await
                        },
                        move |result| {
                            cosmic::Action::App(Message::ConfigurationApplied(id, scope, result))
                        },
                    );
                }

                self.save_config();
                self.applied_configuration = AppliedConfiguration::from_connection(&connection);
            }

            Message::ConfigurationApplied(id, scope, result) => {
                self.configuration_applying = false;

                if self.selected_connection.id != id {
                    return cosmic::Task::none();
                }

                match result {
                    Ok(()) => {
                        self.save_config();
                        self.applied_configuration =
                            AppliedConfiguration::from_connection(&self.selected_connection);

                        return cosmic::Task::perform(
                            ServiceController::new(scope).status(),
                            move |state| {
                                cosmic::Action::App(Message::ConfigurationStateLoaded(id, state))
                            },
                        );
                    }
                    Err(error) => {
                        tracing::error!(%error, "failed to apply Transmission configuration");
                    }
                }
            }

            Message::UndoName => {
                let name = self.applied_configuration.name.clone();
                let id = self.selected_connection.id;

                self.selected_connection.name = name.clone();

                if let Some(connection) = self
                    .connections_config
                    .connections
                    .iter_mut()
                    .find(|connection| connection.id == id)
                {
                    connection.name = name;
                }
            }

            Message::UndoHost => {
                let host = self.applied_configuration.host.clone();
                let id = self.selected_connection.id;

                self.selected_connection.host = host.clone();

                if let Some(connection) = self
                    .connections_config
                    .connections
                    .iter_mut()
                    .find(|connection| connection.id == id)
                {
                    connection.host = host;
                }
            }

            Message::UndoUsername => {
                let username = self.applied_configuration.username.clone();
                let id = self.selected_connection.id;

                self.selected_connection.username = username.clone();

                if let Some(connection) = self
                    .connections_config
                    .connections
                    .iter_mut()
                    .find(|connection| connection.id == id)
                {
                    connection.username = username;
                }
            }

            Message::UndoRpcPort => {
                let rpc_port = self.applied_configuration.rpc_port;
                let id = self.selected_connection.id;

                self.selected_connection.rpc_port = rpc_port;

                if let Some(connection) = self
                    .connections_config
                    .connections
                    .iter_mut()
                    .find(|connection| connection.id == id)
                {
                    connection.rpc_port = rpc_port;
                }
            }

            Message::TogglePasswordVisibility => {
                self.password_hidden = !self.password_hidden;
            }

            Message::AddConnection => {
                let id = uuid::Uuid::new_v4();

                let connection = Connection {
                    id,
                    name: "New Connection".to_string(),
                    host: "localhost".to_string(),
                    rpc_port: 9091,
                    username: String::new(),
                    service_scope: None,
                    poll_interval: PollInterval::default(),
                };

                self.connections_config.connections.push(connection.clone());
                self.selected_connection = connection.clone();
                self.applied_configuration = AppliedConfiguration::from_connection(&connection);
                self.password = None;
                self.saved_password = None;
                self.service_configuration = None;
                self.service_state = None;
                self.service_action = None;
                self.configuration_applying = false;
            }

            Message::DeleteConnection(id) => {
                if !can_delete_connection(id) {
                    return cosmic::Task::none();
                }

                if let Some(index) = self
                    .connections_config
                    .connections
                    .iter()
                    .position(|connection| connection.id == id)
                {
                    self.connections_config.connections.remove(index);

                    if self.connections_config.active_connection == id {
                        self.connections_config.active_connection = LOCAL_USER_ID;
                    }

                    if self.selected_connection.id == id {
                        let selected_id = self.connections_config.active_connection;

                        self.selected_connection = self
                            .connections_config
                            .connections
                            .iter()
                            .find(|connection| connection.id == selected_id)
                            .cloned()
                            .expect("Local User connection must exist");

                        self.applied_configuration =
                            AppliedConfiguration::from_connection(&self.selected_connection);
                        self.password = None;
                        self.saved_password = None;
                        self.service_configuration = self
                            .selected_connection
                            .service_scope
                            .map(|_| ServiceConfiguration::Unconfigured);
                        self.service_state = self
                            .selected_connection
                            .service_scope
                            .map(|_| ServiceState::Checking);
                        self.service_action = None;
                        self.configuration_applying = false;

                        if let Some(scope) = self.selected_connection.service_scope {
                            let service_configuration_task = cosmic::Task::perform(
                                ServiceController::new(scope).configuration(),
                                |configuration| {
                                    cosmic::Action::App(Message::ServiceConfigurationLoaded(
                                        configuration,
                                    ))
                                },
                            );

                            let service_state_task = cosmic::Task::perform(
                                ServiceController::new(scope).status(),
                                |state| cosmic::Action::App(Message::ServiceStateLoaded(state)),
                            );

                            return cosmic::Task::batch([
                                service_configuration_task,
                                service_state_task,
                            ]);
                        }
                    }

                    return cosmic::Task::perform(credentials::delete_password(id), |_| {
                        cosmic::Action::App(Message::PasswordLoaded(None))
                    });
                }
            }

            Message::ReorderConnections(ids) => {
                self.connections_config.connections =
                    reorder_connections(&self.connections_config.connections, &ids);
            }

            Message::NameChanged(name) => {
                let id = self.selected_connection.id;

                self.selected_connection.name = name.clone();

                if let Some(connection) = self
                    .connections_config
                    .connections
                    .iter_mut()
                    .find(|connection| connection.id == id)
                {
                    connection.name = name;
                }
            }

            Message::HostChanged(host) => {
                let id = self.selected_connection.id;

                self.selected_connection.host = host.clone();

                if let Some(connection) = self
                    .connections_config
                    .connections
                    .iter_mut()
                    .find(|connection| connection.id == id)
                {
                    connection.host = host;
                }
            }

            Message::UsernameChanged(username) => {
                let id = self.selected_connection.id;

                self.selected_connection.username = username.clone();

                if let Some(connection) = self
                    .connections_config
                    .connections
                    .iter_mut()
                    .find(|connection| connection.id == id)
                {
                    connection.username = username;
                }
            }

            Message::PasswordChanged(password) => {
                self.password = Some(password);
            }

            Message::SavePassword => {
                let id = self.selected_connection.id;

                if let Some(password) = self.password.clone()
                    && !password.is_empty()
                    && self.saved_password.as_deref() != Some(password.as_str())
                {
                    return cosmic::Task::perform(
                        credentials::set_password(id, SecretString::from(password.clone())),
                        move |_| cosmic::Action::App(Message::PasswordLoaded(Some(password))),
                    );
                }
            }

            Message::ClearPassword => {
                let id = self.selected_connection.id;

                return cosmic::Task::perform(credentials::delete_password(id), |_| {
                    cosmic::Action::App(Message::PasswordLoaded(None))
                });
            }

            Message::RpcPortChanged(port) => {
                if let Some(port) = parse_rpc_port(&port) {
                    let id = self.selected_connection.id;

                    self.selected_connection.rpc_port = port;

                    if let Some(connection) = self
                        .connections_config
                        .connections
                        .iter_mut()
                        .find(|connection| connection.id == id)
                    {
                        connection.rpc_port = port;
                    }
                }
            }

            Message::ServiceScopeChanged(scope) => {
                if let Some(connection) = self.selected_connection_mut() {
                    apply_service_scope(connection, scope);
                }
            }

            Message::PollIntervalChanged(interval) => {
                if let Some(connection) = self.selected_connection_mut() {
                    connection.poll_interval = interval;
                }
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
    fn selected_connection_mut(&mut self) -> Option<&mut Connection> {
        self.connections_config
            .connections
            .iter_mut()
            .find(|connection| connection.id == self.selected_connection.id)
    }

    fn settings_view(&self) -> Element<'_, Message> {
        let spacing = theme::active().cosmic().spacing;

        let connection_section = widget::responsive(move |size| {
            let connection_header = widget::text::heading("Connections");

            let connection = &self.selected_connection;

            let navigation_mode = if size.width >= 700.0 {
                ConnectionNavigationMode::Full
            } else if size.width >= 520.0 {
                ConnectionNavigationMode::Compact
            } else {
                ConnectionNavigationMode::Minimal
            };

            let connection_list = ConnectionReorderList::new(
                self.connections_config.connections.clone(),
                self.selected_connection.id,
                self.connections_config.active_connection,
                Message::SelectConnection,
                Message::DeleteConnection,
                Message::ReorderConnections,
                navigation_mode,
            );

            let add_connection = add_connection_row(navigation_mode);

            let selected_is_local = connection.service_scope.is_some();
            let configuration_changed = !self.applied_configuration.matches(connection);

            let rpc_configuration_changed = !self.applied_configuration.rpc_matches(connection);

            let name_undo = widget::icon::from_name("edit-undo-symbolic")
                .symbolic(true)
                .size(16)
                .apply(widget::button::custom)
                .class(theme::Button::Icon)
                .on_press_maybe(
                    (connection.name != self.applied_configuration.name)
                        .then_some(Message::UndoName),
                );

            let host_undo = widget::icon::from_name("edit-undo-symbolic")
                .symbolic(true)
                .size(16)
                .apply(widget::button::custom)
                .class(theme::Button::Icon)
                .on_press_maybe(
                    (connection.host != self.applied_configuration.host)
                        .then_some(Message::UndoHost),
                );

            let username_undo = widget::icon::from_name("edit-undo-symbolic")
                .symbolic(true)
                .size(16)
                .apply(widget::button::custom)
                .class(theme::Button::Icon)
                .on_press_maybe(
                    (connection.username != self.applied_configuration.username)
                        .then_some(Message::UndoUsername),
                );

            let rpc_port_undo = widget::icon::from_name("edit-undo-symbolic")
                .symbolic(true)
                .size(16)
                .apply(widget::button::custom)
                .class(theme::Button::Icon)
                .on_press_maybe(
                    (connection.rpc_port != self.applied_configuration.rpc_port)
                        .then_some(Message::UndoRpcPort),
                );

            let name = widget::settings::item(
                "Name",
                widget::text_input("", &connection.name)
                    .on_input(Message::NameChanged)
                    .trailing_icon(name_undo.into())
                    .width(Length::Fixed(220.0)),
            );

            let host = widget::settings::item(
                "Host",
                widget::text_input("localhost", &connection.host)
                    .on_input(Message::HostChanged)
                    .trailing_icon(host_undo.into())
                    .width(Length::Fixed(220.0)),
            );

            let username = widget::settings::item(
                "Username",
                widget::text_input("Username", &connection.username)
                    .on_input(Message::UsernameChanged)
                    .trailing_icon(username_undo.into())
                    .width(Length::Fixed(220.0)),
            );

            let rpc_port = widget::settings::item(
                "RPC port",
                widget::text_input("9091", connection.rpc_port.to_string())
                    .on_input(Message::RpcPortChanged)
                    .trailing_icon(rpc_port_undo.into())
                    .width(Length::Fixed(220.0)),
            );

            let apply_button = if configuration_changed {
                widget::button::suggested("Apply").on_press_maybe(
                    (!self.configuration_applying).then_some(Message::ApplyConfiguration),
                )
            } else {
                widget::button::standard("Apply").on_press_maybe(None)
            };

            let apply = widget::settings::item("Configuration", apply_button);

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
                    Some(connection.poll_interval),
                    Message::PollIntervalChanged,
                )
                .width(Length::Fixed(140.0)),
            );

            let password = widget::settings::item(
                "Password",
                widget::secure_input(
                    "Password",
                    self.password.as_deref().unwrap_or(""),
                    Some(Message::TogglePasswordVisibility),
                    self.password_hidden,
                )
                .on_input(Message::PasswordChanged)
                .padding(5)
                .width(Length::Fixed(220.0)),
            );

            let details = if selected_is_local {
                let scope = widget::text(
                    connection
                        .service_scope
                        .expect("Local connection must have a service scope")
                        .to_string(),
                );

                let service_state = self.service_state.unwrap_or(ServiceState::Checking);
                let service_checking = matches!(service_state, ServiceState::Checking);

                let service_status = widget::text(
                    if self.service_action == Some(ServiceAction::Restart)
                        && matches!(service_state, ServiceState::Checking)
                    {
                        "Restarting…"
                    } else {
                        service_state_label(service_state)
                    },
                );

                let service_control = match self.service_configuration {
                    Some(ServiceConfiguration::Unconfigured) | None => widget::settings::item(
                        "Transmission service",
                        widget::button::standard("Setup").on_press(Message::SetupService),
                    ),
                    Some(ServiceConfiguration::Configured { .. }) => {
                        let service_buttons = widget::row::with_children(vec![
                            widget::button::standard("Start")
                                .on_press_maybe(
                                    (!service_checking
                                        && !self.configuration_applying
                                        && service_state != ServiceState::Running)
                                        .then_some(Message::ServiceAction(ServiceAction::Start)),
                                )
                                .into(),
                            widget::button::standard("Stop")
                                .on_press_maybe(
                                    (!service_checking
                                        && !self.configuration_applying
                                        && service_state != ServiceState::Stopped)
                                        .then_some(Message::ServiceAction(ServiceAction::Stop)),
                                )
                                .into(),
                            widget::button::standard("Restart")
                                .on_press_maybe(
                                    (!service_checking && !self.configuration_applying)
                                        .then_some(Message::ServiceAction(ServiceAction::Restart)),
                                )
                                .into(),
                        ])
                        .spacing(spacing.space_xxs);

                        widget::settings::item(
                            "Transmission service",
                            widget::column::with_children(vec![
                                service_status.into(),
                                service_buttons.into(),
                            ])
                            .spacing(spacing.space_xxs),
                        )
                    }
                };

                widget::column::with_children(vec![
                    name.into(),
                    host.into(),
                    username.into(),
                    rpc_port.into(),
                    apply.into(),
                    widget::settings::item("Scope", scope).into(),
                    poll_interval.into(),
                    service_control.into(),
                ])
                .spacing(spacing.space_s)
            } else {
                let _ = rpc_configuration_changed;

                let password_changed = self
                    .password
                    .as_deref()
                    .filter(|password| !password.is_empty())
                    != self.saved_password.as_deref();

                let password_saved = self.saved_password.is_some();

                let save_password = if password_changed {
                    widget::button::suggested("Save password").on_press(Message::SavePassword)
                } else {
                    widget::button::standard("Save password").on_press_maybe(None)
                };

                let clear_password = widget::button::standard("Clear password")
                    .on_press_maybe(password_saved.then_some(Message::ClearPassword));

                widget::column::with_children(vec![
                    name.into(),
                    host.into(),
                    username.into(),
                    rpc_port.into(),
                    apply.into(),
                    password.into(),
                    widget::row::with_children(vec![
                        widget::Space::new().width(Length::Fill).into(),
                        save_password.into(),
                        clear_password.into(),
                    ])
                    .spacing(spacing.space_xxs)
                    .into(),
                    poll_interval.into(),
                ])
                .spacing(spacing.space_s)
            };

            let navigation_width = match navigation_mode {
                ConnectionNavigationMode::Full => Length::Fixed(320.0),
                ConnectionNavigationMode::Compact => Length::Fixed(160.0),
                ConnectionNavigationMode::Minimal => Length::Fixed(56.0),
            };

            widget::column::with_children(vec![
                connection_header.into(),
                widget::row::with_children(vec![
                    widget::column::with_children(vec![
                        add_connection.into(),
                        widget::scrollable(connection_list)
                            .width(Length::Fill)
                            .height(Length::Fill)
                            .into(),
                    ])
                    .width(navigation_width)
                    .height(Length::Fill)
                    .spacing(spacing.space_xxs)
                    .into(),
                    widget::divider::vertical::default().into(),
                    widget::scrollable(details)
                        .width(Length::Fill)
                        .height(Length::Fill)
                        .into(),
                ])
                .spacing(spacing.space_s)
                .height(Length::Fill)
                .into(),
            ])
            .spacing(spacing.space_s)
            .height(Length::Fill)
            .into()
        })
        .height(Length::Fill);

        let settings = widget::column::with_children(vec![connection_section.into()])
            .spacing(spacing.space_l)
            .height(Length::Fill);

        let content = widget::container(settings)
            .class(theme::Container::WindowBackground)
            .padding([
                spacing.space_s,
                spacing.space_s,
                spacing.space_xxl,
                spacing.space_s,
            ])
            .height(Length::Fill);

        content.into()
    }

    fn save_config(&self) {
        let Ok(config) = cosmic::cosmic_config::Config::new(
            "io.github.cosmic.Transmission",
            ConnectionsConfig::VERSION,
        ) else {
            return;
        };

        let _ = self.connections_config.write_entry(&config);
    }
}

fn add_connection_row(navigation_mode: ConnectionNavigationMode) -> Element<'static, Message> {
    let spacing = theme::active().cosmic().spacing;

    let content: Element<'static, Message> = match navigation_mode {
        ConnectionNavigationMode::Full => {
            let content = widget::row::with_children(vec![
                widget::Space::new().width(Length::Fixed(16.0)).into(),
                widget::icon::from_name("list-add-symbolic")
                    .symbolic(true)
                    .size(20)
                    .into(),
                widget::text("Add a new connection").into(),
            ])
            .spacing(spacing.space_s)
            .align_y(Alignment::Center);

            content.into()
        }

        ConnectionNavigationMode::Compact => {
            let content = widget::row::with_children(vec![
                widget::icon::from_name("list-add-symbolic")
                    .symbolic(true)
                    .size(20)
                    .into(),
                widget::text("Add new...").into(),
            ])
            .spacing(spacing.space_s)
            .align_y(Alignment::Center);

            content.into()
        }

        ConnectionNavigationMode::Minimal => {
            let icon = widget::tooltip(
                widget::icon::from_name("list-add-symbolic")
                    .symbolic(true)
                    .size(20),
                widget::text("Add a new connection"),
                widget::tooltip::Position::Right,
            );

            widget::row::with_children(vec![
                widget::Space::new().width(Length::Fill).into(),
                icon.into(),
                widget::Space::new().width(Length::Fill).into(),
            ])
            .align_y(Alignment::Center)
            .into()
        }
    };

    widget::mouse_area(
        widget::container(content)
            .padding([20, 8])
            .width(Length::Fill)
            .class(theme::Container::Primary),
    )
    .on_press(Message::AddConnection)
    .into()
}
fn can_delete_connection(id: uuid::Uuid) -> bool {
    id != LOCAL_USER_ID && id != LOCAL_SYSTEM_ID
}

fn service_state_label(state: ServiceState) -> &'static str {
    match state {
        ServiceState::Running => "Running",
        ServiceState::Checking => "Checking…",
        ServiceState::Stopped => "Stopped",
        ServiceState::Error => "Error",
    }
}

fn parse_rpc_port(port: &str) -> Option<u16> {
    port.parse::<u16>().ok()
}

fn apply_service_scope(connection: &mut Connection, scope: ServiceScope) {
    match connection.id {
        LOCAL_USER_ID => connection.service_scope = Some(ServiceScope::User),
        LOCAL_SYSTEM_ID => connection.service_scope = Some(ServiceScope::System),
        _ => connection.service_scope = Some(scope),
    }
}

fn reorder_connections(connections: &[Connection], ids: &[uuid::Uuid]) -> Vec<Connection> {
    if ids.len() != connections.len() {
        return connections.to_vec();
    }

    let mut seen = std::collections::HashSet::with_capacity(ids.len());

    for id in ids {
        if !seen.insert(*id) || !connections.iter().any(|connection| connection.id == *id) {
            return connections.to_vec();
        }
    }

    ids.iter()
        .filter_map(|id| {
            connections
                .iter()
                .find(|connection| connection.id == *id)
                .cloned()
        })
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConnectionNavigationMode {
    Full,
    Compact,
    Minimal,
}

struct ConnectionReorderList<'a, Message> {
    id: cosmic::widget::Id,
    connections: Vec<Connection>,
    rows: Vec<Element<'a, Message>>,
    on_select: Box<dyn Fn(uuid::Uuid) -> Message + 'a>,
    on_reorder: Box<dyn Fn(Vec<uuid::Uuid>) -> Message + 'a>,
}

#[derive(Debug, Default, Clone)]
struct ConnectionReorderState {
    pressed: Option<(uuid::Uuid, Point)>,
    dragging: Option<uuid::Uuid>,
    cursor_position: Option<Point>,
    drag_offset: Option<Vector>,
}

impl<'a, Message: 'static + Clone> ConnectionReorderList<'a, Message> {
    fn new(
        connections: Vec<Connection>,
        selected: uuid::Uuid,
        active: uuid::Uuid,
        on_select: impl Fn(uuid::Uuid) -> Message + 'a,
        on_delete: impl Fn(uuid::Uuid) -> Message + 'a,
        on_reorder: impl Fn(Vec<uuid::Uuid>) -> Message + 'a,
        navigation_mode: ConnectionNavigationMode,
    ) -> Self {
        let rows = connections
            .iter()
            .map(|connection| {
                Self::connection_row(connection, selected, active, &on_delete, navigation_mode)
            })
            .collect();

        Self {
            id: cosmic::widget::Id::unique(),
            connections,
            rows,
            on_select: Box::new(on_select),
            on_reorder: Box::new(on_reorder),
        }
    }

    fn connection_row(
        connection: &Connection,
        _selected: uuid::Uuid,
        active: uuid::Uuid,
        on_delete: &dyn Fn(uuid::Uuid) -> Message,
        navigation_mode: ConnectionNavigationMode,
    ) -> Element<'a, Message> {
        let spacing = theme::active().cosmic().spacing;

        let icon_name = if connection.service_scope.is_some() {
            "computer-symbolic"
        } else {
            "network-server-symbolic"
        };

        let is_active = connection.id == active;

        match navigation_mode {
            ConnectionNavigationMode::Minimal => {
                let icon = widget::icon::from_name(icon_name).symbolic(true).size(20);

                let icon = widget::tooltip(
                    icon,
                    widget::text(connection.name.clone()),
                    widget::tooltip::Position::Right,
                );

                let content = widget::row::with_children(vec![
                    widget::Space::new().width(Length::Fill).into(),
                    icon.into(),
                    widget::Space::new().width(Length::Fill).into(),
                ])
                .align_y(Alignment::Center)
                .height(Length::Fill);

                let content: Element<'a, Message> = if is_active {
                    let active_indicator = widget::text("●").size(8).class(theme::Text::Color(
                        theme::active().cosmic().success_color().into(),
                    ));

                    cosmic::iced::widget::stack([
                        content.into(),
                        widget::container(active_indicator)
                            .width(Length::Fill)
                            .height(Length::Fixed(20.0))
                            .align_x(Alignment::End)
                            .align_y(Alignment::Center)
                            .into(),
                    ])
                    .into()
                } else {
                    content.into()
                };

                widget::container(content)
                    .padding(8)
                    .width(Length::Fill)
                    .class(theme::Container::Primary)
                    .into()
            }

            ConnectionNavigationMode::Compact => {
                let icon = widget::icon::from_name(icon_name).symbolic(true).size(20);

                let mut children = vec![icon.into(), widget::text(connection.name.clone()).into()];

                if is_active {
                    children.push(
                        widget::text("•")
                            .size(24)
                            .class(theme::Text::Color(
                                theme::active().cosmic().success_color().into(),
                            ))
                            .into(),
                    );
                }

                let content = widget::row::with_children(children)
                    .spacing(spacing.space_s)
                    .align_y(Alignment::Center);

                widget::container(content)
                    .padding(8)
                    .width(Length::Fill)
                    .class(theme::Container::Primary)
                    .into()
            }

            ConnectionNavigationMode::Full => {
                let description = if connection.service_scope.is_some() {
                    match connection.id {
                        LOCAL_USER_ID => "User service".to_string(),
                        LOCAL_SYSTEM_ID => "System service".to_string(),
                        _ => "Local service".to_string(),
                    }
                } else {
                    format!(
                        "{}:{}{}",
                        connection.host,
                        connection.rpc_port,
                        if connection.username.is_empty() {
                            String::new()
                        } else {
                            format!(" · {}", connection.username)
                        }
                    )
                };

                let mut children = vec![
                    widget::icon::from_name("list-drag-handle-symbolic")
                        .symbolic(true)
                        .size(16)
                        .into(),
                    widget::icon::from_name(icon_name)
                        .symbolic(true)
                        .size(20)
                        .into(),
                    widget::column::with_children(vec![
                        widget::text(connection.name.clone()).into(),
                        widget::text::caption(description).into(),
                    ])
                    .spacing(spacing.space_xxs)
                    .width(Length::Fill)
                    .into(),
                ];

                if is_active {
                    children.push(
                        widget::text("•")
                            .size(24)
                            .class(theme::Text::Color(
                                theme::active().cosmic().success_color().into(),
                            ))
                            .into(),
                    );
                }

                if connection.service_scope.is_none() {
                    children.push(
                        widget::button::icon(widget::icon::from_name("edit-delete-symbolic"))
                            .extra_small()
                            .on_press(on_delete(connection.id))
                            .into(),
                    );
                }

                let content = widget::row::with_children(children)
                    .spacing(spacing.space_s)
                    .align_y(Alignment::Center);

                widget::container(content)
                    .padding(8)
                    .width(Length::Fill)
                    .class(theme::Container::Primary)
                    .into()
            }
        }
    }

    fn row_at(&self, list_layout: layout::Layout<'_>, position: Point) -> Option<usize> {
        list_layout
            .children()
            .enumerate()
            .find_map(|(index, child)| child.bounds().contains(position).then_some(index))
    }

    fn reordered_ids(
        &self,
        list_layout: layout::Layout<'_>,
        position: Point,
        dragged_id: uuid::Uuid,
    ) -> Vec<uuid::Uuid> {
        let mut ids = self
            .connections
            .iter()
            .map(|connection| connection.id)
            .collect::<Vec<_>>();

        let Some(dragged_index) = ids.iter().position(|id| *id == dragged_id) else {
            return ids;
        };

        ids.remove(dragged_index);

        let mut target_index = ids.len();

        for (index, child) in list_layout.children().enumerate() {
            if self.connections[index].id == dragged_id {
                continue;
            }

            if position.y < child.bounds().center_y() {
                let target_id = self.connections[index].id;

                target_index = ids
                    .iter()
                    .position(|id| *id == target_id)
                    .unwrap_or(ids.len());

                break;
            }
        }

        ids.insert(target_index.min(ids.len()), dragged_id);
        ids
    }
}

impl<Message: 'static + Clone> Widget<Message, cosmic::Theme, cosmic::Renderer>
    for ConnectionReorderList<'_, Message>
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<ConnectionReorderState>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(ConnectionReorderState::default())
    }

    fn children(&self) -> Vec<Tree> {
        self.rows.iter().map(Tree::new).collect()
    }

    fn diff(&mut self, tree: &mut Tree) {
        let mut rows = self.rows.iter_mut().collect::<Vec<_>>();
        tree.diff_children(&mut rows);
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Shrink)
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &cosmic::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let spacing = theme::active().cosmic().spacing;
        let row_spacing = spacing.space_xxs as f32;

        let row_limits = limits.loose().width(Length::Fill).height(Length::Shrink);

        let mut children = Vec::with_capacity(self.rows.len());
        let mut y = 0.0;
        let mut width: f32 = 0.0;

        for (row, state) in self.rows.iter_mut().zip(tree.children.iter_mut()) {
            let mut node = row.as_widget_mut().layout(state, renderer, &row_limits);

            node = node.move_to(Point::new(0.0, y));

            width = width.max(node.size().width);
            y += node.size().height + row_spacing;

            children.push(node);
        }

        if !children.is_empty() {
            y -= row_spacing;
        }

        let size = limits.resolve(Length::Fill, Length::Shrink, Size::new(width, y.max(0.0)));

        layout::Node::with_children(size, children)
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: layout::Layout<'_>,
        renderer: &cosmic::Renderer,
        operation: &mut dyn Operation<()>,
    ) {
        for ((row, state), row_layout) in self
            .rows
            .iter_mut()
            .zip(tree.children.iter_mut())
            .zip(layout.children())
        {
            row.as_widget_mut()
                .operate(state, row_layout, renderer, operation);
        }
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &event::Event,
        layout: layout::Layout<'_>,
        cursor_position: mouse::Cursor,
        renderer: &cosmic::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        let mut row_layouts = layout.children();

        for ((row, state), row_layout) in self
            .rows
            .iter_mut()
            .zip(tree.children.iter_mut())
            .zip(&mut row_layouts)
        {
            row.as_widget_mut().update(
                state,
                event,
                row_layout,
                cursor_position,
                renderer,
                clipboard,
                shell,
                viewport,
            );

            if shell.is_event_captured() {
                return;
            }
        }

        let Some(position) = cursor_position.position() else {
            return;
        };

        let state = tree.state.downcast_mut::<ConnectionReorderState>();

        match event {
            event::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
            | event::Event::Touch(touch::Event::FingerPressed { .. })
                if layout.bounds().contains(position) =>
            {
                if let Some(index) = self.row_at(layout, position) {
                    state.pressed = Some((self.connections[index].id, position));
                    state.cursor_position = Some(position);
                    shell.capture_event();
                }
            }

            event::Event::Mouse(mouse::Event::CursorMoved { .. })
            | event::Event::Touch(touch::Event::FingerMoved { .. }) => {
                state.cursor_position = Some(position);

                let Some((pressed_id, start)) = state.pressed else {
                    return;
                };

                let dx = position.x - start.x;
                let dy = position.y - start.y;
                let distance_squared = dx * dx + dy * dy;

                if state.dragging.is_none() && distance_squared > DRAG_START_DISTANCE_SQUARED {
                    let dragged_index = self
                        .connections
                        .iter()
                        .position(|connection| connection.id == pressed_id);

                    if let Some(dragged_index) = dragged_index
                        && let Some(row_layout) = layout.children().nth(dragged_index)
                    {
                        let bounds = row_layout.bounds();

                        state.drag_offset =
                            Some(Vector::new(position.x - bounds.x, position.y - bounds.y));
                    }

                    state.dragging = Some(pressed_id);
                    shell.capture_event();
                    shell.request_redraw();
                }

                if let Some(dragged_id) = state.dragging {
                    let reordered = self.reordered_ids(layout, position, dragged_id);

                    let current = self
                        .connections
                        .iter()
                        .map(|connection| connection.id)
                        .collect::<Vec<_>>();

                    if reordered != current {
                        shell.publish((self.on_reorder)(reordered));
                    }

                    shell.capture_event();
                    shell.request_redraw();
                }
            }

            event::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
            | event::Event::Touch(
                touch::Event::FingerLifted { .. } | touch::Event::FingerLost { .. },
            ) => {
                if state.dragging.is_some() {
                    shell.capture_event();
                } else if let Some((id, _)) = state.pressed.take() {
                    shell.publish((self.on_select)(id));
                    shell.capture_event();
                }

                state.dragging = None;
                state.pressed = None;
                state.cursor_position = None;
                state.drag_offset = None;
                shell.request_redraw();
            }

            _ => {}
        }
    }

    fn draw(
        &self,
        state: &Tree,
        renderer: &mut cosmic::Renderer,
        theme: &cosmic::Theme,
        style: &renderer::Style,
        layout: layout::Layout<'_>,
        cursor_position: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let reorder_state = state.state.downcast_ref::<ConnectionReorderState>();
        let dragging = reorder_state.dragging;
        let drag_position = reorder_state.cursor_position;
        let drag_offset = reorder_state.drag_offset;

        let mut dragged_row = None;

        for ((index, row), (row_state, row_layout)) in self
            .rows
            .iter()
            .enumerate()
            .zip(state.children.iter().zip(layout.children()))
        {
            if dragging == Some(self.connections[index].id) {
                dragged_row = Some((row, row_state, row_layout));
                continue;
            }

            row.as_widget().draw(
                row_state,
                renderer,
                theme,
                style,
                row_layout,
                cursor_position,
                viewport,
            );
        }

        if let Some((row, row_state, row_layout)) = dragged_row
            && let (Some(position), Some(offset)) = (drag_position, drag_offset)
        {
            let bounds = row_layout.bounds();

            let target_position = Point::new(position.x - offset.x, position.y - offset.y);

            let translation =
                Vector::new(target_position.x - bounds.x, target_position.y - bounds.y);

            renderer.with_translation(translation, |renderer| {
                row.as_widget().draw(
                    row_state,
                    renderer,
                    theme,
                    style,
                    row_layout,
                    mouse::Cursor::Available(position),
                    viewport,
                );
            });
        }
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: layout::Layout<'b>,
        renderer: &cosmic::Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, cosmic::Theme, cosmic::Renderer>> {
        overlay::from_children(
            &mut self.rows,
            tree,
            layout,
            renderer,
            viewport,
            translation,
        )
    }

    fn mouse_interaction(
        &self,
        state: &Tree,
        layout: layout::Layout<'_>,
        cursor_position: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &cosmic::Renderer,
    ) -> mouse::Interaction {
        let reorder_state = state.state.downcast_ref::<ConnectionReorderState>();

        if reorder_state.dragging.is_some() {
            return mouse::Interaction::Grabbing;
        }

        let interaction = self
            .rows
            .iter()
            .zip(state.children.iter())
            .zip(layout.children())
            .map(|((row, row_state), row_layout)| {
                row.as_widget().mouse_interaction(
                    row_state,
                    row_layout,
                    cursor_position,
                    viewport,
                    renderer,
                )
            })
            .max()
            .unwrap_or_default();

        match interaction {
            mouse::Interaction::Idle => {
                if cursor_position.is_over(layout.bounds()) {
                    mouse::Interaction::Grab
                } else {
                    mouse::Interaction::default()
                }
            }
            interaction => interaction,
        }
    }

    fn id(&self) -> Option<cosmic::iced::runtime::core::id::Id> {
        Some(self.id.clone())
    }

    fn set_id(&mut self, id: cosmic::iced::runtime::core::id::Id) {
        self.id = id;
    }
}

impl<'a, Message: 'static + Clone> From<ConnectionReorderList<'a, Message>>
    for Element<'a, Message>
{
    fn from(list: ConnectionReorderList<'a, Message>) -> Self {
        Element::new(list)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Connection, ConnectionsConfig, PollInterval, ServiceScope};

    fn test_connection(id: uuid::Uuid, name: &str) -> Connection {
        Connection {
            id,
            name: name.to_string(),
            host: "localhost".to_string(),
            rpc_port: 9091,
            username: String::new(),
            service_scope: None,
            poll_interval: PollInterval::TwoSeconds,
        }
    }

    #[test]
    fn applied_configuration_matches_connection() {
        let id = uuid::Uuid::new_v4();
        let connection = test_connection(id, "Test");
        let applied = AppliedConfiguration::from_connection(&connection);

        assert!(applied.matches(&connection));
        assert!(applied.rpc_matches(&connection));
    }

    #[test]
    fn applied_configuration_detects_name_change() {
        let id = uuid::Uuid::new_v4();
        let connection = test_connection(id, "Test");
        let applied = AppliedConfiguration::from_connection(&connection);

        let mut changed = connection.clone();
        changed.name = "Changed".to_string();

        assert!(!applied.matches(&changed));
        assert!(applied.rpc_matches(&changed));
    }

    #[test]
    fn applied_configuration_detects_host_change() {
        let id = uuid::Uuid::new_v4();
        let connection = test_connection(id, "Test");
        let applied = AppliedConfiguration::from_connection(&connection);

        let mut changed = connection.clone();
        changed.host = "example.com".to_string();

        assert!(!applied.matches(&changed));
        assert!(applied.rpc_matches(&changed));
    }

    #[test]
    fn applied_configuration_detects_username_change() {
        let id = uuid::Uuid::new_v4();
        let connection = test_connection(id, "Test");
        let applied = AppliedConfiguration::from_connection(&connection);

        let mut changed = connection.clone();
        changed.username = "user".to_string();

        assert!(!applied.matches(&changed));
        assert!(!applied.rpc_matches(&changed));
    }

    #[test]
    fn applied_configuration_detects_rpc_port_change() {
        let id = uuid::Uuid::new_v4();
        let connection = test_connection(id, "Test");
        let applied = AppliedConfiguration::from_connection(&connection);

        let mut changed = connection.clone();
        changed.rpc_port = 1234;

        assert!(!applied.matches(&changed));
        assert!(!applied.rpc_matches(&changed));
    }

    #[test]
    fn local_connections_cannot_be_deleted() {
        assert!(!can_delete_connection(LOCAL_USER_ID));
        assert!(!can_delete_connection(LOCAL_SYSTEM_ID));
    }

    #[test]
    fn remote_connections_can_be_deleted() {
        let id = uuid::Uuid::new_v4();

        assert!(can_delete_connection(id));
    }

    #[test]
    fn parse_rpc_port_accepts_valid_port() {
        assert_eq!(parse_rpc_port("9091"), Some(9091));
    }

    #[test]
    fn parse_rpc_port_rejects_invalid_port() {
        assert_eq!(parse_rpc_port("invalid"), None);
    }

    #[test]
    fn parse_rpc_port_rejects_out_of_range_port() {
        assert_eq!(parse_rpc_port("65536"), None);
    }

    #[test]
    fn apply_service_scope_preserves_local_user_scope() {
        let mut connection = Connection {
            id: LOCAL_USER_ID,
            name: "Local User".to_string(),
            host: "localhost".to_string(),
            rpc_port: 9091,
            username: String::new(),
            service_scope: Some(ServiceScope::User),
            poll_interval: PollInterval::TwoSeconds,
        };

        apply_service_scope(&mut connection, ServiceScope::System);

        assert_eq!(connection.service_scope, Some(ServiceScope::User));
    }

    #[test]
    fn apply_service_scope_preserves_local_system_scope() {
        let mut connection = Connection {
            id: LOCAL_SYSTEM_ID,
            name: "Local System".to_string(),
            host: "localhost".to_string(),
            rpc_port: 9091,
            username: String::new(),
            service_scope: Some(ServiceScope::System),
            poll_interval: PollInterval::TwoSeconds,
        };

        apply_service_scope(&mut connection, ServiceScope::User);

        assert_eq!(connection.service_scope, Some(ServiceScope::System));
    }

    #[test]
    fn apply_service_scope_changes_remote_scope() {
        let id = uuid::Uuid::new_v4();

        let mut connection = test_connection(id, "Remote");

        apply_service_scope(&mut connection, ServiceScope::System);

        assert_eq!(connection.service_scope, Some(ServiceScope::System));
    }

    #[test]
    fn reorder_connections_reorders_by_id() {
        let first_id = uuid::Uuid::new_v4();
        let second_id = uuid::Uuid::new_v4();
        let third_id = uuid::Uuid::new_v4();

        let connections = vec![
            test_connection(first_id, "First"),
            test_connection(second_id, "Second"),
            test_connection(third_id, "Third"),
        ];

        let reordered = reorder_connections(&connections, &[third_id, first_id, second_id]);

        assert_eq!(
            reordered
                .iter()
                .map(|connection| connection.id)
                .collect::<Vec<_>>(),
            vec![third_id, first_id, second_id]
        );
    }

    #[test]
    fn reorder_connections_rejects_wrong_length() {
        let first_id = uuid::Uuid::new_v4();
        let second_id = uuid::Uuid::new_v4();

        let connections = vec![
            test_connection(first_id, "First"),
            test_connection(second_id, "Second"),
        ];

        let reordered = reorder_connections(&connections, &[first_id]);

        assert_eq!(reordered, connections);
    }

    #[test]
    fn reorder_connections_rejects_duplicate_ids() {
        let first_id = uuid::Uuid::new_v4();
        let second_id = uuid::Uuid::new_v4();

        let connections = vec![
            test_connection(first_id, "First"),
            test_connection(second_id, "Second"),
        ];

        let reordered = reorder_connections(&connections, &[first_id, first_id]);

        assert_eq!(reordered, connections);
    }

    #[test]
    fn reorder_connections_rejects_unknown_ids() {
        let first_id = uuid::Uuid::new_v4();
        let second_id = uuid::Uuid::new_v4();
        let unknown_id = uuid::Uuid::new_v4();

        let connections = vec![
            test_connection(first_id, "First"),
            test_connection(second_id, "Second"),
        ];

        let reordered = reorder_connections(&connections, &[first_id, unknown_id]);

        assert_eq!(reordered, connections);
    }

    #[test]
    fn reorder_connections_preserves_connection_data() {
        let first_id = uuid::Uuid::new_v4();
        let second_id = uuid::Uuid::new_v4();

        let mut first = test_connection(first_id, "First");
        first.host = "example.com".to_string();
        first.username = "user".to_string();
        first.rpc_port = 1234;

        let second = test_connection(second_id, "Second");

        let reordered =
            reorder_connections(&[first.clone(), second.clone()], &[second_id, first_id]);

        assert_eq!(reordered[0], second);
        assert_eq!(reordered[1], first);
    }
}