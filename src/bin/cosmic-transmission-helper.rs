use std::fs;
use std::io;
use std::path::Path;

use zbus::interface;
use zbus::message::Header;
use zbus::{Connection, fdo};
use zbus_polkit::policykit1::{AuthorityProxy, CheckAuthorizationFlags, Subject};

const BUS_NAME: &str = "io.github.cosmic.Transmission.Helper";
const OBJECT_PATH: &str = "/io/github/cosmic/Transmission/Helper";
const ACTION_ID: &str = "io.github.cosmic.Transmission.setup-system";

const OVERRIDE_PATH: &str = "/etc/systemd/system/transmission-daemon.service.d/override.conf";
const MANAGEMENT_MARKER: &str = "# Managed by cosmic-transmission";

struct Helper;

#[interface(name = "io.github.cosmic.Transmission.Helper1")]
impl Helper {
    async fn setup_system(
        &self,
        username: &str,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &Connection,
    ) -> fdo::Result<()> {
        authorize(connection, &header).await?;

        setup_system(username).map_err(|error| fdo::Error::Failed(error.to_string()))
    }
}

async fn authorize(connection: &Connection, header: &Header<'_>) -> fdo::Result<()> {
    let subject = Subject::new_for_message_header(header)
        .map_err(|error| fdo::Error::Failed(error.to_string()))?;

    let authority = AuthorityProxy::new(connection)
        .await
        .map_err(|error| fdo::Error::Failed(error.to_string()))?;

    let result = authority
        .check_authorization(
            &subject,
            ACTION_ID,
            &std::collections::HashMap::new(),
            CheckAuthorizationFlags::AllowUserInteraction.into(),
            "",
        )
        .await
        .map_err(|error| fdo::Error::Failed(error.to_string()))?;

    if !result.is_authorized {
        return Err(fdo::Error::AccessDenied(
            "system Transmission setup is not authorized".to_string(),
        ));
    }

    Ok(())
}

fn setup_system(username: &str) -> io::Result<()> {
    if username.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "username must not be empty",
        ));
    }

    if username.contains('\n') || username.contains('\r') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "username contains a newline",
        ));
    }

    let path = Path::new(OVERRIDE_PATH);

    if path.exists() {
        let existing = fs::read_to_string(path)?;

        if !existing.contains(MANAGEMENT_MARKER) {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "existing override is not managed by cosmic-transmission",
            ));
        }
    }

    let parent = path.parent().expect("override path has a parent");
    fs::create_dir_all(parent)?;

    let configuration = format!("[Service]\n{MANAGEMENT_MARKER}\nUser={username}\n");

    fs::write(path, configuration)?;

    Ok(())
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _connection = zbus::connection::Builder::system()?
        .name(BUS_NAME)?
        .serve_at(OBJECT_PATH, Helper)?
        .build()
        .await?;

    std::future::pending::<()>().await;

    Ok(())
}
