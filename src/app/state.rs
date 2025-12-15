use crate::data::config::Config;
use crate::data::playlist::Playlist;
use crate::render::cover_cache::CoverCache;
use crate::ui::theme::Theme;
use std::cell::RefCell;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayMode {
    Idle,
    LocalPlayback,
    SystemMonitor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackState {
    Playing,
    Paused,
    Stopped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepeatMode {
    Sequence,
    Shuffle,
    LoopAll,
    LoopOne,
}

impl RepeatMode {
    pub fn next(self) -> Self {
        match self {
            RepeatMode::Sequence => RepeatMode::Shuffle,
            RepeatMode::Shuffle => RepeatMode::LoopAll,
            RepeatMode::LoopAll => RepeatMode::LoopOne,
            RepeatMode::LoopOne => RepeatMode::Sequence,
        }
    }

    pub fn symbol(self) -> &'static str {
        match self {
            // 需求：顺序播放 ⇔，随机播放 ≠，列表循环 ∞，单曲循环 ↻
            RepeatMode::Sequence => "⇔",
            RepeatMode::Shuffle => "≠",
            RepeatMode::LoopAll => "∞",
            RepeatMode::LoopOne => "↻",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct EqSettings {
    pub low_db: f32,
    pub mid_db: f32,
    pub high_db: f32,
}

impl Default for EqSettings {
    fn default() -> Self {
        Self {
            low_db: 0.0,
            mid_db: 0.0,
            high_db: 0.0,
        }
    }
}

impl EqSettings {
    pub fn clamp(self) -> Self {
        fn c(v: f32) -> f32 {
            v.clamp(-12.0, 12.0)
        }
        Self {
            low_db: c(self.low_db),
            mid_db: c(self.mid_db),
            high_db: c(self.high_db),
        }
    }
}

#[derive(Debug, Clone)]
pub struct LyricLine {
    pub start_ms: u64,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct TrackMetadata {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration: Duration,
    pub cover: Option<Vec<u8>>,
    pub cover_hash: Option<u64>,
    pub lyrics: Option<Vec<LyricLine>>,
}

#[derive(Debug, Clone)]
pub struct CoverSnapshot {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub cover: Option<Vec<u8>>,
    pub cover_hash: Option<u64>,
}

impl From<&TrackMetadata> for CoverSnapshot {
    fn from(t: &TrackMetadata) -> Self {
        Self {
            title: t.title.clone(),
            artist: t.artist.clone(),
            album: t.album.clone(),
            cover: t.cover.clone(),
            cover_hash: t.cover_hash,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CoverAnim {
    pub from: CoverSnapshot,
    pub to: CoverSnapshot,
    // -1 => slide left (next), +1 => slide right (prev)
    pub dir: i8,
    pub started_at: Instant,
    pub duration: Duration,
}

impl Default for TrackMetadata {
    fn default() -> Self {
        Self {
            title: "Unknown".to_string(),
            artist: "Unknown".to_string(),
            album: "Unknown".to_string(),
            duration: Duration::from_secs(0),
            cover: None,
            cover_hash: None,
            lyrics: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SpectrumData {
    pub bars: [f32; 64],
    pub sample_rate: u32,
    pub fft_size: usize,
}

impl Default for SpectrumData {
    fn default() -> Self {
        Self {
            bars: [0.0; 64],
            sample_rate: 44100,
            fft_size: 2048,
        }
    }
}

#[derive(Debug)]
pub struct PlayerState {
    pub mode: PlayMode,
    pub playback: PlaybackState,
    pub position: Duration,
    pub volume: f32,
    pub repeat_mode: RepeatMode,
    pub track: TrackMetadata,
}

impl Default for PlayerState {
    fn default() -> Self {
        Self {
            mode: PlayMode::Idle,
            playback: PlaybackState::Stopped,
            position: Duration::from_secs(0),
            volume: 0.0,
            repeat_mode: RepeatMode::Sequence,
            track: TrackMetadata::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Overlay {
    None,
    Playlist,
    FolderInput,
    SettingsModal,
    HelpModal,
    EqModal,
}

#[derive(Debug)]
pub struct FolderInput {
    pub buf: String,
}

impl Default for FolderInput {
    fn default() -> Self {
        Self { buf: String::new() }
    }
}

#[derive(Debug)]
pub struct AppState {
    pub config: Config,
    pub theme: Theme,

    pub player: PlayerState,
    pub playlist: Playlist,
    pub spectrum: SpectrumData,

    pub cover_cache: RefCell<CoverCache>,

    pub overlay: Overlay,
    pub folder_input: FolderInput,

    pub settings_selected: usize,

    pub eq: EqSettings,
    pub eq_selected: usize,

    pub cover_anim: Option<CoverAnim>,
    pub pending_system_cover_anim: Option<(CoverSnapshot, i8, Instant)>,

    pub toast: Option<(String, Instant)>,

    pub last_mouse_click: Option<(Instant, u16, u16)>,


    // playlist slide animation
    pub playlist_slide_x: i16,
    pub playlist_slide_target_x: i16,

    pub last_frame: Instant,
}

impl AppState {
    pub fn new(config: Config, theme: Theme) -> Self {
        Self {
            config,
            theme,
            player: PlayerState::default(),
            playlist: Playlist::default(),
            spectrum: SpectrumData::default(),
            cover_cache: RefCell::new(CoverCache::new(20)),
            overlay: Overlay::None,
            folder_input: FolderInput::default(),
            settings_selected: 0,

            eq: EqSettings::default(),
            eq_selected: 0,

            cover_anim: None,
            pending_system_cover_anim: None,
            toast: None,
            last_mouse_click: None,
            playlist_slide_x: 0,
            playlist_slide_target_x: 0,
            last_frame: Instant::now(),
        }
    }

    pub fn set_toast(&mut self, msg: impl Into<String>) {
        self.toast = Some((msg.into(), Instant::now()));
    }

    pub fn tick(&mut self, now: Instant) {
        self.last_frame = now;

        if let Some(anim) = &self.cover_anim {
            if now.duration_since(anim.started_at) >= anim.duration {
                self.cover_anim = None;
            }
        }

        if let Some((_, _, at)) = &self.pending_system_cover_anim {
            if now.duration_since(*at) > Duration::from_secs(2) {
                self.pending_system_cover_anim = None;
            }
        }

        if let Some((_, at)) = &self.toast {
            if now.duration_since(*at) > Duration::from_millis(1500) {
                self.toast = None;
            }
        }
    }

    pub fn start_cover_anim(&mut self, from: CoverSnapshot, to: CoverSnapshot, dir: i8, now: Instant) {
        self.cover_anim = Some(CoverAnim {
            from,
            to,
            dir,
            started_at: now,
            duration: Duration::from_millis(220),
        });
    }

    pub fn is_playlist_open(&self) -> bool {
        self.overlay == Overlay::Playlist
    }

    pub fn open_playlist(&mut self, width: i16) {
        self.overlay = Overlay::Playlist;
        self.playlist_slide_x = -width;
        self.playlist_slide_target_x = 0;
    }

    pub fn close_playlist(&mut self, width: i16) {
        self.overlay = Overlay::None;
        self.playlist_slide_target_x = -width;
    }

    pub fn open_folder_input(&mut self) {
        self.overlay = Overlay::FolderInput;
        self.folder_input.buf.clear();
    }

    pub fn close_overlay(&mut self) {
        self.overlay = Overlay::None;
    }
}
