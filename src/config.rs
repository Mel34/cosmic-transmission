use cosmic_config::CosmicConfigEntry;

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum PollInterval {
    OneSecond,
    TwoSeconds,
    FiveSeconds,
    TenSeconds,
    ThirtySeconds,
}

impl Default for PollInterval {
    fn default() -> Self {
        Self::TwoSeconds
    }
}

impl PollInterval {
    pub fn duration(self) -> std::time::Duration {
        match self {
            Self::OneSecond => std::time::Duration::from_secs(1),
            Self::TwoSeconds => std::time::Duration::from_secs(2),
            Self::FiveSeconds => std::time::Duration::from_secs(5),
            Self::TenSeconds => std::time::Duration::from_secs(10),
            Self::ThirtySeconds => std::time::Duration::from_secs(30),
        }
    }
}

impl std::fmt::Display for PollInterval {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::OneSecond => "1 second",
            Self::TwoSeconds => "2 seconds",
            Self::FiveSeconds => "5 seconds",
            Self::TenSeconds => "10 seconds",
            Self::ThirtySeconds => "30 seconds",
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum ServiceScope {
    User,
    System,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Connection {
    pub id: uuid::Uuid,
    pub name: String,
    pub host: String,
    pub rpc_port: u16,
    pub username: String,
    pub service_scope: Option<ServiceScope>,
    pub poll_interval: PollInterval,
}

impl std::fmt::Display for Connection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.name)
    }
}

pub const LOCAL_USER_ID: uuid::Uuid = uuid::uuid!("00000000-0000-4000-8000-000000000001");

pub const LOCAL_SYSTEM_ID: uuid::Uuid = uuid::uuid!("00000000-0000-4000-8000-000000000002");

pub fn local_connections() -> [Connection; 2] {
    [
        Connection {
            id: LOCAL_USER_ID,
            name: "Local User".to_string(),
            host: "localhost".to_string(),
            rpc_port: 9091,
            username: String::new(),
            service_scope: Some(ServiceScope::User),
            poll_interval: PollInterval::default(),
        },
        Connection {
            id: LOCAL_SYSTEM_ID,
            name: "Local System".to_string(),
            host: "localhost".to_string(),
            rpc_port: 9091,
            username: String::new(),
            service_scope: Some(ServiceScope::System),
            poll_interval: PollInterval::default(),
        },
    ]
}

#[derive(
    Clone,
    cosmic_config::cosmic_config_derive::CosmicConfigEntry,
    Debug,
    Eq,
    PartialEq,
    serde::Serialize,
    serde::Deserialize,
)]
#[version = 1]
pub struct ConnectionsConfig {
    pub connections: Vec<Connection>,
    pub active_connection: uuid::Uuid,
}

impl Default for ConnectionsConfig {
    fn default() -> Self {
        let connections = local_connections().to_vec();

        Self {
            connections,
            active_connection: LOCAL_USER_ID,
        }
    }
}

impl std::fmt::Display for ServiceScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::User => "User",
            Self::System => "System",
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn poll_interval_defaults_to_two_seconds() {
        assert_eq!(PollInterval::default(), PollInterval::TwoSeconds);
    }

    #[test]
    fn poll_interval_durations_are_correct() {
        assert_eq!(
            PollInterval::OneSecond.duration(),
            std::time::Duration::from_secs(1)
        );
        assert_eq!(
            PollInterval::TwoSeconds.duration(),
            std::time::Duration::from_secs(2)
        );
        assert_eq!(
            PollInterval::FiveSeconds.duration(),
            std::time::Duration::from_secs(5)
        );
        assert_eq!(
            PollInterval::TenSeconds.duration(),
            std::time::Duration::from_secs(10)
        );
        assert_eq!(
            PollInterval::ThirtySeconds.duration(),
            std::time::Duration::from_secs(30)
        );
    }

    #[test]
    fn poll_interval_display_is_correct() {
        assert_eq!(PollInterval::OneSecond.to_string(), "1 second");
        assert_eq!(PollInterval::TwoSeconds.to_string(), "2 seconds");
        assert_eq!(PollInterval::FiveSeconds.to_string(), "5 seconds");
        assert_eq!(PollInterval::TenSeconds.to_string(), "10 seconds");
        assert_eq!(PollInterval::ThirtySeconds.to_string(), "30 seconds");
    }

    #[test]
    fn local_connections_are_correct() {
        let connections = local_connections();

        assert_eq!(connections.len(), 2);

        assert_eq!(connections[0].id, LOCAL_USER_ID);
        assert_eq!(connections[0].name, "Local User");
        assert_eq!(connections[0].host, "localhost");
        assert_eq!(connections[0].rpc_port, 9091);
        assert!(connections[0].username.is_empty());
        assert_eq!(connections[0].service_scope, Some(ServiceScope::User));
        assert_eq!(connections[0].poll_interval, PollInterval::TwoSeconds);

        assert_eq!(connections[1].id, LOCAL_SYSTEM_ID);
        assert_eq!(connections[1].name, "Local System");
        assert_eq!(connections[1].host, "localhost");
        assert_eq!(connections[1].rpc_port, 9091);
        assert!(connections[1].username.is_empty());
        assert_eq!(connections[1].service_scope, Some(ServiceScope::System));
        assert_eq!(connections[1].poll_interval, PollInterval::TwoSeconds);
    }

    #[test]
    fn connections_config_defaults_to_local_connections() {
        let config = ConnectionsConfig::default();

        assert_eq!(config.connections, local_connections().to_vec());
        assert_eq!(config.active_connection, LOCAL_USER_ID);
    }
}
#[test]
fn connections_config_round_trips_through_json() {
    let config = ConnectionsConfig::default();

    let serialized = serde_json::to_string(&config).expect("failed to serialize config");
    let deserialized: ConnectionsConfig =
        serde_json::from_str(&serialized).expect("failed to deserialize config");

    assert_eq!(deserialized, config);
}
