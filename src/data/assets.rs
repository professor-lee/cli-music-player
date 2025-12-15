use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

const ENV_ASSET_DIR: &str = "CLI_MUSIC_PLAYER_ASSET_DIR";

const DEFAULT_CONFIG_TOML: &str = include_str!("../../config/default.toml");

const THEME_SYSTEM_TOML: &str = include_str!("../../themes/system.toml");
const THEME_LATTE_TOML: &str = include_str!("../../themes/catppuccin_latte.toml");
const THEME_FRAPPE_TOML: &str = include_str!("../../themes/catppuccin_frappe.toml");
const THEME_MACCHIATO_TOML: &str = include_str!("../../themes/catppuccin_macchiato.toml");
const THEME_MOCHA_TOML: &str = include_str!("../../themes/catppuccin_mocha.toml");

pub fn resolve_asset_root() -> PathBuf {
    if let Some(p) = std::env::var_os(ENV_ASSET_DIR) {
        return PathBuf::from(p);
    }

    if let Some(sys) = system_config_root() {
        // 需求：优先检测系统配置文件夹；若没有文件，则改为当前目录生成 .config
        if sys.join("config/default.toml").is_file() {
            // best-effort: ensure themes exist alongside config
            let _ = ensure_themes(&sys);
            return sys;
        }
    }

    let local = local_config_root();
    let _ = ensure_all_assets(&local);
    local
}

pub fn resolve_asset_path(rel: &Path) -> PathBuf {
    resolve_asset_root().join(rel)
}

pub fn resolve_config_path() -> PathBuf {
    resolve_asset_path(Path::new("config/default.toml"))
}

pub fn ensure_assets_ready() -> Result<PathBuf> {
    if let Some(sys) = system_config_root() {
        if sys.join("config/default.toml").is_file() {
            ensure_themes(&sys)?;
            return Ok(sys);
        }
    }

    let local = local_config_root();
    ensure_all_assets(&local)?;
    Ok(local)
}

fn system_config_root() -> Option<PathBuf> {
    // Linux: $HOME/.config/cli-music-player
    #[cfg(target_os = "linux")]
    {
        let home = std::env::var_os("HOME")?;
        return Some(PathBuf::from(home).join(".config").join("cli-music-player"));
    }

    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

fn local_config_root() -> PathBuf {
    // 需求：在当前文件夹目录生成 .config（并包含运行所需文件）
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(".config")
}

fn ensure_all_assets(root: &Path) -> Result<()> {
    // Create:
    //   <root>/config/default.toml
    //   <root>/themes/*.toml
    ensure_dir(&root.join("config"))?;
    ensure_dir(&root.join("themes"))?;

    write_if_missing(&root.join("config/default.toml"), DEFAULT_CONFIG_TOML)?;
    ensure_themes(root)?;

    Ok(())
}

fn ensure_themes(root: &Path) -> Result<()> {
    ensure_dir(&root.join("themes"))?;

    write_if_missing(&root.join("themes/system.toml"), THEME_SYSTEM_TOML)?;
    write_if_missing(&root.join("themes/catppuccin_latte.toml"), THEME_LATTE_TOML)?;
    write_if_missing(&root.join("themes/catppuccin_frappe.toml"), THEME_FRAPPE_TOML)?;
    write_if_missing(
        &root.join("themes/catppuccin_macchiato.toml"),
        THEME_MACCHIATO_TOML,
    )?;
    write_if_missing(&root.join("themes/catppuccin_mocha.toml"), THEME_MOCHA_TOML)?;

    Ok(())
}

fn ensure_dir(p: &Path) -> Result<()> {
    fs::create_dir_all(p).with_context(|| format!("mkdir {}", p.display()))
}

fn write_if_missing(path: &Path, contents: &str) -> Result<()> {
    if path.is_file() {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        ensure_dir(parent)?;
    }
    fs::write(path, contents).with_context(|| format!("write {}", path.display()))
}
