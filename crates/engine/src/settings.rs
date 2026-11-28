//! What belongs to this computer only: where the library is, accounts, volume and looks.
//! Passwords and tokens are not here but in the credential store.

use crate::queue::Repeat;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerAccount {
    pub name: String,
    pub url: String,
    pub user: String,
}

impl ServerAccount {
    /// The name of its password in the credential store.
    pub fn secret(&self) -> String {
        format!("server:{}:{}", self.url, self.user)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// This computer's id among those that share the library.
    pub device: String,
    /// The library folder; nothing until the person has chosen one.
    pub library: Option<PathBuf>,
    /// Two-letter code; empty follows the system.
    pub language: String,
    pub theme: Theme,
    /// 0 to 100.
    pub volume: u8,
    pub shuffle: bool,
    pub repeat: Repeat,
    pub servers: Vec<ServerAccount>,
    pub spotify_client_id: String,
    pub spotify_device: String,
    pub soloist_path: String,
    pub column_browser: bool,
    /// Songs are played louder or softer, as their tags say, so that they sound equally loud.
    pub even_volume: bool,
    /// The key of the streaming service songs without a file play from; empty for none chosen.
    pub active_service: String,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            device: uuid::Uuid::new_v4().simple().to_string(),
            library: None,
            language: String::new(),
            theme: Theme::System,
            volume: 70,
            shuffle: false,
            repeat: Repeat::Off,
            servers: Vec::new(),
            spotify_client_id: String::new(),
            spotify_device: String::new(),
            soloist_path: String::new(),
            column_browser: false,
            even_volume: false,
            active_service: String::new(),
        }
    }
}

/// This computer's data folder, outside the shared library.
pub fn data_dir() -> Option<PathBuf> {
    dirs::data_local_dir().map(|d| d.join("MusicCraft"))
}

impl Settings {
    /// Reads the settings in `dir`. Unreadable settings are kept aside and the defaults used.
    pub fn load(dir: &Path) -> Settings {
        let path = dir.join("settings.json");
        let Ok(bytes) = std::fs::read(&path) else { return Settings::default() };
        match serde_json::from_slice::<Settings>(&bytes) {
            Ok(s) if is_device_id(&s.device) => s,
            _ => {
                // Best effort: the damaged file only matters to someone who wants to look at it.
                std::fs::rename(&path, dir.join("settings.damaged.json")).ok();
                Settings::default()
            }
        }
    }

    pub fn save(&self, dir: &Path) -> Result<(), String> {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        let bytes = serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?;
        craft_atomic::atomic_write(&dir.join("settings.json"), &bytes).map_err(|e| e.to_string())
    }
}

/// The id is part of file names.
fn is_device_id(s: &str) -> bool {
    (8..=64).contains(&s.len()) && s.chars().all(|c| c.is_ascii_alphanumeric())
}

/// The name other computers show for this one.
pub fn device_name() -> String {
    ["COMPUTERNAME", "HOSTNAME"]
        .iter()
        .find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty()))
        .unwrap_or_else(|| "this computer".to_owned())
}
