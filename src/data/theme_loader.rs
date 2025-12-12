use crate::ui::theme::{detect_color_capability, Theme, ThemeName, ThemePalette};
use anyhow::Result;
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

pub struct ThemeLoader;

#[derive(Debug, Deserialize)]
struct ThemeToml {
    name: String,
    text: String,
    subtext: String,
    base: String,
    surface: String,
    accent: String,
    accent2: String,
    accent3: String,
}

impl ThemeLoader {
    pub fn load(name: &str) -> Result<Theme> {
        let name = ThemeName::from_str_or_system(name);

        let rel = match name {
            ThemeName::System => PathBuf::from("themes/system.toml"),
            ThemeName::Latte => PathBuf::from("themes/catppuccin_latte.toml"),
            ThemeName::Frappe => PathBuf::from("themes/catppuccin_frappe.toml"),
            ThemeName::Macchiato => PathBuf::from("themes/catppuccin_macchiato.toml"),
            ThemeName::Mocha => PathBuf::from("themes/catppuccin_mocha.toml"),
        };

        let path = resolve_asset_path(&rel).unwrap_or(rel);
        let raw = fs::read_to_string(path)?;
        let t: ThemeToml = toml::from_str(&raw)?;
        let capability = detect_color_capability();
        Ok(Theme {
            name,
            palette: ThemePalette {
                text: parse_hex(&t.text),
                subtext: parse_hex(&t.subtext),
                base: parse_hex(&t.base),
                surface: parse_hex(&t.surface),
                accent: parse_hex(&t.accent),
                accent2: parse_hex(&t.accent2),
                accent3: parse_hex(&t.accent3),
            },
            capability,
        })
    }
}

fn parse_hex(s: &str) -> (u8, u8, u8) {
    let s = s.trim_start_matches('#');
    let r = u8::from_str_radix(&s[0..2], 16).unwrap_or(255);
    let g = u8::from_str_radix(&s[2..4], 16).unwrap_or(255);
    let b = u8::from_str_radix(&s[4..6], 16).unwrap_or(255);
    (r, g, b)
}

fn resolve_asset_path(rel: &Path) -> Option<PathBuf> {
    // Resolution order:
    // 1) CLI_MUSIC_PLAYER_ASSET_DIR/<rel>
    // 2) executable dir (and a few parent dirs) + <rel>
    // 3) current working directory + <rel>
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
