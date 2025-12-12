use crate::app::state::{PlaybackState, TrackMetadata};
use crate::data::playlist::{Playlist, PlaylistItem};
use crate::playback::metadata::read_metadata;
use anyhow::{anyhow, Result};
use rodio::{Decoder, OutputStream, OutputStreamHandle, Sink, Source};
use std::sync::{Arc, Mutex};
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub struct LocalPlayer {
    _stream: OutputStream,
    handle: OutputStreamHandle,
    sink: Sink,

    current_path: Option<PathBuf>,
    duration: Option<Duration>,

    volume: f32,

    // position tracking
    base_seek: Duration,
    started_at: Option<Instant>,
    paused_acc: Duration,

    // visualization tap (last ~16384 samples)
    viz_samples: Arc<Mutex<Vec<f32>>>,
}

impl LocalPlayer {
    pub fn new() -> Self {
        let (_stream, handle) = OutputStream::try_default().expect("no output device");
        let sink = Sink::try_new(&handle).expect("sink");
        Self {
            _stream,
            handle,
            sink,
            current_path: None,
            duration: None,
            volume: 0.0,
            base_seek: Duration::from_secs(0),
            started_at: None,
            paused_acc: Duration::from_secs(0),
            viz_samples: Arc::new(Mutex::new(Vec::with_capacity(16384))),
        }
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
        // stop current
        self.sink.stop();
        self.sink = Sink::try_new(&self.handle)?;

        let file = File::open(path)?;
        let reader = BufReader::new(file);
        let decoder = Decoder::new(reader)?;

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

        clear_samples(&self.viz_samples);
        let tapped = TapSource::new(decoder.convert_samples::<f32>(), Arc::clone(&self.viz_samples));
        self.sink.append(tapped);
        self.sink.play();
        Ok(meta)
    }

    pub fn pause(&mut self) -> Result<()> {
        if self.started_at.is_some() {
            self.paused_acc = self.position().unwrap_or_default();
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
        if let Some(start) = self.started_at {
            Some(self.base_seek + self.paused_acc + start.elapsed())
        } else {
            Some(self.base_seek + self.paused_acc)
        }
    }

    pub fn duration(&self) -> Option<Duration> {
        self.duration
    }

    pub fn seek(&mut self, pos: Duration) -> Result<()> {
        let Some(path) = self.current_path.clone() else {
            return Ok(());
        };
        let file = File::open(&path)?;
        let reader = BufReader::new(file);
        let decoder = Decoder::new(reader)?;

        self.sink.stop();
        self.sink = Sink::try_new(&self.handle)?;
        self.sink.set_volume(self.volume);

        clear_samples(&self.viz_samples);
        let src = decoder.convert_samples::<f32>().skip_duration(pos);
        let tapped = TapSource::new(src, Arc::clone(&self.viz_samples));
        self.sink.append(tapped);
        self.sink.play();

        self.base_seek = pos;
        self.paused_acc = Duration::from_secs(0);
        self.started_at = Some(Instant::now());
        Ok(())
    }

    pub fn latest_samples(&self, n: usize) -> Vec<f32> {
        let guard = self.viz_samples.lock().unwrap();
        if guard.len() <= n {
            return guard.clone();
        }
        guard[guard.len() - n..].to_vec()
    }
}

fn clear_samples(buf: &Arc<Mutex<Vec<f32>>>) {
    let mut guard = buf.lock().unwrap();
    guard.clear();
}

fn push_samples(buf: &Arc<Mutex<Vec<f32>>>, s: f32) {
    let mut guard = buf.lock().unwrap();
    guard.push(s);
    const CAP: usize = 16384;
    if guard.len() > CAP {
        let drop = guard.len() - CAP;
        guard.drain(0..drop);
    }
}

struct TapSource<S>
where
    S: Source<Item = f32>,
{
    inner: S,
    buf: Arc<Mutex<Vec<f32>>>,
}

impl<S> TapSource<S>
where
    S: Source<Item = f32>,
{
    fn new(inner: S, buf: Arc<Mutex<Vec<f32>>>) -> Self {
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
        push_samples(&self.buf, s);
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

fn is_audio(p: &Path) -> bool {
    let Some(ext) = p.extension().and_then(|s| s.to_str()) else {
        return false;
    };
    matches!(
        ext.to_lowercase().as_str(),
        "mp3" | "flac" | "wav" | "ogg" | "aac" | "m4a"
    )
}
