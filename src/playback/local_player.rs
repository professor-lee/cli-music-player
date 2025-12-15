use crate::app::state::{EqSettings, PlaybackState, TrackMetadata};
use crate::data::playlist::{Playlist, PlaylistItem};
use crate::playback::metadata::read_metadata;
use anyhow::{anyhow, Result};
use rodio::{OutputStream, Sink, Source};
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
    low_db_x10: AtomicI32,
    mid_db_x10: AtomicI32,
    high_db_x10: AtomicI32,
}

impl EqParams {
    fn new() -> Self {
        Self {
            low_db_x10: AtomicI32::new(0),
            mid_db_x10: AtomicI32::new(0),
            high_db_x10: AtomicI32::new(0),
        }
    }

    fn set_from(&self, eq: EqSettings) {
        let eq = eq.clamp();
        self.low_db_x10.store((eq.low_db * 10.0).round() as i32, Ordering::Relaxed);
        self.mid_db_x10.store((eq.mid_db * 10.0).round() as i32, Ordering::Relaxed);
        self.high_db_x10.store((eq.high_db * 10.0).round() as i32, Ordering::Relaxed);
    }

    fn load_db(&self) -> [f32; 3] {
        [
            self.low_db_x10.load(Ordering::Relaxed) as f32 / 10.0,
            self.mid_db_x10.load(Ordering::Relaxed) as f32 / 10.0,
            self.high_db_x10.load(Ordering::Relaxed) as f32 / 10.0,
        ]
    }

    fn load_db_x10(&self) -> [i32; 3] {
        [
            self.low_db_x10.load(Ordering::Relaxed),
            self.mid_db_x10.load(Ordering::Relaxed),
            self.high_db_x10.load(Ordering::Relaxed),
        ]
    }
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
        }
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
        for entry in std::fs::read_dir(p)? {
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
        playlist.clamp_selected();
        playlist.set_current_selected();

        if let Some(path) = playlist.current_path().cloned() {
            let track = self.play_file(&path)?;
            Ok((playlist, track))
        } else {
            Ok((playlist, TrackMetadata::default()))
        }
    }

    pub fn play_file(&mut self, path: &Path) -> Result<TrackMetadata> {
        // stop current (avoid blocking rebuilds; keep the sink and just clear sources)
        self.sink.clear();

        // metadata
        let meta = read_metadata(path).unwrap_or_default();
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
    last_db_x10: [i32; 3],
    coeffs: [BiquadCoeffs; 3],
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

        let coeffs = [
            biquad_peaking(fs, 100.0, 1.0, eq_db[0]),
            biquad_peaking(fs, 1000.0, 1.0, eq_db[1]),
            biquad_peaking(fs, 8000.0, 1.0, eq_db[2]),
        ];

        let states = vec![BiquadState::default(); (channels as usize) * 3];

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
        ch * 3 + band
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
            self.coeffs = [
                biquad_peaking(fs, 100.0, 1.0, eq_db[0]),
                biquad_peaking(fs, 1000.0, 1.0, eq_db[1]),
                biquad_peaking(fs, 8000.0, 1.0, eq_db[2]),
            ];
            self.last_db_x10 = cur;
        }

        let x = self.inner.next()?;
        let ch = (self.idx % (self.channels as usize)).min(self.channels as usize - 1);
        self.idx = self.idx.wrapping_add(1);

        let mut y = x;
        for band in 0..3 {
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
