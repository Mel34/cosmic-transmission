mod systemd;

pub use systemd::SystemdServiceController;

use crate::config::ServiceScope;

#[derive(Clone, Copy)]
pub enum ServiceController {
    Systemd(SystemdServiceController),
}

impl ServiceController {
    pub fn new(scope: ServiceScope) -> Self {
        Self::Systemd(SystemdServiceController::new(scope))
    }

    pub fn capabilities(self) -> ServiceCapabilities {
        match self {
            Self::Systemd(_) => ServiceCapabilities {
                user_scope: true,
                system_scope: true,
                setup: true,
                removal: false,
            },
        }
    }

    pub async fn configuration(self) -> ServiceConfiguration {
        match self {
            Self::Systemd(controller) => controller.configuration().await,
        }
    }

    pub async fn setup(self, username: Option<String>) -> ServiceSetupResult {
        match self {
            Self::Systemd(controller) => controller.setup(username).await,
        }
    }

    pub async fn status(self) -> ServiceState {
        match self {
            Self::Systemd(controller) => controller.status().await,
        }
    }

    pub async fn action(self, action: ServiceAction) -> ServiceState {
        match self {
            Self::Systemd(controller) => controller.action(action).await,
        }
    }

    pub async fn apply_configuration(
        self,
        rpc_port: u16,
        rpc_username: &str,
    ) -> Result<(), String> {
        match self {
            Self::Systemd(controller) => {
                controller.apply_configuration(rpc_port, rpc_username).await
            }
        }
    }

    pub async fn username(self) -> Option<String> {
        match self {
            Self::Systemd(controller) => controller.username().await,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ServiceSetupResult {
    Success,
    UnmanagedConfiguration,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ServiceCapabilities {
    pub user_scope: bool,
    pub system_scope: bool,
    pub setup: bool,
    pub removal: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ServiceConfiguration {
    Configured { managed: bool },
    Unconfigured,
}

impl ServiceConfiguration {
    pub fn is_managed(self) -> bool {
        matches!(self, Self::Configured { managed: true })
    }
}

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
