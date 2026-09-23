use std::process::Stdio;

use cosmic_config::CosmicConfigEntry;
use cosmic_transmission::config::{Connection, ConnectionsConfig, ServiceScope};
use cosmic_transmission::rpc::{
    RpcClient, TransmissionStats, format_speed, query_connection, query_stats,
};
use cosmic_transmission::service::{ServiceAction, ServiceController, ServiceState};

use cosmic::{
    applet::{menu_button, padded_control},
    cosmic_theme::Spacing,
    iced::platform_specific::shell::wayland::commands::popup::{destroy_popup, get_popup},
    iced::window::Id,
    iced::{Limits, Subscription},
    prelude::*,
    theme, widget,
};

use cosmic::{
    applet::token::subscription::{TokenRequest, TokenUpdate, activation_token_subscription},
    cctk::sctk::reexports::calloop,
};

use tokio::process::Command;

const STATUS_DOT_SVG: &[u8] = br#"
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 8 8">
  <circle cx="4" cy="4" r="4" fill="currentColor"/>
</svg>
"#;

const STATUS_ERROR_SVG: &[u8] = br#"
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 8 8">
  <path
    d="M1.5 1.5l5 5M6.5 1.5l-5 5"
    fill="none"
    stroke="currentColor"
    stroke-width="1.5"
    stroke-linecap="round"
  />
</svg>
"#;

#[derive(Debug, Clone)]
pub enum Message {
    TogglePopup,
    Token(TokenUpdate),
    PopupClosed(Id),
    OpenSettings,
    Refresh,
    SelectConnection(uuid::Uuid),
    StatusChecked {
        state: ServiceState,
        stats: Option<TransmissionStats>,
        rpc_session_id: Option<String>,
    },
    ToggleService(bool),
    ServiceActionFinished {
        state: ServiceState,
    },
    OpenWebUi,
}

pub struct AppModel {
    core: cosmic::Core,
    popup: Option<Id>,
    state: ServiceState,
    service_enabled: bool,
    stats: TransmissionStats,
    connections_config: ConnectionsConfig,
    connection: Connection,
    rpc_client: RpcClient,
    rpc_session_id: Option<String>,
    token_tx: Option<calloop::channel::Sender<TokenRequest>>,
}

impl Default for AppModel {
    fn default() -> Self {
        let connections_config = ConnectionsConfig::default();
        let connection = selected_connection(&connections_config)
            .unwrap_or_else(|| connections_config.connections[0].clone());

        Self {
            core: cosmic::Core::default(),
            popup: None,
            state: ServiceState::Checking,
            service_enabled: false,
            stats: TransmissionStats::default(),
            rpc_client: RpcClient::new(&connection),
            connections_config,
            connection,
            rpc_session_id: None,
            token_tx: None,
        }
    }
}

impl cosmic::Application for AppModel {
    type Executor = cosmic::executor::Default;
    type Flags = ();
    type Message = Message;

    const APP_ID: &'static str = "io.github.cosmic.Transmission";

    fn core(&self) -> &cosmic::Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut cosmic::Core {
        &mut self.core
    }

    fn init(
        core: cosmic::Core,
        _flags: Self::Flags,
    ) -> (Self, Task<cosmic::Action<Self::Message>>) {
        let connections_config = Self::load_connections_config();

        let connection = selected_connection(&connections_config)
            .unwrap_or_else(|| connections_config.connections[0].clone());

        let rpc_client = RpcClient::new(&connection);

        let mut app = Self {
            core,
            connections_config,
            connection,
            ..Default::default()
        };

        app.rpc_client = rpc_client;

        let rpc_client = app.rpc_client.clone();
        let rpc_session_id = app.rpc_session_id.clone();
        let connection = app.connection.clone();

        let task = Task::perform(
            query_connection(rpc_client, rpc_session_id, connection),
            |(state, stats, rpc_session_id)| {
                cosmic::Action::App(Message::StatusChecked {
                    state,
                    stats,
                    rpc_session_id,
                })
            },
        );

        (app, task)
    }

    fn on_close_requested(&self, id: Id) -> Option<Message> {
        Some(Message::PopupClosed(id))
    }

    fn view(&self) -> Element<'_, Self::Message> {
        self.core
            .applet
            .button_from_element(self.panel_icon(), false)
            .on_press(Message::TogglePopup)
            .into()
    }

    fn view_window(&self, id: Id) -> Element<'_, Self::Message> {
        if self.popup != Some(id) {
            return widget::column::with_children([]).into();
        }

        let Spacing {
            space_xxs, space_s, ..
        } = theme::active().cosmic().spacing;

        let connection_picker = cosmic::iced::widget::pick_list(
            self.connections_config.connections.clone(),
            Some(self.connection.clone()),
            |connection| Message::SelectConnection(connection.id),
        )
        .width(cosmic::iced::Length::Fill);

        let service_toggle = widget::toggler(self.service_enabled)
            .label(Some("Transmission".to_string()))
            .width(cosmic::iced::Length::Fill)
            .text_size(14);

        let service_toggle = if self.connection.service_scope.is_some() {
            match self.state {
                ServiceState::Running | ServiceState::Stopped => {
                    service_toggle.on_toggle(Message::ToggleService)
                }
                ServiceState::Checking | ServiceState::Error => service_toggle,
            }
        } else {
            service_toggle
        };

        let top_row = widget::row::with_children([connection_picker.into(), service_toggle.into()])
            .spacing(space_s);

        let stats = widget::column::with_children([
            widget::text(format!("↓ {}", format_speed(self.stats.download_speed))).into(),
            widget::text(format!("↑ {}", format_speed(self.stats.upload_speed))).into(),
        ])
        .spacing(space_xxs);

        let counts = widget::column::with_children([
            widget::row::with_children([
                widget::text("Downloading")
                    .width(cosmic::iced::Length::Fill)
                    .into(),
                widget::text(self.stats.downloading.to_string()).into(),
            ])
            .into(),
            widget::row::with_children([
                widget::text("Seeding")
                    .width(cosmic::iced::Length::Fill)
                    .into(),
                widget::text(self.stats.seeding.to_string()).into(),
            ])
            .into(),
            widget::row::with_children([
                widget::text("Active")
                    .width(cosmic::iced::Length::Fill)
                    .into(),
                widget::text(self.stats.active.to_string()).into(),
            ])
            .into(),
        ])
        .spacing(space_xxs);

        let web_ui = match self.state {
            ServiceState::Running => {
                menu_button(widget::text("Open Web UI")).on_press(Message::OpenWebUi)
            }
            ServiceState::Checking | ServiceState::Stopped | ServiceState::Error => {
                widget::button::custom(widget::text("Open Web UI"))
                    .padding(cosmic::applet::menu_control_padding())
                    .width(cosmic::iced::Length::Fill)
                    .class(theme::Button::MenuItem)
            }
        };

        let settings = menu_button(widget::text("Settings")).on_press(Message::OpenSettings);

        let content = widget::column::with_children([
            padded_control(top_row).into(),
            padded_control(widget::divider::horizontal::default())
                .padding([space_xxs, space_s])
                .into(),
            padded_control(stats).into(),
            padded_control(widget::divider::horizontal::default())
                .padding([space_xxs, space_s])
                .into(),
            padded_control(counts).into(),
            padded_control(widget::divider::horizontal::default())
                .padding([space_xxs, space_s])
                .into(),
            web_ui.into(),
            settings.into(),
        ]);

        self.core.applet.popup_container(content).into()
    }

    fn subscription(&self) -> Subscription<Self::Message> {
        Subscription::batch([
            activation_token_subscription(0).map(Message::Token),
            cosmic::iced::time::every(self.connection.poll_interval.duration())
                .map(|_| Message::Refresh),
        ])
    }

    fn update(&mut self, message: Message) -> Task<cosmic::Action<Self::Message>> {
        match message {
            Message::Token(update) => match update {
                TokenUpdate::Init(tx) => {
                    self.token_tx = Some(tx);
                }

                TokenUpdate::Finished => {
                    self.token_tx = None;
                }

                TokenUpdate::ActivationToken { token, .. } => {
                    let mut cmd = Command::new("cosmic-transmission-settings");

                    if let Some(token) = token {
                        cmd.env("XDG_ACTIVATION_TOKEN", &token);
                        cmd.env("DESKTOP_STARTUP_ID", &token);
                    }

                    tokio::spawn(async move {
                        if let Err(error) = cmd.status().await {
                            tracing::warn!(%error, "failed to launch Transmission settings");
                        }
                    });
                }
            },

            Message::TogglePopup => {
                if let Some(popup) = self.popup.take() {
                    return destroy_popup(popup);
                }

                self.connections_config = Self::load_connections_config();

                let Some(connection) = selected_connection(&self.connections_config) else {
                    return Task::none();
                };

                let connection_changed = connection_changed(&self.connection, &connection);

                self.connection = connection.clone();
                self.rpc_client = RpcClient::new(&connection);

                if connection_changed {
                    self.rpc_session_id = None;
                    self.stats = TransmissionStats::default();
                    self.service_enabled = false;
                    self.state = ServiceState::Checking;
                }

                let new_id = Id::unique();
                self.popup = Some(new_id);

                let mut settings = self.core.applet.get_popup_settings(
                    self.core.main_window_id().unwrap(),
                    new_id,
                    None,
                    None,
                    None,
                );

                settings.positioner.size_limits = Limits::NONE
                    .min_width(300.0)
                    .max_width(420.0)
                    .min_height(100.0)
                    .max_height(400.0);

                let popup_task = get_popup(settings);

                if connection_changed {
                    let rpc_client = self.rpc_client.clone();
                    let connection = self.connection.clone();

                    let query_task = Task::perform(
                        query_connection(rpc_client, None, connection),
                        |(state, stats, rpc_session_id)| {
                            cosmic::Action::App(Message::StatusChecked {
                                state,
                                stats,
                                rpc_session_id,
                            })
                        },
                    );

                    return Task::batch([popup_task, query_task]);
                }

                return popup_task;
            }

            Message::PopupClosed(id) => {
                if self.popup == Some(id) {
                    self.popup = None;
                }
            }

            Message::OpenSettings => {
                let exec = format!(
                    "cosmic-transmission-settings --connection {}",
                    self.connection.id
                );

                if let Some(tx) = self.token_tx.as_ref() {
                    let _ = tx.send(TokenRequest {
                        app_id: Self::APP_ID.to_string(),
                        exec,
                    });
                }
            }

            Message::SelectConnection(id) => {
                let mut connections_config = Self::load_connections_config();

                let Some(connection) = connections_config
                    .connections
                    .iter()
                    .find(|connection| connection.id == id)
                    .cloned()
                else {
                    return Task::none();
                };

                connections_config.active_connection = id;
                self.connections_config = connections_config;
                self.connection = connection;
                self.rpc_client = RpcClient::new(&self.connection);
                self.rpc_session_id = None;
                self.stats = TransmissionStats::default();
                self.service_enabled = false;
                self.state = ServiceState::Checking;

                self.save_connections_config();

                let rpc_client = self.rpc_client.clone();
                let connection = self.connection.clone();

                return Task::perform(
                    query_connection(rpc_client, None, connection),
                    |(state, stats, rpc_session_id)| {
                        cosmic::Action::App(Message::StatusChecked {
                            state,
                            stats,
                            rpc_session_id,
                        })
                    },
                );
            }

            Message::Refresh => {
                let rpc_client = self.rpc_client.clone();
                let rpc_session_id = self.rpc_session_id.clone();
                let connection = self.connection.clone();

                return Task::perform(
                    query_connection(rpc_client, rpc_session_id, connection),
                    |(state, stats, rpc_session_id)| {
                        cosmic::Action::App(Message::StatusChecked {
                            state,
                            stats,
                            rpc_session_id,
                        })
                    },
                );
            }

            Message::StatusChecked {
                state,
                stats,
                rpc_session_id,
            } => {
                self.state = state;
                self.service_enabled =
                    service_enabled_for_state(self.connection.service_scope, state);

                if let Some(stats) = stats {
                    self.stats = stats;
                }

                self.rpc_session_id = rpc_session_id;

                if clears_stats_for_state(state) {
                    self.stats = TransmissionStats::default();
                }
            }

            Message::ToggleService(enabled) => {
                if self.connection.service_scope.is_none() {
                    return Task::none();
                }

                self.service_enabled = enabled;
                self.state = ServiceState::Checking;

                let action = if enabled {
                    ServiceAction::Start
                } else {
                    ServiceAction::Stop
                };

                let service = ServiceController::new(self.service_scope());

                return Task::perform(service.action(action), |state| {
                    cosmic::Action::App(Message::ServiceActionFinished { state })
                });
            }

            Message::ServiceActionFinished { state } => {
                self.state = state;

                match state {
                    ServiceState::Running => {
                        self.service_enabled = true;

                        let rpc_client = self.rpc_client.clone();
                        let rpc_session_id = self.rpc_session_id.clone();

                        return Task::perform(
                            query_stats(rpc_client, rpc_session_id),
                            |(stats, rpc_session_id)| {
                                cosmic::Action::App(Message::StatusChecked {
                                    state: ServiceState::Running,
                                    stats,
                                    rpc_session_id,
                                })
                            },
                        );
                    }

                    ServiceState::Stopped => {
                        self.service_enabled = false;
                        self.stats = TransmissionStats::default();
                    }

                    ServiceState::Error => {
                        self.stats = TransmissionStats::default();
                    }

                    ServiceState::Checking => {}
                }
            }

            Message::OpenWebUi => {
                let url = web_ui_url(&self.connection);

                tokio::spawn(async move {
                    if let Err(error) = Command::new("xdg-open")
                        .arg(url)
                        .stdin(Stdio::null())
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .spawn()
                    {
                        tracing::warn!(%error, "failed to launch Transmission Web UI");
                    }
                });
            }
        }

        Task::none()
    }

    fn style(&self) -> Option<cosmic::iced::theme::Style> {
        Some(cosmic::applet::style())
    }
}

impl AppModel {
    fn service_scope(&self) -> ServiceScope {
        self.connection
            .service_scope
            .expect("service_scope is only used for local connections")
    }

    fn save_connections_config(&self) {
        let Ok(config) = cosmic::cosmic_config::Config::new(
            "io.github.cosmic.Transmission",
            ConnectionsConfig::VERSION,
        ) else {
            return;
        };

        let _ = self.connections_config.write_entry(&config);
    }

    fn load_connections_config() -> ConnectionsConfig {
        cosmic::cosmic_config::Config::new(
            "io.github.cosmic.Transmission",
            ConnectionsConfig::VERSION,
        )
        .ok()
        .map(|config| ConnectionsConfig::get_entry(&config).unwrap_or_else(|(_, config)| config))
        .unwrap_or_default()
    }

    fn panel_icon(&self) -> Element<'_, Message> {
        let transmission = widget::icon::from_name("cosmic-transmission")
            .symbolic(true)
            .size(16);

        let (status_svg, status_color) = match self.state {
            ServiceState::Running => (STATUS_DOT_SVG, theme::active().cosmic().success_color()),
            ServiceState::Checking => (STATUS_DOT_SVG, theme::active().cosmic().warning_color()),
            ServiceState::Stopped => (STATUS_DOT_SVG, theme::active().cosmic().destructive_color()),
            ServiceState::Error => (
                STATUS_ERROR_SVG,
                theme::active().cosmic().destructive_color(),
            ),
        };

        let status_handle = widget::icon::from_svg_bytes(status_svg).symbolic(true);

        let status = widget::icon(status_handle)
            .size(6)
            .class(theme::Svg::custom(move |_| {
                cosmic::iced::widget::svg::Style {
                    color: Some(status_color.into()),
                }
            }));

        cosmic::iced::widget::stack([
            transmission.into(),
            widget::container(status)
                .width(cosmic::iced::Length::Fill)
                .height(cosmic::iced::Length::Fill)
                .align_x(cosmic::iced::Alignment::End)
                .align_y(cosmic::iced::Alignment::End)
                .into(),
        ])
        .into()
    }
}

fn selected_connection(config: &ConnectionsConfig) -> Option<Connection> {
    config
        .connections
        .iter()
        .find(|connection| connection.id == config.active_connection)
        .cloned()
}

fn connection_changed(current: &Connection, next: &Connection) -> bool {
    current.id != next.id || current.poll_interval != next.poll_interval
}

fn service_enabled_for_state(scope: Option<ServiceScope>, state: ServiceState) -> bool {
    if scope.is_none() {
        return false;
    }

    match state {
        ServiceState::Running => true,
        ServiceState::Stopped => false,
        ServiceState::Checking | ServiceState::Error => false,
    }
}

fn clears_stats_for_state(state: ServiceState) -> bool {
    matches!(state, ServiceState::Stopped | ServiceState::Error)
}

fn web_ui_url(connection: &Connection) -> String {
    format!(
        "http://{}:{}/transmission/web/",
        connection.host, connection.rpc_port
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use cosmic_transmission::config::PollInterval;

    fn test_connection(id: uuid::Uuid) -> Connection {
        Connection {
            id,
            name: "Test".to_owned(),
            host: "localhost".to_owned(),
            rpc_port: 9091,
            username: String::new(),
            service_scope: None,
            poll_interval: PollInterval::TwoSeconds,
        }
    }

    #[test]
    fn selected_connection_returns_active_connection() {
        let active_id = uuid::Uuid::new_v4();
        let other_id = uuid::Uuid::new_v4();

        let config = ConnectionsConfig {
            connections: vec![test_connection(other_id), test_connection(active_id)],
            active_connection: active_id,
        };

        assert_eq!(selected_connection(&config).unwrap().id, active_id);
    }

    #[test]
    fn selected_connection_returns_none_for_unknown_active_id() {
        let connection_id = uuid::Uuid::new_v4();
        let active_id = uuid::Uuid::new_v4();

        let config = ConnectionsConfig {
            connections: vec![test_connection(connection_id)],
            active_connection: active_id,
        };

        assert!(selected_connection(&config).is_none());
    }

    #[test]
    fn connection_changed_detects_different_connection() {
        let first_id = uuid::Uuid::new_v4();
        let second_id = uuid::Uuid::new_v4();

        let current = test_connection(first_id);
        let next = test_connection(second_id);

        assert!(connection_changed(&current, &next));
    }

    #[test]
    fn connection_changed_detects_poll_interval_change() {
        let id = uuid::Uuid::new_v4();

        let current = test_connection(id);
        let mut next = test_connection(id);
        next.poll_interval = PollInterval::FiveSeconds;

        assert!(connection_changed(&current, &next));
    }

    #[test]
    fn connection_changed_accepts_same_connection_and_poll_interval() {
        let id = uuid::Uuid::new_v4();

        let current = test_connection(id);
        let next = test_connection(id);

        assert!(!connection_changed(&current, &next));
    }

    #[test]
    fn service_enabled_for_state_requires_local_service_scope() {
        assert!(service_enabled_for_state(
            Some(ServiceScope::User),
            ServiceState::Running
        ));
        assert!(service_enabled_for_state(
            Some(ServiceScope::System),
            ServiceState::Running
        ));
        assert!(!service_enabled_for_state(None, ServiceState::Running));
    }

    #[test]
    fn service_enabled_for_state_is_false_when_service_is_stopped() {
        assert!(!service_enabled_for_state(
            Some(ServiceScope::User),
            ServiceState::Stopped
        ));
    }

    #[test]
    fn service_enabled_for_state_is_false_while_checking_or_on_error() {
        assert!(!service_enabled_for_state(
            Some(ServiceScope::User),
            ServiceState::Checking
        ));
        assert!(!service_enabled_for_state(
            Some(ServiceScope::User),
            ServiceState::Error
        ));
    }

    #[test]
    fn clears_stats_for_stopped_and_error_states() {
        assert!(clears_stats_for_state(ServiceState::Stopped));
        assert!(clears_stats_for_state(ServiceState::Error));
    }

    #[test]
    fn clears_stats_for_running_and_checking_states() {
        assert!(!clears_stats_for_state(ServiceState::Running));
        assert!(!clears_stats_for_state(ServiceState::Checking));
    }

    #[test]
    fn web_ui_url_uses_connection_host_and_port() {
        let connection = test_connection(uuid::Uuid::new_v4());

        assert_eq!(
            web_ui_url(&connection),
            "http://localhost:9091/transmission/web/"
        );
    }
}
