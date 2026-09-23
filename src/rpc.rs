use std::time::Duration;

use crate::config::Connection;
use crate::credentials;
use secrecy::ExposeSecret;

#[derive(Debug, Clone, Default)]
pub struct TransmissionStats {
    pub active: u32,
    pub downloading: u32,
    pub seeding: u32,
    pub download_speed: u64,
    pub upload_speed: u64,
}

#[derive(Clone)]
pub struct RpcClient {
    client: reqwest::Client,
    url: String,
    connection_id: uuid::Uuid,
    username: Option<String>,
}

impl RpcClient {
    pub fn new(connection: &Connection) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .expect("failed to build RPC HTTP client");

        Self {
            client,
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
    #[serde(rename = "rpc-port")]
    rpc_port: Option<u16>,
    #[serde(rename = "rpc-username")]
    rpc_username: Option<String>,
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

#[derive(Debug, serde::Serialize)]
struct SessionSetRequest<'a> {
    method: &'a str,
    arguments: SessionSetArguments<'a>,
}

#[derive(Debug, serde::Serialize)]
struct SessionSetArguments<'a> {
    #[serde(rename = "rpc-port")]
    rpc_port: u16,
    #[serde(rename = "rpc-username")]
    rpc_username: &'a str,
}

#[derive(Debug, serde::Serialize)]
struct SessionGetRequest<'a> {
    method: &'a str,
    arguments: SessionGetArguments,
}

#[derive(Debug, serde::Serialize)]
struct SessionGetArguments {
    fields: [&'static str; 2],
}

pub async fn query_connection(
    rpc_client: RpcClient,
    session_id: Option<String>,
    connection: Connection,
) -> (
    crate::service::ServiceState,
    Option<TransmissionStats>,
    Option<String>,
) {
    if connection.service_scope.is_none() {
        let (stats, session_id) = query_stats(rpc_client, session_id).await;

        return match stats {
            Some(stats) => (
                crate::service::ServiceState::Running,
                Some(stats),
                session_id,
            ),
            None => (crate::service::ServiceState::Error, None, session_id),
        };
    }

    let service_scope = connection
        .service_scope
        .expect("local connection must have a service scope");

    let service = crate::service::ServiceController::new(service_scope);
    let state = service.status().await;

    match state {
        crate::service::ServiceState::Running => {
            let (stats, session_id) = query_stats(rpc_client, session_id).await;

            (crate::service::ServiceState::Running, stats, session_id)
        }

        crate::service::ServiceState::Stopped => (
            crate::service::ServiceState::Stopped,
            Some(TransmissionStats::default()),
            session_id,
        ),

        crate::service::ServiceState::Checking => {
            (crate::service::ServiceState::Checking, None, session_id)
        }

        crate::service::ServiceState::Error => (
            crate::service::ServiceState::Error,
            Some(TransmissionStats::default()),
            session_id,
        ),
    }
}

pub async fn query_stats(
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
        request_builder = request_builder.basic_auth(
            username,
            password.as_ref().map(|password| password.expose_secret()),
        );
    }

    if let Some(session_id) = session_id.as_deref() {
        request_builder = request_builder.header("X-Transmission-Session-Id", session_id);
    }

    let response = match request_builder.send().await {
        Ok(response) => response,
        Err(error) => {
            tracing::warn!(%error, "Transmission RPC request failed");
            return (None, session_id);
        }
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
            tracing::warn!("Transmission RPC response did not include a session ID");
            return (None, None);
        };

        let mut retry_request = rpc_client
            .client
            .post(&rpc_client.url)
            .header("X-Transmission-Session-Id", &new_session_id)
            .json(&request);

        if let Some(username) = rpc_client.username.as_deref() {
            retry_request = retry_request.basic_auth(
                username,
                password.as_ref().map(|password| password.expose_secret()),
            );
        }

        let response = match retry_request.send().await {
            Ok(response) => response,
            Err(error) => {
                tracing::warn!(%error, "Transmission RPC retry failed");
                return (None, Some(new_session_id));
            }
        };

        let stats = parse_rpc_response(response).await;
        return (Some(stats), Some(new_session_id));
    }

    (None, session_id)
}

pub async fn apply_connection_settings(
    rpc_client: RpcClient,
    session_id: Option<String>,
    connection: &Connection,
) -> (bool, Option<String>) {
    let request = SessionSetRequest {
        method: "session-set",
        arguments: SessionSetArguments {
            rpc_port: connection.rpc_port,
            rpc_username: &connection.username,
        },
    };

    let password = if rpc_client.username.is_some() {
        credentials::get_password(rpc_client.connection_id).await
    } else {
        None
    };

    let (response, session_id) =
        send_rpc_request(&rpc_client, session_id, &request, password.as_ref()).await;

    let Some(response) = response else {
        return (false, session_id);
    };

    if response.result != "success" {
        tracing::warn!(
            result = %response.result,
            "Transmission session-set returned an error"
        );
        return (false, session_id);
    }

    let verification_client = RpcClient::new(connection);
    let request = SessionGetRequest {
        method: "session-get",
        arguments: SessionGetArguments {
            fields: ["rpc-port", "rpc-username"],
        },
    };

    let password = if verification_client.username.is_some() {
        credentials::get_password(verification_client.connection_id).await
    } else {
        None
    };

    let (response, session_id) = send_rpc_request(
        &verification_client,
        session_id,
        &request,
        password.as_ref(),
    )
    .await;

    let Some(response) = response else {
        return (false, session_id);
    };

    let Some(arguments) = response.arguments else {
        tracing::warn!("Transmission session-get response did not contain arguments");
        return (false, session_id);
    };

    let rpc_port_matches = arguments.rpc_port == Some(connection.rpc_port);
    let rpc_username_matches =
        arguments.rpc_username.as_deref() == Some(connection.username.as_str());

    if !rpc_port_matches || !rpc_username_matches {
        tracing::warn!(
            expected_port = connection.rpc_port,
            actual_port = ?arguments.rpc_port,
            expected_username = %connection.username,
            actual_username = ?arguments.rpc_username,
            "Transmission session settings verification failed"
        );
        return (false, session_id);
    }

    (true, session_id)
}

async fn send_rpc_request<T: serde::Serialize>(
    rpc_client: &RpcClient,
    session_id: Option<String>,
    request: &T,
    password: Option<&secrecy::SecretString>,
) -> (Option<RpcResponse>, Option<String>) {
    let mut request_builder = rpc_client.client.post(&rpc_client.url).json(request);

    if let Some(username) = rpc_client.username.as_deref() {
        request_builder =
            request_builder.basic_auth(username, password.map(|password| password.expose_secret()));
    }

    if let Some(session_id) = session_id.as_deref() {
        request_builder = request_builder.header("X-Transmission-Session-Id", session_id);
    }

    let response = match request_builder.send().await {
        Ok(response) => response,
        Err(error) => {
            tracing::warn!(%error, "Transmission RPC request failed");
            return (None, session_id);
        }
    };

    if response.status() == reqwest::StatusCode::CONFLICT {
        let new_session_id = response
            .headers()
            .get("X-Transmission-Session-Id")
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);

        let Some(new_session_id) = new_session_id else {
            tracing::warn!("Transmission RPC response did not include a session ID");
            return (None, None);
        };

        let mut retry_request = rpc_client
            .client
            .post(&rpc_client.url)
            .header("X-Transmission-Session-Id", &new_session_id)
            .json(request);

        if let Some(username) = rpc_client.username.as_deref() {
            retry_request = retry_request
                .basic_auth(username, password.map(|password| password.expose_secret()));
        }

        let response = match retry_request.send().await {
            Ok(response) => response,
            Err(error) => {
                tracing::warn!(%error, "Transmission RPC retry failed");
                return (None, Some(new_session_id));
            }
        };

        let response = match response.json::<RpcResponse>().await {
            Ok(response) => response,
            Err(error) => {
                tracing::warn!(%error, "failed to parse Transmission RPC response");
                return (None, Some(new_session_id));
            }
        };

        return (Some(response), Some(new_session_id));
    }

    if !response.status().is_success() {
        tracing::warn!(
            status = %response.status(),
            "Transmission RPC request returned an error"
        );
        return (None, session_id);
    }

    let response = match response.json::<RpcResponse>().await {
        Ok(response) => response,
        Err(error) => {
            tracing::warn!(%error, "failed to parse Transmission RPC response");
            return (None, session_id);
        }
    };

    (Some(response), session_id)
}

async fn parse_rpc_response(response: reqwest::Response) -> TransmissionStats {
    let response: RpcResponse = match response.json().await {
        Ok(response) => response,
        Err(error) => {
            tracing::warn!(%error, "failed to parse Transmission RPC response");
            return TransmissionStats::default();
        }
    };

    parse_rpc_arguments(response)
}

fn parse_rpc_arguments(response: RpcResponse) -> TransmissionStats {
    if response.result != "success" {
        tracing::warn!(result = %response.result, "Transmission RPC returned an error");
        return TransmissionStats::default();
    }

    let torrents = match response.arguments.and_then(|args| args.torrents) {
        Some(torrents) => torrents,
        None => return TransmissionStats::default(),
    };

    aggregate_torrents(torrents)
}

fn aggregate_torrents(torrents: Vec<RpcTorrent>) -> TransmissionStats {
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

pub fn format_speed(bytes_per_second: u64) -> String {
    if bytes_per_second >= 1024 * 1024 {
        format!("{:.1} MiB/s", bytes_per_second as f64 / (1024.0 * 1024.0))
    } else if bytes_per_second >= 1024 {
        format!("{:.0} KiB/s", bytes_per_second as f64 / 1024.0)
    } else {
        format!("{} B/s", bytes_per_second)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_speed_formats_bytes() {
        assert_eq!(format_speed(0), "0 B/s");
        assert_eq!(format_speed(512), "512 B/s");
        assert_eq!(format_speed(1023), "1023 B/s");
    }

    #[test]
    fn format_speed_formats_kibibytes() {
        assert_eq!(format_speed(1024), "1 KiB/s");
        assert_eq!(format_speed(1536), "2 KiB/s");
        assert_eq!(format_speed(1024 * 1024 - 1), "1024 KiB/s");
    }

    #[test]
    fn format_speed_formats_mebibytes() {
        assert_eq!(format_speed(1024 * 1024), "1.0 MiB/s");
        assert_eq!(format_speed(2 * 1024 * 1024), "2.0 MiB/s");
        assert_eq!(format_speed(1536 * 1024 * 1024), "1536.0 MiB/s");
    }

    #[test]
    fn parse_rpc_arguments_parses_successful_response() {
        let response = RpcResponse {
            result: "success".to_owned(),
            arguments: Some(RpcArguments {
                torrents: Some(vec![
                    RpcTorrent {
                        status: 4,
                        rate_download: 1024,
                        rate_upload: 256,
                    },
                    RpcTorrent {
                        status: 6,
                        rate_download: 2048,
                        rate_upload: 512,
                    },
                ]),
                rpc_port: None,
                rpc_username: None,
            }),
        };

        let stats = parse_rpc_arguments(response);

        assert_eq!(stats.active, 2);
        assert_eq!(stats.downloading, 1);
        assert_eq!(stats.seeding, 1);
        assert_eq!(stats.download_speed, 3072);
        assert_eq!(stats.upload_speed, 768);
    }

    #[test]
    fn parse_rpc_arguments_returns_default_for_rpc_error() {
        let response = RpcResponse {
            result: "invalid-method".to_owned(),
            arguments: None,
        };

        let stats = parse_rpc_arguments(response);

        assert_eq!(stats.active, 0);
        assert_eq!(stats.downloading, 0);
        assert_eq!(stats.seeding, 0);
        assert_eq!(stats.download_speed, 0);
        assert_eq!(stats.upload_speed, 0);
    }

    #[test]
    fn parse_rpc_arguments_returns_default_without_torrents() {
        let response = RpcResponse {
            result: "success".to_owned(),
            arguments: Some(RpcArguments {
                torrents: None,
                rpc_port: None,
                rpc_username: None,
            }),
        };

        let stats = parse_rpc_arguments(response);

        assert_eq!(stats.active, 0);
        assert_eq!(stats.downloading, 0);
        assert_eq!(stats.seeding, 0);
        assert_eq!(stats.download_speed, 0);
        assert_eq!(stats.upload_speed, 0);
    }

    #[test]
    fn aggregate_torrents_calculates_stats() {
        let torrents = vec![
            RpcTorrent {
                status: 4,
                rate_download: 1024,
                rate_upload: 256,
            },
            RpcTorrent {
                status: 6,
                rate_download: 2048,
                rate_upload: 512,
            },
            RpcTorrent {
                status: 0,
                rate_download: 4096,
                rate_upload: 1024,
            },
            RpcTorrent {
                status: 1,
                rate_download: 512,
                rate_upload: 128,
            },
        ];

        let stats = aggregate_torrents(torrents);

        assert_eq!(stats.active, 3);
        assert_eq!(stats.downloading, 1);
        assert_eq!(stats.seeding, 1);
        assert_eq!(stats.download_speed, 7680);
        assert_eq!(stats.upload_speed, 1920);
    }
}
