use std::process::Stdio;

use cosmic_config::CosmicConfigEntry;
use cosmic_transmission::config::{AppConfig, ConnectionConfig, ServiceScope};
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

const WEB_UI: &str = "http://localhost:9091";

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
    config: ConnectionConfig,
    rpc_client: RpcClient,
    rpc_session_id: Option<String>,
    token_tx: Option<calloop::channel::Sender<TokenRequest>>,
    app_config: AppConfig,
}

impl Default for AppModel {
    fn default() -> Self {
        Self {
            core: cosmic::Core::default(),
            popup: None,
            state: ServiceState::Checking,
            service_enabled: false,
            stats: TransmissionStats::default(),
            config: ConnectionConfig::default(),
            rpc_client: RpcClient::new(&ConnectionConfig::default()),
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
        let config = cosmic::cosmic_config::Config::new(Self::APP_ID, ConnectionConfig::VERSION)
            .ok()
            .map(|config| ConnectionConfig::get_entry(&config).unwrap_or_else(|(_, config)| config))
            .unwrap_or_default();

        let app_config = cosmic::cosmic_config::Config::new(Self::APP_ID, AppConfig::VERSION)
            .ok()
            .map(|config| AppConfig::get_entry(&config).unwrap_or_else(|(_, config)| config))
            .unwrap_or_default();

        let rpc_client = RpcClient::new(&config);

        let mut app = Self {
            core,
            config,
            app_config,
            ..Default::default()
        };

        app.rpc_client = rpc_client;

        let rpc_client = app.rpc_client.clone();
        let rpc_session_id = app.rpc_session_id.clone();

        let task = Task::perform(
            query_transmission(rpc_client, rpc_session_id, app.config.service_scope),
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

        let service_toggle = widget::toggler(self.service_enabled)
            .label(Some("Transmission".to_string()))
            .width(cosmic::iced::Length::Fill)
            .text_size(14);

        let service_toggle = match self.state {
            ServiceState::Running | ServiceState::Stopped => {
                service_toggle.on_toggle(Message::ToggleService)
            }
            ServiceState::Checking | ServiceState::Error => service_toggle,
        };

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
            padded_control(service_toggle).into(),
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

    fn update(&mut self, message: Self::Message) -> Task<cosmic::Action<Self::Message>> {
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

                return get_popup(settings);
            }

            Message::PopupClosed(id) => {
                if self.popup == Some(id) {
                    self.popup = None;
                }
            }

            Message::OpenSettings => {
                let exec = "cosmic-transmission-settings".to_string();

                if let Some(tx) = self.token_tx.as_ref() {
                    let _ = tx.send(TokenRequest {
                        app_id: Self::APP_ID.to_string(),
                        exec,
                    });
                }
            }

            Message::Refresh => {
                let rpc_client = self.rpc_client.clone();
                let rpc_session_id = self.rpc_session_id.clone();

                return Task::perform(
                    query_transmission(rpc_client, rpc_session_id, self.config.service_scope),
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

                match state {
                    ServiceState::Running => {
                        self.service_enabled = true;
                    }
                    ServiceState::Stopped => {
                        self.service_enabled = false;
                    }
                    ServiceState::Checking | ServiceState::Error => {}
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
                self.service_enabled = enabled;
                self.state = ServiceState::Checking;

                let action = if enabled { "start" } else { "stop" };

                let service = ServiceController::new(self.config.service_scope);

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
                tokio::spawn(async {
                    let _ = Command::new("xdg-open")
                        .arg(WEB_UI)
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

async fn query_transmission(
    rpc_client: RpcClient,
    session_id: Option<String>,
    service_scope: ServiceScope,
) -> (ServiceState, Option<TransmissionStats>, Option<String>) {
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
}

impl RpcClient {
    fn new(config: &ConnectionConfig) -> Self {
        Self {
            client: reqwest::Client::new(),
            url: format!(
                "http://{}:{}/transmission/rpc",
                config.host, config.rpc_port
            ),
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

    let mut request_builder = rpc_client.client.post(&rpc_client.url).json(&request);

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

        let response = match rpc_client
            .client
            .post(&rpc_client.url)
            .header("X-Transmission-Session-Id", &new_session_id)
            .json(&request)
            .send()
            .await
        {
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
