use std::collections::HashMap;

use macaddr::MacAddr6;
use rusqlite::Connection;
use tracing::instrument;

use super::{Error, type_conversions::SqliteMacAddr6};

#[instrument(skip(connection))]
pub fn fetch(
    connection: &Connection,
    mac_address: MacAddr6,
) -> Result<HashMap<String, String>, Error> {
    let mut statement = connection
        .prepare_cached("SELECT setting_key, value FROM device_settings WHERE mac_address = ?1")?;
    let rows = statement
        .query_map([SqliteMacAddr6(mac_address)], |row| {
            let key: String = row.get("setting_key")?;
            let value: String = row.get("value")?;
            Ok((key, value))
        })?
        .collect::<Result<HashMap<_, _>, _>>()?;
    Ok(rows)
}

#[instrument(skip(connection))]
pub fn upsert(
    connection: &Connection,
    mac_address: MacAddr6,
    setting_key: String,
    value: String,
) -> Result<(), Error> {
    connection.execute(
        r#"INSERT INTO device_settings (mac_address, setting_key, value)
                    VALUES (?1, ?2, ?3)
                ON CONFLICT(mac_address, setting_key) DO UPDATE SET
                    value = excluded.value,
                    updated_at = strftime('%s')"#,
        (SqliteMacAddr6(mac_address), setting_key, value),
    )?;
    Ok(())
}

#[instrument(skip(connection))]
pub fn delete_for_device(connection: &Connection, mac_address: MacAddr6) -> Result<(), Error> {
    connection.execute(
        "DELETE FROM device_settings WHERE mac_address = ?1",
        (SqliteMacAddr6(mac_address),),
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use macaddr::MacAddr6;

    use crate::{
        devices::DeviceModel,
        storage::{OpenSCQ30Database, PairedDevice},
    };

    #[tokio::test]
    async fn upsert_fetch_and_delete() {
        let db = OpenSCQ30Database::new_in_memory().await.unwrap();
        let mac = MacAddr6::from_str("AA:BB:CC:DD:EE:FF").unwrap();
        let other_mac = MacAddr6::from_str("11:22:33:44:55:66").unwrap();

        // Works without paired_device row (no FK constraint)
        db.upsert_device_setting(mac, "allow_off".to_owned(), "true".to_owned())
            .await
            .unwrap();
        db.upsert_device_setting(mac, "mic_side".to_owned(), "Automatic".to_owned())
            .await
            .unwrap();
        db.upsert_device_setting(other_mac, "allow_off".to_owned(), "false".to_owned())
            .await
            .unwrap();

        // Overwrite existing key
        db.upsert_device_setting(mac, "allow_off".to_owned(), "false".to_owned())
            .await
            .unwrap();

        let settings = db.fetch_device_settings(mac).await.unwrap();
        assert_eq!(settings.len(), 2);
        assert_eq!(settings.get("allow_off").map(String::as_str), Some("false"));
        assert_eq!(
            settings.get("mic_side").map(String::as_str),
            Some("Automatic")
        );

        db.delete_device_settings(mac).await.unwrap();
        assert!(db.fetch_device_settings(mac).await.unwrap().is_empty());
        assert_eq!(db.fetch_device_settings(other_mac).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn unpair_cleans_up_device_settings() {
        let db = OpenSCQ30Database::new_in_memory().await.unwrap();
        let mac = MacAddr6::from_str("AA:BB:CC:DD:EE:FF").unwrap();

        db.upsert_paired_device(PairedDevice {
            mac_address: mac,
            model: DeviceModel::SoundcoreA3028,
            is_demo: false,
        })
        .await
        .unwrap();
        db.upsert_device_setting(mac, "ear_detection".to_owned(), "true".to_owned())
            .await
            .unwrap();

        db.delete_paired_device(mac).await.unwrap();
        assert!(db.fetch_device_settings(mac).await.unwrap().is_empty());
    }
}
