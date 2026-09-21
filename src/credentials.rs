use secrecy::{ExposeSecret, SecretString};
use secret_service::{EncryptionType, SecretService};
use std::collections::HashMap;

const APPLICATION: &str = "io.github.cosmic.Transmission";

pub async fn get_password(connection_id: uuid::Uuid) -> Option<SecretString> {
    let service = SecretService::connect(EncryptionType::Dh).await.ok()?;

    let connection_id = connection_id.to_string();

    let items = service
        .search_items(HashMap::from([
            ("application", APPLICATION),
            ("connection-id", connection_id.as_str()),
        ]))
        .await
        .ok()?;

    let item = if let Some(item) = items.unlocked.first() {
        item
    } else {
        let item = items.locked.first()?;
        item.unlock().await.ok()?;
        item
    };

    let secret = item.get_secret().await.ok()?;

    String::from_utf8(secret)
        .ok()
        .map(|password| SecretString::new(password.into_boxed_str()))
}

pub async fn set_password(connection_id: uuid::Uuid, password: SecretString) -> bool {
    let service = match SecretService::connect(EncryptionType::Dh).await {
        Ok(service) => service,
        Err(_) => return false,
    };

    let collection = match service.get_default_collection().await {
        Ok(collection) => collection,
        Err(_) => return false,
    };

    let connection_id = connection_id.to_string();

    let properties = HashMap::from([
        ("application", APPLICATION),
        ("connection-id", connection_id.as_str()),
    ]);

    collection
        .create_item(
            "Transmission connection password",
            properties,
            password.expose_secret().as_bytes(),
            true,
            "text/plain",
        )
        .await
        .is_ok()
}

pub async fn delete_password(connection_id: uuid::Uuid) -> bool {
    let service = match SecretService::connect(EncryptionType::Dh).await {
        Ok(service) => service,
        Err(_) => return false,
    };

    let connection_id = connection_id.to_string();

    let items = match service
        .search_items(HashMap::from([
            ("application", APPLICATION),
            ("connection-id", connection_id.as_str()),
        ]))
        .await
    {
        Ok(items) => items,
        Err(_) => return false,
    };

    let mut deleted = false;

    for item in items.unlocked {
        if item.delete().await.is_ok() {
            deleted = true;
        }
    }

    for item in items.locked {
        if item.unlock().await.is_ok() && item.delete().await.is_ok() {
            deleted = true;
        }
    }

    deleted
}
