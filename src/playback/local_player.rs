use crate::app::state::{EQ_BANDS, EQ_FREQS_HZ, EqSettings, LocalFolderKind, PlaybackState, TrackMetadata};
use crate::data::playlist::{Playlist, PlaylistItem};
use crate::playback::metadata::read_metadata;
use crate::playback::metadata::read_cover_from_folder;
use anyhow::{anyhow, Result};
use rodio::{OutputStream, Sink, Source};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicI32, AtomicU32, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::{FormatOptions, SeekMode, SeekTo};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use symphonia::core::units::Time;

struct EqParams {
    // store dB * 10 as integer to avoid float atomics
    bands_db_x10: [AtomicI32; EQ_BANDS],
}

impl EqParams {
    fn new() -> Self {
        Self {
            bands_db_x10: std::array::from_fn(|_| AtomicI32::new(0)),
        }
    }

    fn set_from(&self, eq: EqSettings) {
        let eq = eq.clamp();
        for (i, v) in eq.bands_db.iter().enumerate() {
            self.bands_db_x10[i].store((v * 10.0).round() as i32, Ordering::Relaxed);
        }
    }

    fn load_db(&self) -> [f32; EQ_BANDS] {
        std::array::from_fn(|i| self.bands_db_x10[i].load(Ordering::Relaxed) as f32 / 10.0)
    }

    fn load_db_x10(&self) -> [i32; EQ_BANDS] {
        std::array::from_fn(|i| self.bands_db_x10[i].load(Ordering::Relaxed))
    }
}

#[derive(Debug, Serialize, Deserialize, Default)]
struct OrderFile {
    order: Vec<String>,

    #[serde(default)]
    last_opened_song: Option<String>,

    #[serde(default)]
    last_album: Option<String>,
}

fn order_key(folder: &Path, path: &Path) -> String {
    let rel = path.strip_prefix(folder).unwrap_or(path);
    let s = rel.to_string_lossy().to_string();
    // Normalize in case paths ever contain backslashes (e.g., copied config).
    s.replace('\\', "/")
}

fn read_order_file(folder: &Path) -> Option<OrderFile> {
    let p = folder.join(".order.toml");
    let s = std::fs::read_to_string(p).ok()?;
    toml::from_str(&s).ok()
}

fn write_order_file_struct(folder: &Path, of: &OrderFile) -> Result<()> {
    let content = toml::to_string_pretty(of)?;

    let tmp = folder.join(".order.toml.tmp");
    let dst = folder.join(".order.toml");
    std::fs::write(&tmp, content)?;
    // Best-effort atomic replace.
    let _ = std::fs::remove_file(&dst);
    std::fs::rename(&tmp, &dst)?;
    Ok(())
}

fn apply_order_file(folder: &Path, playlist: &mut Playlist, order: &OrderFile) {
    if playlist.items.is_empty() {
        return;
    }

    let selected_path = playlist.selected_path().cloned();
    let current_path = playlist.current_path().cloned();

    let mut key_to_index: HashMap<String, usize> = HashMap::with_capacity(playlist.items.len());
    for (i, it) in playlist.items.iter().enumerate() {
        key_to_index.insert(order_key(folder, &it.path), i);
    }

    let mut used = vec![false; playlist.items.len()];
    let mut new_items: Vec<PlaylistItem> = Vec::with_capacity(playlist.items.len());

    for k in &order.order {
        if let Some(&idx) = key_to_index.get(k) {
            if !used[idx] {
                used[idx] = true;
                new_items.push(playlist.items[idx].clone());
            }
        }
    }

    for (i, it) in playlist.items.iter().enumerate() {
        if !used[i] {
            new_items.push(it.clone());
        }
    }

    playlist.items = new_items;

    // Restore selection/current by path (best-effort).
    if let Some(sp) = selected_path {
        if let Some(i) = playlist.items.iter().position(|it| it.path == sp) {
            playlist.selected = i;
        }
    }
    if let Some(cp) = current_path {
        if let Some(i) = playlist.items.iter().position(|it| it.path == cp) {
            playlist.current = Some(i);
        }
    }
    playlist.clamp_selected();
}

pub fn write_order_file(folder: &Path, playlist: &Playlist) -> Result<()> {
    let mut of = read_order_file(folder).unwrap_or_default();
    of.order = playlist
        .items
        .iter()
        .map(|it| order_key(folder, &it.path))
        .collect::<Vec<_>>();
    write_order_file_struct(folder, &of)
}

pub fn write_last_opened_song(folder: &Path, song_path: &Path) -> Result<()> {
    let mut of = read_order_file(folder).unwrap_or_default();
    of.last_opened_song = Some(order_key(folder, song_path));
    write_order_file_struct(folder, &of)
}

pub fn write_last_album(root: &Path, album_folder: &Path) -> Result<()> {
    let mut of = read_order_file(root).unwrap_or_default();
    let rel = album_folder.strip_prefix(root).unwrap_or(album_folder);
    of.last_album = Some(rel.to_string_lossy().replace('\\', "/"));
    write_order_file_struct(root, &of)
}

fn apply_last_opened_song(folder: &Path, playlist: &mut Playlist, of: &OrderFile) {
    let Some(k) = of.last_opened_song.as_ref() else {
        return;
    };
    if let Some(i) = playlist
        .items
        .iter()
        .position(|it| order_key(folder, &it.path) == *k)
    {
        playlist.selected = i;
        playlist.clamp_selected();
        playlist.set_current_selected();
    }
}

#[derive(Debug)]
pub struct LoadPathResult {
    pub kind: LocalFolderKind,
    pub root_folder: PathBuf,
    pub playback_folder: PathBuf,
    pub album_folders: Vec<PathBuf>,
    pub album_index: usize,
    pub album_cover: Option<(Vec<u8>, u64)>,
    pub playlist: Playlist,
    pub track: TrackMetadata,
}

fn is_hidden_or_order_file(path: &Path) -> bool {
    path.file_name()
        .and_then(|s| s.to_str())
        .map(|s| s.starts_with('.') || s.eq_ignore_ascii_case("thumbs.db"))
        .unwrap_or(false)
}

fn detect_album_folder(folder: &Path) -> bool {
    let Ok(rd) = std::fs::read_dir(folder) else {
        return false;
    };

    let mut has_audio = false;
    for entry in rd.flatten() {
        let p = entry.path();
        if p.is_dir() {
            return false;
        }
        if is_hidden_or_order_file(&p) {
            continue;
        }

        if is_audio(&p) {
            has_audio = true;
            continue;
        }

        // allow cover image files named cover.*
        if let Some(stem) = p.file_stem().and_then(|s| s.to_str()) {
            if stem.eq_ignore_ascii_case("cover") {
                continue;
            }
        }

        // allow external lyrics files
        if p.extension().and_then(|s| s.to_str()).map(|s| s.eq_ignore_ascii_case("lrc")) == Some(true) {
            continue;
        }

        // any other file => not an "album folder" per spec
        return false;
    }

    has_audio
}

fn detect_folder_kind(folder: &Path) -> (LocalFolderKind, Vec<PathBuf>) {
    // Multi-album: no audio at root + has >=1 album subfolder.
    let mut root_has_audio = false;
    let mut album_folders: Vec<PathBuf> = Vec::new();

    let Ok(rd) = std::fs::read_dir(folder) else {
        return (LocalFolderKind::Plain, Vec::new());
    };
    for entry in rd.flatten() {
        let p = entry.path();
        if p.is_file() {
            if is_audio(&p) {
                root_has_audio = true;
            }
            continue;
        }
        if p.is_dir() {
            if detect_album_folder(&p) {
                album_folders.push(p);
            }
        }
    }
    album_folders.sort();

    if !root_has_audio && !album_folders.is_empty() {
        return (LocalFolderKind::MultiAlbum, album_folders);
    }
    if detect_album_folder(folder) {
        return (LocalFolderKind::Album, Vec::new());
    }
    (LocalFolderKind::Plain, Vec::new())
}

pub struct LocalPlayer {
    _stream: OutputStream,
    sink: Sink,

    current_path: Option<PathBuf>,
    duration: Option<Duration>,

    volume: f32,

    eq: EqSettings,
    eq_params: Arc<EqParams>,

    // position tracking
    base_seek: Duration,
    started_at: Option<Instant>,
    paused_acc: Duration,

    // visualization tap (last ~16384 samples)
    viz_samples: Arc<VizRing>,

    // metadata cache (avoid expensive tag parsing for cover/lyrics)
    meta_cache: HashMap<PathBuf, TrackMetadata>,
    meta_order: VecDeque<PathBuf>,
    meta_cap: usize,
}

impl LocalPlayer {
    pub fn new() -> Self {
        let (_stream, handle) = OutputStream::try_default().expect("no output device");
        let sink = Sink::try_new(&handle).expect("sink");
        let eq_params = Arc::new(EqParams::new());
        Self {
            _stream,
            sink,
            current_path: None,
            duration: None,
            volume: 0.0,

            eq: EqSettings::default(),
            eq_params,
            base_seek: Duration::from_secs(0),
            started_at: None,
            paused_acc: Duration::from_secs(0),
            viz_samples: Arc::new(VizRing::new(16384)),

            meta_cache: HashMap::new(),
            meta_order: VecDeque::new(),
            meta_cap: 64,
        }
    }

    fn cached_metadata(&mut self, path: &Path) -> TrackMetadata {
        if let Some(m) = self.meta_cache.get(path) {
            // touch
            if let Some(pos) = self.meta_order.iter().position(|p| p == path) {
                let p = self.meta_order.remove(pos).unwrap_or_else(|| path.to_path_buf());
                self.meta_order.push_back(p);
            }
            return m.clone();
        }

        let meta = read_metadata(path).unwrap_or_default();
        let key = path.to_path_buf();
        self.meta_cache.insert(key.clone(), meta.clone());
        self.meta_order.push_back(key);

        while self.meta_order.len() > self.meta_cap {
            if let Some(old) = self.meta_order.pop_front() {
                self.meta_cache.remove(&old);
            }
        }

        meta
    }

    pub fn set_eq(&mut self, eq: EqSettings) -> Result<()> {
        // 需求：自动应用时不能有明显延迟。
        // 这里改为更新共享参数，EqSource 会在运行时重算系数，无需 seek 重建。
        self.eq = eq.clamp();
        self.eq_params.set_from(self.eq);
        Ok(())
    }

    pub fn load_folder(&mut self, folder: &str) -> Result<(Playlist, TrackMetadata)> {
        let p = PathBuf::from(folder);
        if !p.exists() {
            return Err(anyhow!("not found"));
        }

        let mut playlist = Playlist::default();
        let mut files: Vec<PathBuf> = Vec::new();
        for entry in std::fs::read_dir(&p)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() && is_audio(&path) {
                files.push(path);
            }
        }
        files.sort();

        for path in files {
            let title = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("Unknown")
                .to_string();
            playlist.items.push(PlaylistItem { path, title });
        }

        // Optional persisted order (local folder only). If it fails to parse, keep default order.
        if let Some(order) = read_order_file(&p) {
            apply_order_file(&p, &mut playlist, &order);
        }

        // Restore last opened song if present.
        if let Some(order) = read_order_file(&p) {
            apply_last_opened_song(&p, &mut playlist, &order);
        }

        // For a freshly loaded folder, start from the top of the (possibly re-ordered) list.
        playlist.selected = 0;

        playlist.clamp_selected();
        playlist.set_current_selected();

        if let Some(path) = playlist.current_path().cloned() {
            let track = self.play_file(&path)?;
            Ok((playlist, track))
        } else {
            Ok((playlist, TrackMetadata::default()))
        }
    }

    pub fn load_path(&mut self, folder: &Path) -> Result<LoadPathResult> {
        let folder = folder.to_path_buf();
        if !folder.exists() {
            return Err(anyhow!("not found"));
        }

        let (kind, album_folders) = detect_folder_kind(&folder);
        match kind {
            LocalFolderKind::Plain | LocalFolderKind::Album => {
                let (playlist, track) = self.load_folder(folder.to_string_lossy().as_ref())?;
                let cover = read_cover_from_folder(&folder);
                Ok(LoadPathResult {
                    kind,
                    root_folder: folder.clone(),
                    playback_folder: folder,
                    album_folders: Vec::new(),
                    album_index: 0,
                    album_cover: cover,
                    playlist,
                    track,
                })
            }
            LocalFolderKind::MultiAlbum => {
                let of = read_order_file(&folder).unwrap_or_default();
                let mut album_index = 0usize;
                if let Some(last) = of.last_album.as_ref() {
                    if let Some(i) = album_folders.iter().position(|p| {
                        let rel = p.strip_prefix(&folder).unwrap_or(p);
                        rel.to_string_lossy().replace('\\', "/") == *last
                    }) {
                        album_index = i;
                    }
                }
                let playback_folder = album_folders
                    .get(album_index)
                    .cloned()
                    .ok_or_else(|| anyhow!("no album folders"))?;

                let (playlist, track) = self.load_folder(playback_folder.to_string_lossy().as_ref())?;
                let cover = read_cover_from_folder(&playback_folder);
                Ok(LoadPathResult {
                    kind,
                    root_folder: folder,
                    playback_folder,
                    album_folders,
                    album_index,
                    album_cover: cover,
                    playlist,
                    track,
                })
            }
        }
    }

    pub fn load_playlist_only(&mut self, folder: &Path, restore_last_opened: bool) -> Result<Playlist> {
        let p = folder.to_path_buf();
        let mut playlist = Playlist::default();
        let mut files: Vec<PathBuf> = Vec::new();
        for entry in std::fs::read_dir(&p)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() && is_audio(&path) {
                files.push(path);
            }
        }
        files.sort();

        for path in files {
            let title = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("Unknown")
                .to_string();
            playlist.items.push(PlaylistItem { path, title });
        }

        if let Some(order) = read_order_file(&p) {
            apply_order_file(&p, &mut playlist, &order);
            if restore_last_opened {
                apply_last_opened_song(&p, &mut playlist, &order);
            }
        }

        playlist.clamp_selected();
        Ok(playlist)
    }

    pub fn play_file(&mut self, path: &Path) -> Result<TrackMetadata> {
        // stop current (avoid blocking rebuilds; keep the sink and just clear sources)
        self.sink.clear();

        // metadata
        let meta = self.cached_metadata(path);
        self.duration = Some(meta.duration);
        self.current_path = Some(path.to_path_buf());

        // reset position
        self.base_seek = Duration::from_secs(0);
        self.paused_acc = Duration::from_secs(0);
        self.started_at = Some(Instant::now());

        // apply volume
        self.sink.set_volume(self.volume);

        self.viz_samples.clear();
        let src = SymphoniaSource::open(path, Duration::from_secs(0), Some(meta.duration))?;
        // ensure params reflect current state
        self.eq_params.set_from(self.eq);
        let eqd = EqSource::new(src, Arc::clone(&self.eq_params));
        let tapped = TapSource::new(eqd, Arc::clone(&self.viz_samples));
        self.sink.append(tapped);
        self.sink.play();
        Ok(meta)
    }

    pub fn pause(&mut self) -> Result<()> {
        if self.started_at.is_some() {
            // paused_acc is accumulated time *after* base_seek.
            // This matters after a seek: storing the absolute position would double-count base_seek
            // and cause the UI progress to jump.
            let pos = self.position().unwrap_or_default();
            self.paused_acc = pos.saturating_sub(self.base_seek);
            self.started_at = None;
        }
        self.sink.pause();
        Ok(())
    }

    pub fn toggle_play_pause(&mut self) -> Result<()> {
        if self.sink.is_paused() {
            self.sink.play();
            self.started_at = Some(Instant::now());
        } else {
            self.pause()?;
        }
        Ok(())
    }

    pub fn set_volume(&mut self, v: f32) {
        self.volume = v.clamp(0.0, 1.0);
        self.sink.set_volume(self.volume);
    }

    pub fn volume(&self) -> f32 {
        self.volume
    }

    pub fn playback_state(&self) -> PlaybackState {
        if self.current_path.is_none() {
            return PlaybackState::Stopped;
        }
        // When the sink has no more sources (track finished), treat as stopped.
        if self.sink.empty() {
            return PlaybackState::Stopped;
        }
        if self.sink.is_paused() {
            PlaybackState::Paused
        } else {
            PlaybackState::Playing
        }
    }

    pub fn position(&self) -> Option<Duration> {
        if self.current_path.is_none() {
            return None;
        }
        let mut pos = if let Some(start) = self.started_at {
            self.base_seek + self.paused_acc + start.elapsed()
        } else {
            self.base_seek + self.paused_acc
        };
        if let Some(dur) = self.duration {
            if pos > dur {
                pos = dur;
            }
        }
        Some(pos)
    }

    /// Called from the UI tick loop.
    /// Returns true if we just transitioned from playing -> finished.
    pub fn poll_end(&mut self) -> bool {
        if self.current_path.is_none() {
            return false;
        }
        // Only transition once: when we were "playing" (started_at exists)
        // and the sink becomes empty OR we reached the known duration.
        if self.started_at.is_some() {
            let mut finished = self.sink.empty();
            if !finished {
                if let Some(dur) = self.duration {
                    // Some formats may not flip sink.empty reliably; use duration as fallback.
                    if dur > Duration::from_millis(0) {
                        if let Some(pos) = self.position() {
                            finished = pos + Duration::from_millis(120) >= dur;
                        }
                    }
                }
            }

            if finished {
                let final_pos = self.position().unwrap_or_default();
                self.paused_acc = final_pos.saturating_sub(self.base_seek);
                self.started_at = None;
                return true;
            }
        }
        false
    }

    /// Restart the current track from the beginning (used when playback finished).
    pub fn restart_current(&mut self) -> Result<Option<TrackMetadata>> {
        let Some(path) = self.current_path.clone() else {
            return Ok(None);
        };
        self.play_file(&path).map(Some)
    }

    pub fn duration(&self) -> Option<Duration> {
        self.duration
    }

    pub fn seek(&mut self, pos: Duration) -> Result<()> {
        let Some(path) = self.current_path.clone() else {
            return Ok(());
        };

        let was_paused = self.sink.is_paused();

        // Replace source without rebuilding the output sink (prevents UI stalls on some systems).
        self.sink.clear();
        self.sink.set_volume(self.volume);

        self.viz_samples.clear();
        let src = SymphoniaSource::open(&path, pos, self.duration)?;
        self.eq_params.set_from(self.eq);
        let eqd = EqSource::new(src, Arc::clone(&self.eq_params));
        let tapped = TapSource::new(eqd, Arc::clone(&self.viz_samples));
        self.sink.append(tapped);

        if was_paused {
            self.sink.pause();
        } else {
            self.sink.play();
        }

        self.base_seek = pos;
        self.paused_acc = Duration::from_secs(0);
        self.started_at = if was_paused { None } else { Some(Instant::now()) };
        Ok(())
    }

    pub fn latest_samples(&self, n: usize) -> Vec<f32> {
        self.viz_samples.latest_samples(n)
    }
}

/// Lock-free fixed-size ring buffer for visualization samples.
///
/// This runs on the audio thread (rodio source pull). Using a Mutex per sample
/// can easily starve audio and trigger ALSA underruns, so we store samples in a
/// ring using atomics.
struct VizRing {
    cap: usize,
    write_idx: AtomicUsize,
    data: Vec<AtomicU32>,
}

impl VizRing {
    fn new(cap: usize) -> Self {
        let cap = cap.max(1);
        let mut data = Vec::with_capacity(cap);
        for _ in 0..cap {
            data.push(AtomicU32::new(0));
        }
        Self { cap, write_idx: AtomicUsize::new(0), data }
    }

    fn clear(&self) {
        self.write_idx.store(0, Ordering::Relaxed);
    }

    fn push(&self, s: f32) {
        let idx = self.write_idx.fetch_add(1, Ordering::Relaxed);
        let pos = idx % self.cap;
        self.data[pos].store(s.to_bits(), Ordering::Relaxed);
    }

    fn latest_samples(&self, n: usize) -> Vec<f32> {
        let end = self.write_idx.load(Ordering::Relaxed);
        if end == 0 {
            return Vec::new();
        }

        let n = n.min(self.cap).min(end);
        let start = end - n;

        let mut out = Vec::with_capacity(n);
        for i in 0..n {
            let pos = (start + i) % self.cap;
            let bits = self.data[pos].load(Ordering::Relaxed);
            out.push(f32::from_bits(bits));
        }
        out
    }
}

struct SymphoniaSource {
    format: Box<dyn symphonia::core::formats::FormatReader>,
    decoder: Box<dyn symphonia::core::codecs::Decoder>,
    track_id: u32,
    channels: u16,
    sample_rate: u32,
    total_duration: Option<Duration>,

    sample_buf: Option<SampleBuffer<f32>>,
    buf: Vec<f32>,
    buf_pos: usize,
}

impl SymphoniaSource {
    fn open(path: &Path, start: Duration, total_duration: Option<Duration>) -> Result<Self> {
        let file = Box::new(File::open(path)?);
        let mss = MediaSourceStream::new(file, Default::default());
        let hint = Hint::new();

        let mut format_opts: FormatOptions = Default::default();
        // Improves seek responsiveness for interactive scrubbing.
        format_opts.prebuild_seek_index = true;
        format_opts.seek_index_fill_rate = 5;

        let metadata_opts: MetadataOptions = Default::default();
        let decoder_opts: DecoderOptions = Default::default();

        let probed = symphonia::default::get_probe().format(&hint, mss, &format_opts, &metadata_opts)?;
        let mut format = probed.format;

        let track = format
            .default_track()
            .ok_or_else(|| anyhow!("no default audio track"))?;
        if track.codec_params.codec == CODEC_TYPE_NULL {
            return Err(anyhow!("unsupported codec"));
        }

        let mut decoder = symphonia::default::get_codecs().make(&track.codec_params, &decoder_opts)?;

        let track_id = track.id;
        let channels = track
            .codec_params
            .channels
            .map(|c| c.count() as u16)
            .unwrap_or(2)
            .max(1);
        let sample_rate = track.codec_params.sample_rate.unwrap_or(44100).max(1);

        // Seek to requested start time (best-effort).
        if start > Duration::from_millis(0) {
            let time = Time::from(start.as_secs_f64());
            let _ = format.seek(SeekMode::Accurate, SeekTo::Time { time, track_id: Some(track_id) });
            decoder.reset();
        }

        Ok(Self {
            format,
            decoder,
            track_id,
            channels,
            sample_rate,
            total_duration,
            sample_buf: None,
            buf: Vec::new(),
            buf_pos: 0,
        })
    }

    fn refill(&mut self) -> Option<()> {
        loop {
            let packet = match self.format.next_packet() {
                Ok(p) => p,
                Err(_) => return None,
            };

            if packet.track_id() != self.track_id {
                continue;
            }

            match self.decoder.decode(&packet) {
                Ok(audio_buf) => {
                    if self.sample_buf.is_none() {
                        let spec = *audio_buf.spec();
                        let duration = audio_buf.capacity() as u64;
                        self.sample_buf = Some(SampleBuffer::<f32>::new(duration, spec));
                    }
                    if let Some(sb) = &mut self.sample_buf {
                        sb.copy_interleaved_ref(audio_buf);
                        self.buf.clear();
                        self.buf.extend_from_slice(sb.samples());
                        self.buf_pos = 0;
                        return Some(());
                    }
                }
                Err(SymphoniaError::DecodeError(_)) => continue,
                Err(_) => return None,
            }
        }
    }
}

impl Iterator for SymphoniaSource {
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        if self.buf_pos >= self.buf.len() {
            self.refill()?;
        }
        let s = self.buf.get(self.buf_pos).copied();
        self.buf_pos = self.buf_pos.saturating_add(1);
        s
    }
}

impl Source for SymphoniaSource {
    fn current_frame_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> u16 {
        self.channels
    }

    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn total_duration(&self) -> Option<Duration> {
        self.total_duration
    }
}

struct TapSource<S>
where
    S: Source<Item = f32>,
{
    inner: S,
    buf: Arc<VizRing>,
}

impl<S> TapSource<S>
where
    S: Source<Item = f32>,
{
    fn new(inner: S, buf: Arc<VizRing>) -> Self {
        Self { inner, buf }
    }
}

impl<S> Iterator for TapSource<S>
where
    S: Source<Item = f32>,
{
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        let s = self.inner.next()?;
        self.buf.push(s);
        Some(s)
    }
}

impl<S> Source for TapSource<S>
where
    S: Source<Item = f32>,
{
    fn current_frame_len(&self) -> Option<usize> {
        self.inner.current_frame_len()
    }

    fn channels(&self) -> u16 {
        self.inner.channels()
    }

    fn sample_rate(&self) -> u32 {
        self.inner.sample_rate()
    }

    fn total_duration(&self) -> Option<Duration> {
        self.inner.total_duration()
    }
}

struct BiquadCoeffs {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
}

#[derive(Default, Clone, Copy)]
struct BiquadState {
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}

fn biquad_peaking(fs: f32, f0: f32, q: f32, gain_db: f32) -> BiquadCoeffs {
    let fs = if fs > 0.0 { fs } else { 44100.0 };
    let f0 = f0.clamp(10.0, fs * 0.45);
    let q = q.max(0.001);

    let a = 10.0_f32.powf(gain_db / 40.0);
    let w0 = 2.0 * std::f32::consts::PI * (f0 / fs);
    let cos_w0 = w0.cos();
    let sin_w0 = w0.sin();
    let alpha = sin_w0 / (2.0 * q);

    let b0 = 1.0 + alpha * a;
    let b1 = -2.0 * cos_w0;
    let b2 = 1.0 - alpha * a;
    let a0 = 1.0 + alpha / a;
    let a1 = -2.0 * cos_w0;
    let a2 = 1.0 - alpha / a;

    BiquadCoeffs {
        b0: b0 / a0,
        b1: b1 / a0,
        b2: b2 / a0,
        a1: a1 / a0,
        a2: a2 / a0,
    }
}

fn biquad_process(c: &BiquadCoeffs, s: &mut BiquadState, x: f32) -> f32 {
    let y = c.b0 * x + c.b1 * s.x1 + c.b2 * s.x2 - c.a1 * s.y1 - c.a2 * s.y2;
    s.x2 = s.x1;
    s.x1 = x;
    s.y2 = s.y1;
    s.y1 = y;
    y
}

struct EqSource<S>
where
    S: Source<Item = f32>,
{
    inner: S,
    channels: u16,
    idx: usize,
    params: Arc<EqParams>,
    last_db_x10: [i32; EQ_BANDS],
    coeffs: [BiquadCoeffs; EQ_BANDS],
    states: Vec<BiquadState>,
}

impl<S> EqSource<S>
where
    S: Source<Item = f32>,
{
    fn new(inner: S, params: Arc<EqParams>) -> Self {
        let channels = inner.channels().max(1);
        let fs = inner.sample_rate() as f32;
        let eq_db = params.load_db();
        let last_db_x10 = params.load_db_x10();

        let coeffs = std::array::from_fn(|i| biquad_peaking(fs, EQ_FREQS_HZ[i], 1.0, eq_db[i]));

        let states = vec![BiquadState::default(); (channels as usize) * EQ_BANDS];

        Self {
            inner,
            channels,
            idx: 0,
            params,
            last_db_x10,
            coeffs,
            states,
        }
    }

    fn state_index(&self, ch: usize, band: usize) -> usize {
        ch * EQ_BANDS + band
    }
}

impl<S> Iterator for EqSource<S>
where
    S: Source<Item = f32>,
{
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        // 如果 EQ 参数变化，重算系数（无需重建播放链路）
        let cur = self.params.load_db_x10();
        if cur != self.last_db_x10 {
            let fs = self.inner.sample_rate() as f32;
            let eq_db = self.params.load_db();
            self.coeffs = std::array::from_fn(|i| biquad_peaking(fs, EQ_FREQS_HZ[i], 1.0, eq_db[i]));
            self.last_db_x10 = cur;
        }

        let x = self.inner.next()?;
        let ch = (self.idx % (self.channels as usize)).min(self.channels as usize - 1);
        self.idx = self.idx.wrapping_add(1);

        let mut y = x;
        for band in 0..EQ_BANDS {
            let si = self.state_index(ch, band);
            y = biquad_process(&self.coeffs[band], &mut self.states[si], y);
        }
        Some(y)
    }
}

impl<S> Source for EqSource<S>
where
    S: Source<Item = f32>,
{
    fn current_frame_len(&self) -> Option<usize> {
        self.inner.current_frame_len()
    }

    fn channels(&self) -> u16 {
        self.inner.channels()
    }

    fn sample_rate(&self) -> u32 {
        self.inner.sample_rate()
    }

    fn total_duration(&self) -> Option<Duration> {
        self.inner.total_duration()
    }
}

fn is_audio(p: &Path) -> bool {
    let Some(ext) = p.extension().and_then(|s| s.to_str()) else {
        return false;
    };
    matches!(
        ext.to_lowercase().as_str(),
        "mp3" | "flac" | "wav" | "ogg" | "aac" | "m4a"
    )
}
