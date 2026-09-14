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
            ServiceScope::System => ServiceState::Error,
        }
    }

    pub async fn action(self, action: &'static str) -> ServiceState {
        match self.scope {
            ServiceScope::User => user_service_action(action).await,
            ServiceScope::System => ServiceState::Error,
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
