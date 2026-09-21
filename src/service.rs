use crate::config::ServiceScope;

const SERVICE: &str = "transmission-daemon.service";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ServiceState {
    Running,
    Checking,
    Stopped,
    Error,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ServiceAction {
    Start,
    Stop,
    Restart,
}

#[derive(Clone, Copy)]
pub struct ServiceController {
    scope: ServiceScope,
}

impl ServiceController {
    pub fn new(scope: ServiceScope) -> Self {
        Self { scope }
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
}

#[zbus::proxy(
    interface = "org.freedesktop.systemd1.Service",
    default_service = "org.freedesktop.systemd1"
)]
trait SystemdService {
    #[zbus(property)]
    fn user(&self) -> zbus::Result<String>;
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

async fn service_username(connection: &zbus::Connection) -> Option<String> {
    let manager = match SystemdManagerProxy::new(connection).await {
        Ok(manager) => manager,
        Err(error) => {
            tracing::error!(%error, "failed to create systemd manager proxy");
            return None;
        }
    };

    let unit_path = match manager.get_unit(SERVICE).await {
        Ok(path) => path,
        Err(error) => {
            tracing::error!(%error, "failed to find transmission service");
            return None;
        }
    };

    let builder = match SystemdServiceProxy::builder(connection).path(unit_path) {
        Ok(builder) => builder,
        Err(error) => {
            tracing::error!(%error, "failed to create systemd service proxy builder");
            return None;
        }
    };

    let service = match builder.build().await {
        Ok(service) => service,
        Err(error) => {
            tracing::error!(%error, "failed to build systemd service proxy");
            return None;
        }
    };

    match service.user().await {
        Ok(username) if !username.is_empty() => Some(username),
        Ok(_) => {
            tracing::warn!("systemd service user is empty");
            None
        }
        Err(error) => {
            tracing::error!(%error, "failed to query systemd service user");
            None
        }
    }
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
}
