use crate::data::config::Config;
use crate::data::playlist::Playlist;
use crate::render::cover_cache::CoverCache;
use crate::render::cover_cache::CoverKey;
use crate::render::cover_renderer::render_cover_ascii;
use crate::ui::theme::Theme;
use std::cell::RefCell;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
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
    pub bands_db: [f32; EQ_BANDS],
}

pub const EQ_BANDS: usize = 10;
pub const EQ_FREQS_HZ: [f32; EQ_BANDS] = [31.0, 62.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0];

impl Default for EqSettings {
    fn default() -> Self {
        Self { bands_db: [0.0; EQ_BANDS] }
    }
}

impl EqSettings {
    pub fn clamp(self) -> Self {
        let mut out = self;
        for v in &mut out.bands_db {
            *v = v.clamp(-12.0, 12.0);
        }
        out
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

#[derive(Debug, Clone)]
pub struct PlaylistAlbumAnim {
    pub from_cover: Option<Vec<u8>>,
    pub from_hash: Option<u64>,
    pub to_cover: Option<Vec<u8>>,
    pub to_hash: Option<u64>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalFolderKind {
    Plain,
    Album,
    MultiAlbum,
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

    // Playlist overlay browsing list.
    // For MultiAlbum, this can differ from `playlist` (playback queue).
    pub playlist_view: Playlist,
    pub spectrum: SpectrumData,

    pub cover_cache: RefCell<CoverCache>,

    cover_render_tx: Sender<CoverRenderRequest>,
    cover_render_rx: Receiver<CoverRenderResult>,
    cover_render_inflight: HashSet<CoverKey>,

    pub overlay: Overlay,
    pub folder_input: FolderInput,

    pub settings_selected: usize,

    pub eq: EqSettings,
    pub eq_selected: usize,

    // Folder that backs the *current playback queue* (contains audio files).
    pub local_folder: Option<PathBuf>,

    // For MultiAlbum: the root folder containing multiple album folders.
    pub local_root_folder: Option<PathBuf>,
    pub local_folder_kind: LocalFolderKind,

    // For MultiAlbum: all album folders under `local_root_folder`.
    pub local_album_folders: Vec<PathBuf>,
    // Which album folder is currently being *viewed* in the playlist overlay.
    pub local_view_album_index: usize,
    pub local_view_album_folder: Option<PathBuf>,

    // Album cover shown in the playlist overlay's top area.
    pub local_view_album_cover: Option<Vec<u8>>,
    pub local_view_album_cover_hash: Option<u64>,

    pub playlist_album_anim: Option<PlaylistAlbumAnim>,

    pub cover_anim: Option<CoverAnim>,
    pub pending_system_cover_anim: Option<(CoverSnapshot, i8, Instant)>,

    pub toast: Option<(String, Instant)>,

    pub last_mouse_click: Option<(Instant, u16, u16)>,


    // playlist slide animation
    pub playlist_slide_x: i16,
    pub playlist_slide_target_x: i16,

    pub last_frame: Instant,
}

#[derive(Debug)]
struct CoverRenderRequest {
    key: CoverKey,
    bytes: Vec<u8>,
    placeholder: char,
}

#[derive(Debug)]
struct CoverRenderResult {
    key: CoverKey,
    ascii: String,
}

fn fill_ascii(width: u16, height: u16, ch: char) -> String {
    let row = ch.to_string().repeat(width as usize);
    let mut s = String::new();
    for _ in 0..height {
        s.push_str(&row);
        s.push('\n');
    }
    s
}

impl AppState {
    pub fn new(config: Config, theme: Theme) -> Self {
        let (cover_render_tx, cover_render_req_rx) = mpsc::channel::<CoverRenderRequest>();
        let (cover_render_res_tx, cover_render_rx) = mpsc::channel::<CoverRenderResult>();

        std::thread::spawn(move || {
            while let Ok(req) = cover_render_req_rx.recv() {
                let ascii = render_cover_ascii(&req.bytes, req.key.width, req.key.height)
                    .unwrap_or_else(|| fill_ascii(req.key.width, req.key.height, req.placeholder));
                let _ = cover_render_res_tx.send(CoverRenderResult { key: req.key, ascii });
            }
        });

        Self {
            config,
            theme,
            player: PlayerState::default(),
            playlist: Playlist::default(),
            playlist_view: Playlist::default(),
            spectrum: SpectrumData::default(),
            cover_cache: RefCell::new(CoverCache::new(20)),
            cover_render_tx,
            cover_render_rx,
            cover_render_inflight: HashSet::new(),
            overlay: Overlay::None,
            folder_input: FolderInput::default(),
            settings_selected: 0,

            eq: EqSettings::default(),
            eq_selected: 0,

            local_folder: None,
            local_root_folder: None,
            local_folder_kind: LocalFolderKind::Plain,
            local_album_folders: Vec::new(),
            local_view_album_index: 0,
            local_view_album_folder: None,
            local_view_album_cover: None,
            local_view_album_cover_hash: None,

            playlist_album_anim: None,

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

    pub fn queue_cover_ascii_render(&mut self, key: CoverKey, bytes: &[u8], placeholder: char) {
        if self.cover_cache.borrow().contains(key) {
            return;
        }
        if self.cover_render_inflight.contains(&key) {
            return;
        }
        self.cover_render_inflight.insert(key);
        let _ = self.cover_render_tx.send(CoverRenderRequest {
            key,
            bytes: bytes.to_vec(),
            placeholder,
        });
    }

    pub fn tick(&mut self, now: Instant) {
        self.last_frame = now;

        loop {
            match self.cover_render_rx.try_recv() {
                Ok(msg) => {
                    self.cover_render_inflight.remove(&msg.key);
                    self.cover_cache.borrow_mut().put(msg.key, msg.ascii);
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => break,
            }
        }

        if let Some(anim) = &self.cover_anim {
            if now.duration_since(anim.started_at) >= anim.duration {
                self.cover_anim = None;
            }
        }

        if let Some(anim) = &self.playlist_album_anim {
            if now.duration_since(anim.started_at) >= anim.duration {
                self.playlist_album_anim = None;
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
