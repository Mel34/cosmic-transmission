use cosmic_config::CosmicConfigEntry;

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
}

impl Default for ConnectionConfig {
    fn default() -> Self {
        Self {
            host: "localhost".to_string(),
            rpc_port: 9091,
        }
    }
}
