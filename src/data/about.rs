use serde::Deserialize;
use std::collections::BTreeMap;
use std::sync::OnceLock;

#[derive(Debug, Clone, Deserialize)]
pub struct AboutInfo {
    pub description: String,
    pub version: String,
    #[serde(default)]
    pub links: BTreeMap<String, String>,
}

pub fn about_info() -> &'static AboutInfo {
    static INFO: OnceLock<AboutInfo> = OnceLock::new();
    INFO.get_or_init(|| {
        let raw = include_str!("../../about/about.toml");
        toml::from_str(raw).unwrap_or_else(|_| AboutInfo {
            description: String::new(),
            version: String::new(),
            links: BTreeMap::new(),
        })
    })
}

pub fn about_image_bytes() -> &'static [u8] {
    include_bytes!("../../about/about.png")
}
