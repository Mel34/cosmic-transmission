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

#[derive(Clone, cosmic_config::cosmic_config_derive::CosmicConfigEntry, Debug, Eq, PartialEq)]
#[version = 1]
pub struct AppConfig {
    pub poll_interval: PollInterval,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            poll_interval: PollInterval::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum ServiceScope {
    User,
    System,
}
#[derive(Clone, cosmic_config::cosmic_config_derive::CosmicConfigEntry, Debug, Eq, PartialEq)]
#[version = 1]
pub struct ConnectionConfig {
    pub host: String,
    pub rpc_port: u16,
    pub service_scope: ServiceScope,
}

impl Default for ConnectionConfig {
    fn default() -> Self {
        Self {
            host: "localhost".to_string(),
            rpc_port: 9091,
            service_scope: ServiceScope::User,
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
