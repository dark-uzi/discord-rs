use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct SessionFile {
    token: String,
}

fn session_path() -> PathBuf {
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join("discord-rs").join("session.json")
}

/// Load a previously saved token, if any.
pub fn load_token() -> Option<String> {
    let data = std::fs::read_to_string(session_path()).ok()?;
    let file: SessionFile = serde_json::from_str(&data).ok()?;
    if file.token.trim().is_empty() {
        None
    } else {
        Some(file.token)
    }
}

/// Persist the token so the user stays logged in across restarts.
pub fn save_token(token: &str) {
    let path = session_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let file = SessionFile {
        token: token.to_string(),
    };
    if let Ok(data) = serde_json::to_string_pretty(&file) {
        let _ = std::fs::write(path, data);
    }
}

pub fn clear_token() {
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
}

fn default_true() -> bool {
    true
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
        }
    }
}

fn settings_path() -> PathBuf {
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join("discord-rs").join("settings.json")
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

fn aliases_path() -> PathBuf {
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join("discord-rs").join("aliases.json")
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
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join("discord-rs").join("pins.json")
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
