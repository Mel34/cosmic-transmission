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
        Ok(state) => match state.as_str() {
            "active" => ServiceState::Running,
            "activating" | "deactivating" | "reloading" => ServiceState::Checking,
            "inactive" | "failed" => ServiceState::Stopped,
            other => {
                tracing::warn!(active_state = other, "unknown systemd service state");
                ServiceState::Error
            }
        },
        Err(error) => {
            tracing::error!(%error, "failed to query systemd service state");
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

    match result {
        Ok(_) => service_status(connection).await,
        Err(error) => {
            tracing::error!(?action, %error, "systemd service action failed");
            ServiceState::Error
        }
    }
}
