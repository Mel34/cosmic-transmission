use cosmic_config::CosmicConfigEntry;

#[derive(
    Clone,
    Copy,
    Debug,
    Eq,
    PartialEq,
    serde::Serialize,
    serde::Deserialize,
)]
pub enum ServiceScope {
    User,
    System,
}
#[derive(
    Clone,
    cosmic_config::cosmic_config_derive::CosmicConfigEntry,
    Debug,
    Eq,
    PartialEq,
)]
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
