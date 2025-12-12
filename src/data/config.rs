use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

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
            ui_fps: 60,
            spectrum_hz: 30,
            mpris_poll_ms: 100,
            transparent_background: false,
            album_border: default_album_border(),
        }
    }
}

impl Config {
    pub fn load_or_default() -> Result<Self> {
        let path = Self::default_path();
        if !path.exists() {
            return Ok(Self::default());
        }
        let raw = fs::read_to_string(path)?;
        Ok(toml::from_str(&raw).unwrap_or_default())
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::default_path();
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let raw = toml::to_string_pretty(self).unwrap_or_default();
        fs::write(path, raw)?;
        Ok(())
    }

    fn default_path() -> PathBuf {
        let rel = PathBuf::from("config/default.toml");
        resolve_asset_path(&rel).unwrap_or(rel)
    }
}

fn resolve_asset_path(rel: &Path) -> Option<PathBuf> {
    if let Ok(base) = std::env::var("CLI_MUSIC_PLAYER_ASSET_DIR") {
        let p = PathBuf::from(base).join(rel);
        if p.exists() {
            return Some(p);
        }
    }

    if let Ok(exe) = std::env::current_exe() {
        let mut cur = exe.parent();
        for _ in 0..6 {
            let Some(dir) = cur else { break };
            let p = dir.join(rel);
            if p.exists() {
                return Some(p);
            }
            cur = dir.parent();
        }
    }

    if let Ok(cwd) = std::env::current_dir() {
        let p = cwd.join(rel);
        if p.exists() {
            return Some(p);
        }
    }

    None
}
