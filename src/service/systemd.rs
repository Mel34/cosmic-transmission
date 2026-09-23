use super::{ServiceAction, ServiceConfiguration, ServiceState};
use crate::config::ServiceScope;

const SERVICE: &str = "transmission-daemon.service";
const MANAGEMENT_MARKER: &str = "# Managed by cosmic-transmission";

#[derive(Clone, Copy)]
pub struct SystemdServiceController {
    scope: ServiceScope,
}

impl SystemdServiceController {
    pub fn new(scope: ServiceScope) -> Self {
        Self { scope }
    }

    pub async fn configuration(&self) -> ServiceConfiguration {
        let connection = match self.connection().await {
            Ok(connection) => connection,
            Err(error) => {
                println!("systemd connection error: {error}");
                return ServiceConfiguration::Unconfigured;
            }
        };

        let configuration = match service_configuration(&connection).await {
            Some(configuration) => configuration,
            None => {
                println!("systemd service configuration: None");
                return ServiceConfiguration::Unconfigured;
            }
        };

        println!(
            "systemd service configuration: load_state={:?}, drop_in_paths={:?}, managed={}",
            configuration.load_state, configuration.drop_in_paths, configuration.managed
        );

        match configuration.load_state.as_str() {
            "loaded" if !configuration.drop_in_paths.is_empty() => {
                ServiceConfiguration::Configured {
                    managed: configuration.managed,
                }
            }
            _ => ServiceConfiguration::Unconfigured,
        }
    }

    pub async fn setup(&self, username: Option<String>) -> super::ServiceSetupResult {
        let connection = match self.connection().await {
            Ok(connection) => connection,
            Err(error) => {
                tracing::error!(?self.scope, %error, "failed to connect to systemd for setup");
                return super::ServiceSetupResult::Failed;
            }
        };

        let configuration = match service_configuration(&connection).await {
            Some(configuration) => configuration,
            None => {
                tracing::error!(?self.scope, "failed to inspect systemd service configuration");
                return super::ServiceSetupResult::Failed;
            }
        };

        if !configuration.drop_in_paths.is_empty() && !configuration.managed {
            tracing::warn!(
                ?self.scope,
                paths = ?configuration.drop_in_paths,
                "refusing to modify unmanaged systemd configuration"
            );
            return super::ServiceSetupResult::UnmanagedConfiguration;
        }

        let username = match self.scope {
            ServiceScope::System => {
                let Some(username) = username.as_deref() else {
                    tracing::error!("system scope setup requires a username");
                    return super::ServiceSetupResult::Failed;
                };

                Some(username)
            }
            ServiceScope::User => None,
        };

        let setup_result = match self.scope {
            ServiceScope::System => {
                let Some(username) = username else {
                    return super::ServiceSetupResult::Failed;
                };

                setup_system_configuration(&connection, username).await
            }
            ServiceScope::User => match write_managed_configuration(self.scope, username) {
                Ok(()) => Ok(()),
                Err(error) => Err(error.to_string()),
            },
        };

        if let Err(error) = setup_result {
            tracing::error!(
                ?self.scope,
                %error,
                "failed to write managed systemd configuration"
            );
            return super::ServiceSetupResult::Failed;
        }

        if let Err(error) = reload_systemd(&connection).await {
            tracing::error!(
                ?self.scope,
                %error,
                "failed to reload systemd"
            );
            return super::ServiceSetupResult::Failed;
        }

        let state = service_action(&connection, ServiceAction::Restart).await;

        if !matches!(state, ServiceState::Running) {
            tracing::error!(
                ?self.scope,
                ?state,
                "failed to start transmission service after setup"
            );
            return super::ServiceSetupResult::Failed;
        }

        super::ServiceSetupResult::Success
    }

    pub async fn status(self) -> ServiceState {
        let Ok(connection) = self.connection().await else {
            tracing::error!(?self.scope, "failed to connect to systemd");
            return ServiceState::Error;
        };

        service_status(&connection).await
    }

    pub async fn action(self, action: ServiceAction) -> ServiceState {
        let Ok(connection) = self.connection().await else {
            tracing::error!(?self.scope, ?action, "failed to connect to systemd");
            return ServiceState::Error;
        };

        service_action(&connection, action).await
    }

    pub async fn username(self) -> Option<String> {
        match self.scope {
            ServiceScope::User => std::env::var("USER").ok(),
            ServiceScope::System => {
                let connection = match self.connection().await {
                    Ok(connection) => connection,
                    Err(error) => {
                        tracing::error!(?self.scope, %error, "failed to connect to systemd");
                        return None;
                    }
                };

                service_username(&connection).await
            }
        }
    }

    pub async fn apply_configuration(
        self,
        rpc_port: u16,
        rpc_username: &str,
    ) -> Result<(), String> {
        let connection = self.connection().await.map_err(|error| error.to_string())?;

        let state = service_status(&connection).await;
        let was_running = matches!(state, ServiceState::Running);

        if was_running {
            let state = service_action(&connection, ServiceAction::Stop).await;

            if !matches!(state, ServiceState::Stopped) {
                return Err(format!(
                    "failed to stop Transmission before editing configuration: {state:?}"
                ));
            }
        }

        let result = match self.scope {
            ServiceScope::User => {
                let username = std::env::var("USER").ok();

                let path = transmission_settings_path(username.as_deref())
                    .ok_or_else(|| "could not determine Transmission settings path".to_string())?;

                update_transmission_settings(&path, rpc_port, rpc_username)
            }
            ServiceScope::System => {
                let username = service_username(&connection).await.ok_or_else(|| {
                    "could not determine Transmission service username".to_string()
                })?;

                update_system_configuration(&connection, &username, rpc_port, rpc_username).await
            }
        };

        if was_running {
            let restart_state = service_action(&connection, ServiceAction::Start).await;

            if let Err(error) = result {
                if !matches!(restart_state, ServiceState::Running) {
                    return Err(format!(
                        "{error}; additionally failed to restart Transmission: {restart_state:?}"
                    ));
                }

                return Err(error);
            }

            if !matches!(restart_state, ServiceState::Running) {
                return Err(format!(
                    "configuration was updated, but failed to restart Transmission: {restart_state:?}"
                ));
            }
        }

        result
    }

    async fn connection(self) -> zbus::Result<zbus::Connection> {
        match self.scope {
            ServiceScope::User => zbus::Connection::session().await,
            ServiceScope::System => zbus::Connection::system().await,
        }
    }
}

#[zbus::proxy(
    interface = "org.freedesktop.systemd1.Manager",
    default_service = "org.freedesktop.systemd1",
    default_path = "/org/freedesktop/systemd1"
)]
trait SystemdManager {
    async fn get_unit(&self, name: &str) -> zbus::Result<zbus::zvariant::OwnedObjectPath>;

    async fn load_unit(&self, name: &str) -> zbus::Result<zbus::zvariant::OwnedObjectPath>;

    async fn start_unit(
        &self,
        name: &str,
        mode: &str,
    ) -> zbus::Result<zbus::zvariant::OwnedObjectPath>;

    async fn stop_unit(
        &self,
        name: &str,
        mode: &str,
    ) -> zbus::Result<zbus::zvariant::OwnedObjectPath>;

    async fn reload(&self) -> zbus::Result<()>;

    async fn restart_unit(
        &self,
        name: &str,
        mode: &str,
    ) -> zbus::Result<zbus::zvariant::OwnedObjectPath>;
}

#[zbus::proxy(
    interface = "org.freedesktop.systemd1.Unit",
    default_service = "org.freedesktop.systemd1"
)]
trait SystemdUnit {
    #[zbus(property)]
    fn active_state(&self) -> zbus::Result<String>;

    #[zbus(property)]
    fn load_state(&self) -> zbus::Result<String>;

    #[zbus(property)]
    fn drop_in_paths(&self) -> zbus::Result<Vec<String>>;
}

#[zbus::proxy(
    interface = "org.freedesktop.systemd1.Service",
    default_service = "org.freedesktop.systemd1"
)]
trait SystemdService {
    #[zbus(property)]
    fn user(&self) -> zbus::Result<String>;
}

#[derive(Debug)]
struct SystemdServiceConfiguration {
    load_state: String,
    drop_in_paths: Vec<String>,
    managed: bool,
}

async fn service_configuration(
    connection: &zbus::Connection,
) -> Option<SystemdServiceConfiguration> {
    let manager = match SystemdManagerProxy::new(connection).await {
        Ok(manager) => manager,
        Err(error) => {
            tracing::error!(%error, "failed to create systemd manager proxy");
            return None;
        }
    };

    let unit_path = match manager.load_unit(SERVICE).await {
        Ok(path) => path,
        Err(error) => {
            println!("systemd LoadUnit error: {error}");
            return None;
        }
    };

    let builder = match SystemdUnitProxy::builder(connection).path(unit_path) {
        Ok(builder) => builder,
        Err(error) => {
            tracing::error!(%error, "failed to create systemd unit proxy builder");
            return None;
        }
    };

    let unit = match builder.build().await {
        Ok(unit) => unit,
        Err(error) => {
            tracing::error!(%error, "failed to build systemd unit proxy");
            return None;
        }
    };

    let load_state = match unit.load_state().await {
        Ok(state) => state,
        Err(error) => {
            tracing::error!(%error, "failed to query systemd unit load state");
            return None;
        }
    };

    let drop_in_paths = match unit.drop_in_paths().await {
        Ok(paths) => paths,
        Err(error) => {
            tracing::error!(%error, "failed to query systemd unit drop-in paths");
            return None;
        }
    };

    let managed = drop_in_paths.iter().any(|path| {
        std::fs::read_to_string(path)
            .map(|contents| is_managed_configuration(&contents))
            .unwrap_or(false)
    });

    Some(SystemdServiceConfiguration {
        load_state,
        drop_in_paths,
        managed,
    })
}

fn is_managed_configuration(contents: &str) -> bool {
    contents.contains(MANAGEMENT_MARKER)
}

async fn reload_systemd(connection: &zbus::Connection) -> zbus::Result<()> {
    let manager = SystemdManagerProxy::new(connection).await?;
    manager.reload().await
}

async fn setup_system_configuration(
    connection: &zbus::Connection,
    username: &str,
) -> Result<(), String> {
    let proxy = zbus::Proxy::new(
        connection,
        "io.github.cosmic.Transmission.Helper",
        "/io/github/cosmic/Transmission/Helper",
        "io.github.cosmic.Transmission.Helper1",
    )
    .await
    .map_err(|error| error.to_string())?;

    let flags = zbus::proxy::MethodFlags::AllowInteractiveAuth.into();

    let _: Option<()> = proxy
        .call_with_flags("SetupSystem", flags, &(username,))
        .await
        .map_err(|error| error.to_string())?;

    Ok(())
}

async fn update_system_configuration(
    connection: &zbus::Connection,
    username: &str,
    rpc_port: u16,
    rpc_username: &str,
) -> Result<(), String> {
    let proxy = zbus::Proxy::new(
        connection,
        "io.github.cosmic.Transmission.Helper",
        "/io/github/cosmic/Transmission/Helper",
        "io.github.cosmic.Transmission.Helper1",
    )
    .await
    .map_err(|error| error.to_string())?;

    let flags = zbus::proxy::MethodFlags::AllowInteractiveAuth.into();

    let _: Option<()> = proxy
        .call_with_flags(
            "UpdateSystemSettings",
            flags,
            &(username, rpc_port, rpc_username),
        )
        .await
        .map_err(|error| error.to_string())?;

    Ok(())
}

async fn service_status(connection: &zbus::Connection) -> ServiceState {
    let Ok(manager) = SystemdManagerProxy::new(connection).await else {
        tracing::error!("failed to create systemd manager proxy");
        return ServiceState::Error;
    };

    let Ok(unit_path) = manager.get_unit(SERVICE).await else {
        return ServiceState::Stopped;
    };

    let Ok(builder) = SystemdUnitProxy::builder(connection).path(unit_path) else {
        tracing::error!("failed to create systemd unit proxy builder");
        return ServiceState::Error;
    };

    let Ok(unit) = builder.build().await else {
        tracing::error!("failed to build systemd unit proxy");
        return ServiceState::Error;
    };

    match unit.active_state().await {
        Ok(state) => service_state_from_active_state(&state),
        Err(error) => {
            tracing::error!(%error, "failed to query systemd service state");
            ServiceState::Error
        }
    }
}

fn managed_configuration(scope: ServiceScope, username: Option<&str>) -> String {
    let mut configuration = String::from("[Service]\n");
    configuration.push_str(MANAGEMENT_MARKER);
    configuration.push('\n');

    if matches!(scope, ServiceScope::System) {
        if let Some(username) = username {
            configuration.push_str("User=");
            configuration.push_str(username);
            configuration.push('\n');
        }
    }

    configuration
}

fn managed_configuration_path(scope: ServiceScope) -> Option<std::path::PathBuf> {
    match scope {
        ServiceScope::User => {
            let config_dir = std::env::var_os("XDG_CONFIG_HOME")
                .map(std::path::PathBuf::from)
                .or_else(|| {
                    std::env::var_os("HOME")
                        .map(|home| std::path::PathBuf::from(home).join(".config"))
                })?;

            Some(
                config_dir
                    .join("systemd")
                    .join("user")
                    .join("transmission-daemon.service.d")
                    .join("override.conf"),
            )
        }
        ServiceScope::System => Some(
            std::path::PathBuf::from("/etc/systemd/system")
                .join("transmission-daemon.service.d")
                .join("override.conf"),
        ),
    }
}

fn write_managed_configuration(scope: ServiceScope, username: Option<&str>) -> std::io::Result<()> {
    let path = managed_configuration_path(scope).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "could not determine systemd configuration path",
        )
    })?;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    std::fs::write(path, managed_configuration(scope, username))
}

async fn service_username(connection: &zbus::Connection) -> Option<String> {
    let manager = match SystemdManagerProxy::new(connection).await {
        Ok(manager) => manager,
        Err(_error) => {
            return None;
        }
    };

    let unit_path = match manager.load_unit(SERVICE).await {
        Ok(path) => path,
        Err(_error) => {
            return None;
        }
    };

    let builder = match SystemdServiceProxy::builder(connection).path(unit_path) {
        Ok(builder) => builder,
        Err(_error) => {
            return None;
        }
    };

    let service = match builder.build().await {
        Ok(service) => service,
        Err(_error) => {
            return None;
        }
    };

    match service.user().await {
        Ok(username) => {
            if username.is_empty() {
                None
            } else {
                Some(username)
            }
        }
        Err(_error) => None,
    }
}

fn transmission_settings_path(username: Option<&str>) -> Option<std::path::PathBuf> {
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from)?;

    if username.is_some() {
        tracing::warn!(
            username = username.unwrap_or_default(),
            "ignoring service username when resolving user Transmission settings"
        );
    }

    Some(
        home.join(".config")
            .join("transmission-daemon")
            .join("settings.json"),
    )
}

fn update_transmission_settings(
    path: &std::path::Path,
    rpc_port: u16,
    rpc_username: &str,
) -> Result<(), String> {
    let contents = std::fs::read_to_string(path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;

    let mut settings: serde_json::Value = serde_json::from_str(&contents)
        .map_err(|error| format!("failed to parse {}: {error}", path.display()))?;

    let object = settings
        .as_object_mut()
        .ok_or_else(|| "Transmission settings.json does not contain a JSON object".to_string())?;

    object.insert("rpc-port".to_string(), serde_json::Value::from(rpc_port));
    object.insert(
        "rpc-username".to_string(),
        serde_json::Value::from(rpc_username),
    );

    let output = serde_json::to_string_pretty(&settings)
        .map_err(|error| format!("failed to serialize Transmission settings: {error}"))?;

    std::fs::write(path, format!("{output}\n"))
        .map_err(|error| format!("failed to write {}: {error}", path.display()))
}

fn service_state_from_active_state(state: &str) -> ServiceState {
    match state {
        "active" => ServiceState::Running,
        "activating" | "deactivating" | "reloading" => ServiceState::Checking,
        "inactive" | "failed" => ServiceState::Stopped,
        other => {
            tracing::warn!(active_state = other, "unknown systemd service state");
            ServiceState::Error
        }
    }
}

async fn service_action(connection: &zbus::Connection, action: ServiceAction) -> ServiceState {
    let Ok(manager) = SystemdManagerProxy::new(connection).await else {
        tracing::error!(?action, "failed to create systemd manager proxy");
        return ServiceState::Error;
    };

    let result = match action {
        ServiceAction::Start => manager.start_unit(SERVICE, "replace").await,
        ServiceAction::Stop => manager.stop_unit(SERVICE, "replace").await,
        ServiceAction::Restart => manager.restart_unit(SERVICE, "replace").await,
    };

    if let Err(error) = result {
        tracing::error!(?action, %error, "systemd service action failed");
        return ServiceState::Error;
    }

    for _ in 0..50 {
        let state = service_status(connection).await;

        if !matches!(state, ServiceState::Checking) {
            return state;
        }

        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }

    tracing::error!(?action, "timed out waiting for systemd service action");
    ServiceState::Error
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_state_from_active_state_returns_running() {
        assert_eq!(
            service_state_from_active_state("active"),
            ServiceState::Running
        );
    }

    #[test]
    fn service_state_from_active_state_returns_checking() {
        for state in ["activating", "deactivating", "reloading"] {
            assert_eq!(
                service_state_from_active_state(state),
                ServiceState::Checking
            );
        }
    }

    #[test]
    fn service_state_from_active_state_returns_stopped() {
        for state in ["inactive", "failed"] {
            assert_eq!(
                service_state_from_active_state(state),
                ServiceState::Stopped
            );
        }
    }

    #[test]
    fn service_state_from_active_state_returns_error_for_unknown_state() {
        assert_eq!(
            service_state_from_active_state("unknown"),
            ServiceState::Error
        );
    }

    #[test]
    fn management_marker_is_detected() {
        let contents = format!("[Service]\n{MANAGEMENT_MARKER}\nUser=anon\n");

        assert!(is_managed_configuration(&contents));
    }

    #[test]
    fn external_configuration_is_not_marked_as_managed() {
        let contents = "[Service]\nUser=anon\n";

        assert!(!is_managed_configuration(contents));
    }

    #[test]
    fn managed_user_configuration_path_uses_xdg_config_home() {
        let path = managed_configuration_path(ServiceScope::User);

        if std::env::var_os("XDG_CONFIG_HOME").is_some() {
            assert!(path.is_some());
        }
    }

    #[test]
    fn managed_system_configuration_path_is_expected() {
        assert_eq!(
            managed_configuration_path(ServiceScope::System),
            Some(std::path::PathBuf::from(
                "/etc/systemd/system/transmission-daemon.service.d/override.conf"
            ))
        );
    }

    #[test]
    fn managed_system_configuration_contains_username() {
        let contents = managed_configuration(ServiceScope::System, Some("anon"));

        assert_eq!(
            contents,
            "[Service]\n# Managed by cosmic-transmission\nUser=anon\n"
        );
    }

    #[test]
    fn managed_user_configuration_does_not_contain_username() {
        let contents = managed_configuration(ServiceScope::User, Some("anon"));

        assert_eq!(contents, "[Service]\n# Managed by cosmic-transmission\n");
    }

    #[test]
    fn transmission_settings_are_updated() {
        let path = std::env::temp_dir().join(format!(
            "cosmic-transmission-test-{}.json",
            uuid::Uuid::new_v4()
        ));

        let input = r#"{
  "rpc-enabled": true,
  "rpc-port": 9091,
  "rpc-username": "",
  "download-dir": "/home/anon/Downloads"
}"#;

        std::fs::write(&path, input).unwrap();

        update_transmission_settings(&path, 12345, "test-user").unwrap();

        let contents = std::fs::read_to_string(&path).unwrap();
        let settings: serde_json::Value = serde_json::from_str(&contents).unwrap();

        assert_eq!(settings["rpc-port"], 12345);
        assert_eq!(settings["rpc-username"], "test-user");
        assert_eq!(settings["rpc-enabled"], true);
        assert_eq!(settings["download-dir"], "/home/anon/Downloads");

        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn transmission_settings_reject_invalid_json() {
        let path = std::env::temp_dir().join(format!(
            "cosmic-transmission-test-{}.json",
            uuid::Uuid::new_v4()
        ));

        std::fs::write(&path, "{ invalid json").unwrap();

        let result = update_transmission_settings(&path, 12345, "test-user");

        assert!(result.is_err());

        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn transmission_settings_reject_non_object_json() {
        let path = std::env::temp_dir().join(format!(
            "cosmic-transmission-test-{}.json",
            uuid::Uuid::new_v4()
        ));

        std::fs::write(&path, "[]").unwrap();

        let result = update_transmission_settings(&path, 12345, "test-user");

        assert!(result.is_err());

        std::fs::remove_file(path).unwrap();
    }
}
