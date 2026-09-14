use std::process::Stdio;
use std::time::Duration;

use cosmic::{
    applet::{menu_button, padded_control},
    cosmic_theme::Spacing,
    iced::futures,
    iced::futures::SinkExt,
    iced::platform_specific::shell::wayland::commands::popup::{
        destroy_popup,
        get_popup,
    },
    iced::window::Id,
    iced::{Limits, Subscription},
    prelude::*,
    theme,
    widget,
};
use tokio::process::Command;

const SERVICE: &str = "transmission-daemon.service";
const WEB_UI: &str = "http://localhost:9091";
const RPC_URL: &str = "http://localhost:9091/transmission/rpc";
const REFRESH_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceState {
    Checking,
    Running,
    Stopped,
    Error,
}

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
    PopupClosed(Id),

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

    rpc_client: reqwest::Client,
    rpc_session_id: Option<String>,
}

impl Default for AppModel {
    fn default() -> Self {
        Self {
            core: cosmic::Core::default(),
            popup: None,
            state: ServiceState::Checking,
            service_enabled: false,
            stats: TransmissionStats::default(),
            rpc_client: reqwest::Client::new(),
            rpc_session_id: None,
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
    ) -> (
        Self,
        Task<cosmic::Action<Self::Message>>,
    ) {
        let app = Self {
            core,
            ..Default::default()
        };

        let rpc_client = app.rpc_client.clone();
        let rpc_session_id = app.rpc_session_id.clone();

        let task = Task::perform(
            query_transmission(rpc_client, rpc_session_id),
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
            .icon_button(self.panel_icon())
            .on_press(Message::TogglePopup)
            .into()
    }

    fn view_window(&self, _id: Id) -> Element<'_, Self::Message> {
        let Spacing {
            space_xxs,
            space_s,
            ..
        } = theme::active().cosmic().spacing;

        let service_toggle = widget::toggler(self.service_enabled)
            .label(Some("Transmission".to_string()))
            .width(cosmic::iced::Length::Fill)
            .text_size(14);

        let service_toggle = match self.state {
            ServiceState::Running | ServiceState::Stopped => {
                service_toggle.on_toggle(Message::ToggleService)
            }

            ServiceState::Checking | ServiceState::Error => {
                service_toggle
            }
        };

        let stats = widget::column::with_children([
            widget::text(format!(
                "↓ {}",
                format_speed(self.stats.download_speed)
            ))
            .into(),

            widget::text(format!(
                "↑ {}",
                format_speed(self.stats.upload_speed)
            ))
            .into(),
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
                menu_button(widget::text("Open Web UI"))
                    .on_press(Message::OpenWebUi)
            }

            ServiceState::Checking
            | ServiceState::Stopped
            | ServiceState::Error => {
                menu_button(widget::text("Open Web UI"))
            }
        };

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
        ]);

        self.core.applet.popup_container(content).into()
    }

    fn subscription(&self) -> Subscription<Self::Message> {
        Subscription::run(|| {
            cosmic::iced::stream::channel(
                1,
                |mut sender: futures::channel::mpsc::Sender<Message>| async move {
                    loop {
                        tokio::time::sleep(REFRESH_INTERVAL).await;

                        if sender.send(Message::Refresh).await.is_err() {
                            break;
                        }
                    }
                },
            )
        })
    }

    fn update(
        &mut self,
        message: Self::Message,
    ) -> Task<cosmic::Action<Self::Message>> {
        match message {
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

            Message::Refresh => {
                let rpc_client = self.rpc_client.clone();
                let rpc_session_id = self.rpc_session_id.clone();

                return Task::perform(
                    query_transmission(rpc_client, rpc_session_id),
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

                if matches!(
                    state,
                    ServiceState::Stopped | ServiceState::Error
                ) {
                    self.stats = TransmissionStats::default();
                }
            }

            Message::ToggleService(enabled) => {
                self.service_enabled = enabled;
                self.state = ServiceState::Checking;

                let action = if enabled {
                    "start"
                } else {
                    "stop"
                };

                return Task::perform(
                    service_action(action),
                    |state| {
                        cosmic::Action::App(
                            Message::ServiceActionFinished { state },
                        )
                    },
                );
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
                                cosmic::Action::App(
                                    Message::StatusChecked {
                                        state: ServiceState::Running,
                                        stats,
                                        rpc_session_id,
                                    },
                                )
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

                    ServiceState::Checking => {
                        // Retain the optimistic toggle state while
                        // the service is transitioning.
                    }
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
    fn panel_icon(&self) -> &'static str {
        match self.state {
            ServiceState::Checking => "network-server-symbolic",
            ServiceState::Running => "network-receive-symbolic",
            ServiceState::Stopped => "network-disconnected-symbolic",
            ServiceState::Error => "network-error-symbolic",
        }
    }
}

async fn service_status() -> ServiceState {
    let output = Command::new("systemctl")
        .args([
            "--user",
            "is-active",
            SERVICE,
        ])
        .output()
        .await;

    match output {
        Ok(output) if output.status.success() => {
            match String::from_utf8_lossy(&output.stdout).trim() {
                "active" => ServiceState::Running,
                "activating" | "deactivating" => ServiceState::Checking,
                _ => ServiceState::Error,
            }
        }

        Ok(output) => {
            match String::from_utf8_lossy(&output.stdout).trim() {
                "inactive" | "failed" => ServiceState::Stopped,
                "activating" | "deactivating" => ServiceState::Checking,
                _ => ServiceState::Error,
            }
        }

        Err(_) => ServiceState::Error,
    }
}

async fn service_action(action: &'static str) -> ServiceState {
    let result = Command::new("systemctl")
        .args([
            "--user",
            action,
            SERVICE,
        ])
        .status()
        .await;

    match result {
        Ok(status) if status.success() => {
            service_status().await
        }

        _ => ServiceState::Error,
    }
}

async fn query_transmission(
    client: reqwest::Client,
    session_id: Option<String>,
) -> (
    ServiceState,
    Option<TransmissionStats>,
    Option<String>,
) {
    let state = service_status().await;

    match state {
        ServiceState::Running => {
            let (stats, session_id) =
                query_stats(client, session_id).await;

            (ServiceState::Running, stats, session_id)
        }

        ServiceState::Stopped => {
            (
                ServiceState::Stopped,
                Some(TransmissionStats::default()),
                session_id,
            )
        }

        ServiceState::Checking => {
            (
                ServiceState::Checking,
                None,
                session_id,
            )
        }

        ServiceState::Error => {
            (
                ServiceState::Error,
                Some(TransmissionStats::default()),
                session_id,
            )
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
    client: reqwest::Client,
    session_id: Option<String>,
) -> (Option<TransmissionStats>, Option<String>) {
    let request = RpcRequest {
        method: "torrent-get",
        arguments: RpcRequestArguments {
            fields: [
                "status",
                "rateDownload",
                "rateUpload",
            ],
        },
    };

    let mut request_builder = client
        .post(RPC_URL)
        .json(&request);

    if let Some(session_id) = session_id.as_deref() {
        request_builder = request_builder
            .header(
                "X-Transmission-Session-Id",
                session_id,
            );
    }

    let response = match request_builder.send().await {
        Ok(response) => response,

        Err(_) => {
            return (
                None,
                session_id,
            );
        }
    };

    if response.status().is_success() {
        let stats = parse_rpc_response(response).await;

        return (
            Some(stats),
            session_id,
        );
    }

    if response.status() == reqwest::StatusCode::CONFLICT {
        let new_session_id = response
            .headers()
            .get("X-Transmission-Session-Id")
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);

        let Some(new_session_id) = new_session_id else {
            return (
                None,
                None,
            );
        };

        let response = match client
            .post(RPC_URL)
            .header(
                "X-Transmission-Session-Id",
                &new_session_id,
            )
            .json(&request)
            .send()
            .await
        {
            Ok(response) => response,

            Err(_) => {
                return (
                    None,
                    Some(new_session_id),
                );
            }
        };

        let stats = parse_rpc_response(response).await;

        return (
            Some(stats),
            Some(new_session_id),
        );
    }

    (
        None,
        session_id,
    )
}

async fn parse_rpc_response(
    response: reqwest::Response,
) -> TransmissionStats {
    let response: RpcResponse = match response.json().await {
        Ok(response) => response,

        Err(_) => {
            return TransmissionStats::default();
        }
    };

    if response.result != "success" {
        return TransmissionStats::default();
    }

    let torrents = match response
        .arguments
        .and_then(|args| args.torrents)
    {
        Some(torrents) => torrents,

        None => {
            return TransmissionStats::default();
        }
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
        format!(
            "{:.1} MiB/s",
            bytes_per_second as f64 / (1024.0 * 1024.0)
        )
    } else if bytes_per_second >= 1024 {
        format!(
            "{:.0} KiB/s",
            bytes_per_second as f64 / 1024.0
        )
    } else {
        format!("{} B/s", bytes_per_second)
    }
}