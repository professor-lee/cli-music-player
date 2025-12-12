mod app;
mod audio;
mod data;
mod playback;
mod render;
mod ui;
mod utils;

use anyhow::Result;

fn main() -> Result<()> {
    env_logger::init();

    let config = data::config::Config::load_or_default()?;
    let theme = data::theme_loader::ThemeLoader::load(&config.theme)?;

    let mut app = app::state::AppState::new(config, theme);
    app::event_loop::run(&mut app)
}
