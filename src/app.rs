use std::process::Stdio;

use cosmic_config::CosmicConfigEntry;
use cosmic_transmission::config::{AppConfig, Connection, ConnectionsConfig, ServiceScope};
use cosmic_transmission::credentials;
use cosmic_transmission::service::{ServiceController, ServiceState};

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

#[derive(Debug, Clone, Default)]
pub struct TransmissionStats {
    active: u32,
    downloading: u32,
    seeding: u32,
    download_speed: u64,
    upload_speed: u64,
}

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
    app_config: AppConfig,
}

impl Default for AppModel {
    fn default() -> Self {
        let connections_config = ConnectionsConfig::default();
        let connection = connections_config
            .connections
            .iter()
            .find(|connection| connection.id == connections_config.active_connection)
            .cloned()
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
            app_config: AppConfig::default(),
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
        let connections_config =
            cosmic::cosmic_config::Config::new(Self::APP_ID, ConnectionsConfig::VERSION)
                .ok()
                .map(|config| {
                    ConnectionsConfig::get_entry(&config).unwrap_or_else(|(_, config)| config)
                })
                .unwrap_or_default();

        let connection = connections_config
            .connections
            .iter()
            .find(|connection| connection.id == connections_config.active_connection)
            .cloned()
            .unwrap_or_else(|| connections_config.connections[0].clone());

        let app_config = cosmic::cosmic_config::Config::new(Self::APP_ID, AppConfig::VERSION)
            .ok()
            .map(|config| AppConfig::get_entry(&config).unwrap_or_else(|(_, config)| config))
            .unwrap_or_default();

        let rpc_client = RpcClient::new(&connection);

        let mut app = Self {
            core,
            connections_config,
            connection,
            app_config,
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
            cosmic::iced::time::every(self.app_config.poll_interval.duration())
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
                        let _ = cmd.status().await;
                    });
                }
            },

            Message::TogglePopup => {
                if let Some(popup) = self.popup.take() {
                    return destroy_popup(popup);
                }

                let old_active_connection = self.connections_config.active_connection;

                self.connections_config = AppModel::load_connections_config();

                let active_changed =
                    self.connections_config.active_connection != old_active_connection;

                let current_connection_exists = self
                    .connections_config
                    .connections
                    .iter()
                    .any(|connection| connection.id == self.connection.id);

                let connection_id = if active_changed || !current_connection_exists {
                    self.connections_config.active_connection
                } else {
                    self.connection.id
                };

                let Some(connection) = self
                    .connections_config
                    .connections
                    .iter()
                    .find(|connection| connection.id == connection_id)
                    .cloned()
                else {
                    return Task::none();
                };

                let connection_changed = connection.id != self.connection.id;

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
                let Some(connection) = self
                    .connections_config
                    .connections
                    .iter()
                    .find(|connection| connection.id == id)
                    .cloned()
                else {
                    return Task::none();
                };

                self.connections_config.active_connection = id;
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

                if self.connection.service_scope.is_some() {
                    match state {
                        ServiceState::Running => {
                            self.service_enabled = true;
                        }
                        ServiceState::Stopped => {
                            self.service_enabled = false;
                        }
                        ServiceState::Checking | ServiceState::Error => {}
                    }
                } else {
                    self.service_enabled = false;
                }

                if let Some(stats) = stats {
                    self.stats = stats;
                }

                self.rpc_session_id = rpc_session_id;

                if matches!(state, ServiceState::Stopped | ServiceState::Error) {
                    self.stats = TransmissionStats::default();
                }
            }

            Message::ToggleService(enabled) => {
                if self.connection.service_scope.is_none() {
                    return Task::none();
                }

                self.service_enabled = enabled;
                self.state = ServiceState::Checking;

                let action = if enabled { "start" } else { "stop" };

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
                let url = format!(
                    "http://{}:{}/transmission/web/",
                    self.connection.host, self.connection.rpc_port
                );

                tokio::spawn(async move {
                    let _ = Command::new("xdg-open")
                        .arg(url)
                        .stdin(Stdio::null())
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .spawn();
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
        let transmission = widget::icon::from_name("transmission")
            .symbolic(false)
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
            .size(10)
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

async fn query_connection(
    rpc_client: RpcClient,
    session_id: Option<String>,
    connection: Connection,
) -> (ServiceState, Option<TransmissionStats>, Option<String>) {
    if connection.service_scope.is_none() {
        let (stats, session_id) = query_stats(rpc_client, session_id).await;

        return match stats {
            Some(stats) => (ServiceState::Running, Some(stats), session_id),
            None => (ServiceState::Error, None, session_id),
        };
    }

    let service_scope = connection
        .service_scope
        .expect("local connection must have a service scope");

    let service = ServiceController::new(service_scope);
    let state = service.status().await;

    match state {
        ServiceState::Running => {
            let (stats, session_id) = query_stats(rpc_client, session_id).await;

            (ServiceState::Running, stats, session_id)
        }

        ServiceState::Stopped => (
            ServiceState::Stopped,
            Some(TransmissionStats::default()),
            session_id,
        ),

        ServiceState::Checking => (ServiceState::Checking, None, session_id),

        ServiceState::Error => (
            ServiceState::Error,
            Some(TransmissionStats::default()),
            session_id,
        ),
    }
}

#[derive(Clone)]
struct RpcClient {
    client: reqwest::Client,
    url: String,
    connection_id: uuid::Uuid,
    username: Option<String>,
}

impl RpcClient {
    fn new(connection: &Connection) -> Self {
        Self {
            client: reqwest::Client::new(),
            url: format!(
                "http://{}:{}/transmission/rpc",
                connection.host, connection.rpc_port
            ),
            connection_id: connection.id,
            username: if connection.username.is_empty() {
                None
            } else {
                Some(connection.username.clone())
            },
        }
    }
}

#[derive(Debug, serde::Deserialize)]
struct RpcResponse {
    result: String,
    arguments: Option<RpcArguments>,
}

#[derive(Debug, serde::Deserialize)]
struct RpcArguments {
    torrents: Option<Vec<RpcTorrent>>,
}

#[derive(Debug, serde::Deserialize)]
struct RpcTorrent {
    status: u8,
    #[serde(rename = "rateDownload")]
    rate_download: u64,
    #[serde(rename = "rateUpload")]
    rate_upload: u64,
}

#[derive(Debug, serde::Serialize)]
struct RpcRequest<'a> {
    method: &'a str,
    arguments: RpcRequestArguments,
}

#[derive(Debug, serde::Serialize)]
struct RpcRequestArguments {
    fields: [&'static str; 3],
}

async fn query_stats(
    rpc_client: RpcClient,
    session_id: Option<String>,
) -> (Option<TransmissionStats>, Option<String>) {
    let request = RpcRequest {
        method: "torrent-get",
        arguments: RpcRequestArguments {
            fields: ["status", "rateDownload", "rateUpload"],
        },
    };

    let password = if rpc_client.username.is_some() {
        credentials::get_password(rpc_client.connection_id).await
    } else {
        None
    };

    let mut request_builder = rpc_client.client.post(&rpc_client.url).json(&request);

    if let Some(username) = rpc_client.username.as_deref() {
        request_builder = request_builder.basic_auth(username, password.as_deref());
    }

    if let Some(session_id) = session_id.as_deref() {
        request_builder = request_builder.header("X-Transmission-Session-Id", session_id);
    }

    let response = match request_builder.send().await {
        Ok(response) => response,
        Err(_) => return (None, session_id),
    };

    if response.status().is_success() {
        let stats = parse_rpc_response(response).await;
        return (Some(stats), session_id);
    }

    if response.status() == reqwest::StatusCode::CONFLICT {
        let new_session_id = response
            .headers()
            .get("X-Transmission-Session-Id")
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);

        let Some(new_session_id) = new_session_id else {
            return (None, None);
        };

        let mut retry_request = rpc_client
            .client
            .post(&rpc_client.url)
            .header("X-Transmission-Session-Id", &new_session_id)
            .json(&request);

        if let Some(username) = rpc_client.username.as_deref() {
            retry_request = retry_request.basic_auth(username, password.as_deref());
        }

        let response = match retry_request.send().await {
            Ok(response) => response,
            Err(_) => return (None, Some(new_session_id)),
        };

        let stats = parse_rpc_response(response).await;
        return (Some(stats), Some(new_session_id));
    }

    (None, session_id)
}

async fn parse_rpc_response(response: reqwest::Response) -> TransmissionStats {
    let response: RpcResponse = match response.json().await {
        Ok(response) => response,
        Err(_) => return TransmissionStats::default(),
    };

    if response.result != "success" {
        return TransmissionStats::default();
    }

    let torrents = match response.arguments.and_then(|args| args.torrents) {
        Some(torrents) => torrents,
        None => return TransmissionStats::default(),
    };

    let mut stats = TransmissionStats::default();

    for torrent in torrents {
        if torrent.status != 0 {
            stats.active += 1;
        }

        match torrent.status {
            4 => stats.downloading += 1,
            6 => stats.seeding += 1,
            _ => {}
        }

        stats.download_speed += torrent.rate_download;
        stats.upload_speed += torrent.rate_upload;
    }

    stats
}

fn format_speed(bytes_per_second: u64) -> String {
    if bytes_per_second >= 1024 * 1024 {
        format!("{:.1} MiB/s", bytes_per_second as f64 / (1024.0 * 1024.0))
    } else if bytes_per_second >= 1024 {
        format!("{:.0} KiB/s", bytes_per_second as f64 / 1024.0)
    } else {
        format!("{} B/s", bytes_per_second)
    }
}
