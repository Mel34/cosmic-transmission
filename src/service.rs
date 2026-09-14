use tokio::process::Command;

use crate::config::ServiceScope;

const SERVICE: &str = "transmission-daemon.service";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ServiceState {
    Running,
    Checking,
    Stopped,
    Error,
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
        match self.scope {
            ServiceScope::User => user_service_status().await,
            ServiceScope::System => system_service_status().await,
        }
    }

    pub async fn action(self, action: &'static str) -> ServiceState {
        match self.scope {
            ServiceScope::User => user_service_action(action).await,
            ServiceScope::System => system_service_action(action).await,
        }
    }
}

async fn user_service_status() -> ServiceState {
    let output = Command::new("systemctl")
        .args(["--user", "is-active", SERVICE])
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
        Ok(output) => match String::from_utf8_lossy(&output.stdout).trim() {
            "inactive" | "failed" => ServiceState::Stopped,
            "activating" | "deactivating" => ServiceState::Checking,
            _ => ServiceState::Error,
        },
        Err(_) => ServiceState::Error,      
    }
}

async fn user_service_action(action: &'static str) -> ServiceState {
    let result = Command::new("systemctl")
        .args(["--user", action, SERVICE])
        .status()
        .await;

    match result {
        Ok(status) if status.success() => user_service_status().await,
        _ => ServiceState::Error,
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
}
#[zbus::proxy(
    interface = "org.freedesktop.systemd1.Unit",
    default_service = "org.freedesktop.systemd1"
)]
trait SystemdUnit {
    #[zbus(property)]
    fn active_state(&self) -> zbus::Result<String>;
}

async fn system_service_status() -> ServiceState {
    let Ok(connection) = zbus::Connection::system().await else {
        return ServiceState::Error;
    };

    let manager = SystemdManagerProxy::new(&connection).await;

    let Ok(manager) = manager else {
        return ServiceState::Error;
    };

    let Ok(unit_path) = manager.get_unit(SERVICE).await else {
        return ServiceState::Stopped;
    };

	let Ok(builder) = SystemdUnitProxy::builder(&connection)
	    .path(unit_path)
	else {
	    return ServiceState::Error;
	};

	let Ok(unit) = builder.build().await else {
	    return ServiceState::Error;
	};

    match unit.active_state().await.as_deref() {
        Ok("active") => ServiceState::Running,
        Ok("activating" | "deactivating" | "reloading") => {
            ServiceState::Checking
        }
        Ok("inactive" | "failed") => ServiceState::Stopped,
        _ => ServiceState::Error,
    }
}

async fn system_service_action(action: &'static str) -> ServiceState {
    let Ok(connection) = zbus::Connection::system().await else {
        return ServiceState::Error;
    };

    let Ok(manager) = SystemdManagerProxy::new(&connection).await else {
        return ServiceState::Error;
    };

    let result = match action {
        "start" => manager.start_unit(SERVICE, "replace").await,
        "stop" => manager.stop_unit(SERVICE, "replace").await,
        _ => return ServiceState::Error,
    };

    match result {
        Ok(_) => system_service_status().await,
        Err(_) => ServiceState::Error,
    }
}
