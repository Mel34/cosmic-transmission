use std::fs;
use std::io;
use std::path::{Path, PathBuf};

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

    async fn update_system_settings(
        &self,
        username: &str,
        rpc_port: u16,
        rpc_username: &str,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &Connection,
    ) -> fdo::Result<()> {
        authorize(connection, &header).await?;

        update_system_settings(username, rpc_port, rpc_username)
            .map_err(|error| fdo::Error::Failed(error.to_string()))
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

fn update_system_settings(username: &str, rpc_port: u16, rpc_username: &str) -> io::Result<()> {
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

    let path = transmission_settings_path(username)?;

    update_transmission_settings(&path, rpc_port, rpc_username).map_err(io::Error::other)
}

fn transmission_settings_path(username: &str) -> io::Result<PathBuf> {
    let home = passwd_home_directory(username)?;

    Ok(home
        .join(".config")
        .join("transmission-daemon")
        .join("settings.json"))
}

fn passwd_home_directory(username: &str) -> io::Result<PathBuf> {
    let contents = fs::read_to_string("/etc/passwd")?;

    passwd_home_directory_from_contents(&contents, username).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            format!("could not find home directory for user {username}"),
        )
    })
}

fn passwd_home_directory_from_contents(contents: &str, username: &str) -> Option<PathBuf> {
    contents.lines().find_map(|line| {
        let mut fields = line.split(':');

        if fields.next()? != username {
            return None;
        }

        fields.next()?;
        fields.next()?;
        fields.next()?;
        fields.next()?;

        Some(PathBuf::from(fields.next()?))
    })
}

fn update_transmission_settings(
    path: &Path,
    rpc_port: u16,
    rpc_username: &str,
) -> Result<(), String> {
    let contents = fs::read_to_string(path)
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

    fs::write(path, format!("{output}\n"))
        .map_err(|error| format!("failed to write {}: {error}", path.display()))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transmission_settings_are_updated() {
        let path = std::env::temp_dir().join(format!(
            "cosmic-transmission-helper-test-{}.json",
            uuid::Uuid::new_v4()
        ));

        let input = r#"{
  "rpc-enabled": true,
  "rpc-port": 9091,
  "rpc-username": "",
  "download-dir": "/home/transmission/Downloads"
}"#;

        fs::write(&path, input).unwrap();

        update_transmission_settings(&path, 12345, "test-user").unwrap();

        let contents = fs::read_to_string(&path).unwrap();
        let settings: serde_json::Value = serde_json::from_str(&contents).unwrap();

        assert_eq!(settings["rpc-port"], 12345);
        assert_eq!(settings["rpc-username"], "test-user");
        assert_eq!(settings["rpc-enabled"], true);
        assert_eq!(settings["download-dir"], "/home/transmission/Downloads");

        fs::remove_file(path).unwrap();
    }

    #[test]
    fn transmission_settings_reject_invalid_json() {
        let path = std::env::temp_dir().join(format!(
            "cosmic-transmission-helper-test-{}.json",
            uuid::Uuid::new_v4()
        ));

        fs::write(&path, "{ invalid json").unwrap();

        let result = update_transmission_settings(&path, 12345, "test-user");

        assert!(result.is_err());

        fs::remove_file(path).unwrap();
    }

    #[test]
    fn transmission_settings_reject_non_object_json() {
        let path = std::env::temp_dir().join(format!(
            "cosmic-transmission-helper-test-{}.json",
            uuid::Uuid::new_v4()
        ));

        fs::write(&path, "[]").unwrap();

        let result = update_transmission_settings(&path, 12345, "test-user");

        assert!(result.is_err());

        fs::remove_file(path).unwrap();
    }

    #[test]
    fn transmission_settings_reject_missing_file() {
        let path = std::env::temp_dir().join(format!(
            "cosmic-transmission-helper-test-{}.json",
            uuid::Uuid::new_v4()
        ));

        let result = update_transmission_settings(&path, 12345, "test-user");

        assert!(result.is_err());
    }

    #[test]
    fn transmission_settings_preserve_existing_values() {
        let path = std::env::temp_dir().join(format!(
            "cosmic-transmission-helper-test-{}.json",
            uuid::Uuid::new_v4()
        ));

        let input = r#"{
  "rpc-enabled": false,
  "rpc-port": 9091,
  "rpc-username": "old-user",
  "rpc-password": "old-password",
  "download-dir": "/srv/downloads",
  "speed-limit-down": 5000,
  "speed-limit-down-enabled": true
}"#;

        fs::write(&path, input).unwrap();

        update_transmission_settings(&path, 12345, "new-user").unwrap();

        let contents = fs::read_to_string(&path).unwrap();
        let settings: serde_json::Value = serde_json::from_str(&contents).unwrap();

        assert_eq!(settings["rpc-enabled"], false);
        assert_eq!(settings["rpc-port"], 12345);
        assert_eq!(settings["rpc-username"], "new-user");
        assert_eq!(settings["rpc-password"], "old-password");
        assert_eq!(settings["download-dir"], "/srv/downloads");
        assert_eq!(settings["speed-limit-down"], 5000);
        assert_eq!(settings["speed-limit-down-enabled"], true);

        fs::remove_file(path).unwrap();
    }

    #[test]
    fn passwd_home_directory_finds_existing_user() {
        let contents = "\
root:x:0:0:root:/root:/bin/bash
transmission:x:169:169:Transmission BitTorrent Daemon:/var/lib/transmission:/usr/bin/nologin
";

        let home = passwd_home_directory_from_contents(contents, "transmission");

        assert_eq!(home, Some(PathBuf::from("/var/lib/transmission")));
    }

    #[test]
    fn passwd_home_directory_ignores_other_users() {
        let contents = "\
root:x:0:0:root:/root:/bin/bash
nobody:x:65534:65534:nobody:/nonexistent:/usr/bin/nologin
";

        let home = passwd_home_directory_from_contents(contents, "transmission");

        assert_eq!(home, None);
    }

    #[test]
    fn passwd_home_directory_returns_expected_home() {
        let contents = "\
root:x:0:0:root:/root:/bin/bash
transmission:x:169:169:Transmission BitTorrent Daemon:/var/lib/transmission:/usr/bin/nologin
";

        let home = passwd_home_directory_from_contents(contents, "transmission").unwrap();

        assert_eq!(home, PathBuf::from("/var/lib/transmission"));
    }

    #[test]
    fn passwd_home_directory_handles_missing_home_field() {
        let contents = "\
transmission:x:169:169:Transmission BitTorrent Daemon
";

        let home = passwd_home_directory_from_contents(contents, "transmission");

        assert_eq!(home, None);
    }
}
