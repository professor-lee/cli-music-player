use crate::data::assets;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

const DEFAULT_EQ_BANDS_DB: [f32; crate::app::state::EQ_BANDS] = [0.0; crate::app::state::EQ_BANDS];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub theme: String,
    pub ui_fps: u32,
    pub spectrum_hz: u32,
    pub mpris_poll_ms: u64,

    #[serde(default = "default_eq_bands_db")]
    pub eq_bands_db: [f32; crate::app::state::EQ_BANDS],

    #[serde(default)]
    pub transparent_background: bool,

    #[serde(default = "default_album_border")]
    pub album_border: bool,

    #[serde(default)]
    pub kitty_graphics: bool,

    #[serde(default = "default_kitty_cover_scale_percent")]
    pub kitty_cover_scale_percent: u8,
}

fn default_album_border() -> bool {
    true
}

fn default_eq_bands_db() -> [f32; crate::app::state::EQ_BANDS] {
    DEFAULT_EQ_BANDS_DB
}

fn default_kitty_cover_scale_percent() -> u8 {
    50
}

impl Default for Config {
    fn default() -> Self {
        Self {
            theme: "mocha".to_string(),
            ui_fps: 30,
            spectrum_hz: 30,
            mpris_poll_ms: 100,
            eq_bands_db: default_eq_bands_db(),
            transparent_background: false,
            album_border: default_album_border(),
            kitty_graphics: false,
            kitty_cover_scale_percent: default_kitty_cover_scale_percent(),
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
