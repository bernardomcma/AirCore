CREATE TABLE device_settings (
    mac_address TEXT NOT NULL,
    setting_key TEXT NOT NULL,
    value TEXT NOT NULL,
    updated_at INTEGER NOT NULL DEFAULT (strftime('%s')),
    PRIMARY KEY (mac_address, setting_key)
) STRICT;
