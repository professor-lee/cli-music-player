use crate::data::assets;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub theme: String,
    pub ui_fps: u32,
    pub spectrum_hz: u32,
    pub mpris_poll_ms: u64,

    #[serde(default)]
    pub transparent_background: bool,

    #[serde(default = "default_album_border")]
    pub album_border: bool,
}

fn default_album_border() -> bool {
    true
}

impl Default for Config {
    fn default() -> Self {
        Self {
            theme: "mocha".to_string(),
            ui_fps: 30,
            spectrum_hz: 30,
            mpris_poll_ms: 100,
            transparent_background: false,
            album_border: default_album_border(),
        }
    }
}

impl Config {
    pub fn load_or_default() -> Result<Self> {
        // Ensure assets exist according to the required resolution rules.
        let _ = assets::ensure_assets_ready();
        let path = Self::default_path();
        if !path.exists() {
            return Ok(Self::default());
        }
        let raw = fs::read_to_string(path)?;
        Ok(toml::from_str(&raw).unwrap_or_default())
    }

    pub fn save(&self) -> Result<()> {
        let _ = assets::ensure_assets_ready();
        let path = Self::default_path();
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let raw = toml::to_string_pretty(self).unwrap_or_default();
        fs::write(path, raw)?;
        Ok(())
    }

    fn default_path() -> PathBuf {
        assets::resolve_config_path()
    }
}
