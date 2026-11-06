use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct SessionFile {
    token: String,
}

fn app_dir() -> PathBuf {
    if let Some(appdata) = std::env::var_os("APPDATA") {
        PathBuf::from(appdata).join("discord-rs")
    } else if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
        PathBuf::from(xdg).join("discord-rs")
    } else if let Some(home) = std::env::var_os("HOME") {
        PathBuf::from(home).join(".config").join("discord-rs")
    } else {
        std::env::temp_dir().join("discord-rs")
    }
}

fn session_path() -> PathBuf {
    app_dir().join("session.json")
}

/// Service name used for the token in the OS credential store (Windows
/// Credential Manager). Keeps the session token out of plaintext files.
#[cfg(target_os = "windows")]
const KEYRING_SERVICE: &str = "discord-rs";

#[cfg(target_os = "windows")]
fn keyring_entry() -> Option<keyring::Entry> {
    keyring::Entry::new(KEYRING_SERVICE, "session-token").ok()
}

/// Load a previously saved token, if any. Reads from the OS credential store;
/// on a miss the legacy plaintext `session.json` is used and migrated.
pub fn load_token() -> Option<String> {
    #[cfg(target_os = "windows")]
    if let Some(entry) = keyring_entry() {
        if let Ok(tok) = entry.get_password() {
            if !tok.trim().is_empty() {
                let _ = std::fs::remove_file(session_path());
                return Some(tok);
            }
        }
    }
    let data = std::fs::read_to_string(session_path()).ok()?;
    let file: SessionFile = serde_json::from_str(&data).ok()?;
    if file.token.trim().is_empty() {
        None
    } else {
        save_token(&file.token);
        Some(file.token)
    }
}

/// Persist the token in the OS credential store so the user stays logged in
/// across restarts.
pub fn save_token(token: &str) {
    #[cfg(target_os = "windows")]
    if let Some(entry) = keyring_entry() {
        if entry.set_password(token).is_ok() {
            let _ = std::fs::remove_file(session_path());
            return;
        }
    }
    let path = session_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
        }
    }
    if let Ok(data) = serde_json::to_string(&SessionFile {
        token: token.to_string(),
    }) {
        // Security: Restrict session token file to owner-only read/write (0600 on Unix)
        // to prevent sensitive authentication tokens from leaking to other local users.
        #[cfg(unix)]
        {
            use std::io::Write;
            use std::os::unix::fs::OpenOptionsExt;
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .mode(0o600)
                .open(&path)
            {
                let _ = f.write_all(data.as_bytes());
                return;
            }
        }
        let _ = std::fs::write(path, data);
    }
}

pub fn clear_token() {
    #[cfg(target_os = "windows")]
    if let Some(entry) = keyring_entry() {
        let _ = entry.delete_credential();
    }
    let _ = std::fs::remove_file(session_path());
}

/// User-adjustable settings, persisted to disk.
#[derive(Serialize, Deserialize, Clone)]
pub struct Settings {
    #[serde(default)]
    pub show_avatars: bool,
    #[serde(default)]
    pub compact: bool,
    #[serde(default = "default_true")]
    pub time_24h: bool,
    #[serde(default = "default_true")]
    pub show_member_list: bool,
    #[serde(default = "default_true")]
    pub load_images: bool,
    #[serde(default)]
    pub use_names: bool,
    #[serde(default = "default_true")]
    pub use_custom_names: bool,
    #[serde(default = "default_true")]
    pub dm_avatars: bool,
    #[serde(default = "default_true")]
    pub theme_dark: bool,
    #[serde(default)]
    pub recent_emojis: Vec<String>,
    #[serde(default)]
    pub moderation: bool,
    #[serde(default)]
    pub message_log: bool,
    #[serde(default = "default_true")]
    pub radial_menu: bool,
    #[serde(default = "default_true")]
    pub notifications: bool,
    #[serde(default = "default_true")]
    pub notification_sound: bool,
    #[serde(default)]
    pub performance_mode: bool,
    #[serde(default = "default_scroll_speed")]
    pub scroll_speed: f32,
}

fn default_true() -> bool {
    true
}

fn default_scroll_speed() -> f32 {
    2.0
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            show_avatars: true,
            compact: false,
            time_24h: true,
            show_member_list: true,
            load_images: true,
            use_names: false,
            use_custom_names: true,
            dm_avatars: true,
            theme_dark: true,
            recent_emojis: Vec::new(),
            moderation: false,
            message_log: false,
            radial_menu: true,
            notifications: true,
            notification_sound: true,
            performance_mode: false,
            scroll_speed: 2.0,
        }
    }
}

fn settings_path() -> PathBuf {
    app_dir().join("settings.json")
}

pub fn load_settings() -> Settings {
    std::fs::read_to_string(settings_path())
        .ok()
        .and_then(|data| serde_json::from_str(&data).ok())
        .unwrap_or_default()
}

pub fn save_settings(settings: &Settings) {
    let path = settings_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(data) = serde_json::to_string_pretty(settings) {
        let _ = std::fs::write(path, data);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_dir_path_structure() {
        let dir = app_dir();
        assert!(dir.ends_with("discord-rs"));
    }

    #[cfg(unix)]
    #[test]
    fn test_save_token_unix_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let test_token = "sentinel_test_token_12345";
        save_token(test_token);

        let path = session_path();
        assert!(path.exists(), "session file should exist");

        let file_meta = std::fs::metadata(&path).expect("file metadata");
        let file_mode = file_meta.permissions().mode() & 0o777;
        assert_eq!(file_mode, 0o600, "session file should be restricted to 0600");

        if let Some(parent) = path.parent() {
            let dir_meta = std::fs::metadata(parent).expect("parent metadata");
            let dir_mode = dir_meta.permissions().mode() & 0o777;
            assert_eq!(dir_mode, 0o700, "app dir should be restricted to 0700");
        }

        clear_token();
        assert!(!path.exists(), "session file should be deleted on clear");
    }
}

fn aliases_path() -> PathBuf {
    app_dir().join("aliases.json")
}

pub fn load_aliases() -> HashMap<String, String> {
    std::fs::read_to_string(aliases_path())
        .ok()
        .and_then(|data| serde_json::from_str(&data).ok())
        .unwrap_or_default()
}

pub fn save_aliases(aliases: &HashMap<String, String>) {
    let path = aliases_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(data) = serde_json::to_string_pretty(aliases) {
        let _ = std::fs::write(path, data);
    }
}

#[derive(Serialize, Deserialize, Default)]
pub struct Pins {
    #[serde(default)]
    pub guilds: Vec<String>,
    #[serde(default)]
    pub dms: Vec<String>,
}

fn pins_path() -> PathBuf {
    app_dir().join("pins.json")
}

pub fn load_pins() -> Pins {
    std::fs::read_to_string(pins_path())
        .ok()
        .and_then(|data| serde_json::from_str(&data).ok())
        .unwrap_or_default()
}

pub fn save_pins(pins: &Pins) {
    let path = pins_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(data) = serde_json::to_string_pretty(pins) {
        let _ = std::fs::write(path, data);
    }
}
