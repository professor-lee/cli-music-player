use crate::app::state::{LyricLine, TrackMetadata};
use crate::playback::metadata::{parse_lrc, parse_plain_lyrics};
use chromaprint::Chromaprint;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use serde::Deserialize;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TrackKey {
    pub path: Option<PathBuf>,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration_secs: u64,
}

impl TrackKey {
    pub fn from_track(track: &TrackMetadata, path: Option<&Path>) -> Self {
        Self {
            path: path.map(|p| p.to_path_buf()),
            title: track.title.clone(),
            artist: track.artist.clone(),
            album: track.album.clone(),
            duration_secs: track.duration.as_secs(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct FetchOptions {
    pub enable_fetch: bool,
    pub download: bool,
    pub enable_fingerprint: bool,
    pub acoustid_api_key: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RemoteFetchRequest {
    pub key: TrackKey,
    pub path: Option<PathBuf>,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration_secs: u64,
    pub has_lyrics: bool,
    pub has_cover: bool,
    pub options: FetchOptions,
}

#[derive(Debug, Clone)]
pub struct RemoteFetchResult {
    pub key: TrackKey,
    pub path: Option<PathBuf>,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub lyrics: Option<Vec<LyricLine>>,
    pub cover: Option<Vec<u8>>,
    pub cover_hash: Option<u64>,
    pub cover_folder: Option<PathBuf>,
}

impl RemoteFetchResult {
    pub fn apply_to(&self, track: &mut TrackMetadata) {
        if let Some(t) = self.title.as_ref() {
            if track.title == "Unknown" {
                track.title = t.clone();
            }
        }
        if let Some(a) = self.artist.as_ref() {
            if track.artist == "Unknown" {
                track.artist = a.clone();
            }
        }
        if let Some(al) = self.album.as_ref() {
            if track.album == "Unknown" {
                track.album = al.clone();
            }
        }
        if track.lyrics.is_none() {
            if let Some(lines) = self.lyrics.as_ref() {
                track.lyrics = Some(lines.clone());
            }
        }
        if track.cover.is_none() {
            if let Some(bytes) = self.cover.as_ref() {
                track.cover = Some(bytes.clone());
                track.cover_hash = self.cover_hash;
                if let Some(folder) = self.cover_folder.as_ref() {
                    track.cover_folder = Some(folder.clone());
                }
            }
        }
    }
}

pub fn start_remote_fetch_worker() -> (Sender<RemoteFetchRequest>, Receiver<RemoteFetchResult>) {
    let (tx, rx) = mpsc::channel::<RemoteFetchRequest>();
    let (res_tx, res_rx) = mpsc::channel::<RemoteFetchResult>();

    std::thread::spawn(move || worker_loop(rx, res_tx));
    (tx, res_rx)
}

fn worker_loop(rx: Receiver<RemoteFetchRequest>, res_tx: Sender<RemoteFetchResult>) {
    let debounce = Duration::from_millis(700);
    let throttle = Duration::from_secs(120);
    let mut pending: Option<RemoteFetchRequest> = None;
    let mut last_attempt: HashMap<TrackKey, Instant> = HashMap::new();

    loop {
        let mut req = match pending.take() {
            Some(r) => r,
            None => match rx.recv() {
                Ok(r) => r,
                Err(_) => break,
            },
        };

        loop {
            match rx.recv_timeout(debounce) {
                Ok(r) => {
                    req = r;
                    continue;
                }
                Err(mpsc::RecvTimeoutError::Timeout) => break,
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            }
        }

        let now = Instant::now();
        if let Some(last) = last_attempt.get(&req.key) {
            if now.duration_since(*last) < throttle {
                continue;
            }
        }
        last_attempt.insert(req.key.clone(), now);

        if let Some(res) = process_request(req) {
            let _ = res_tx.send(res);
        }
    }
}

fn process_request(req: RemoteFetchRequest) -> Option<RemoteFetchResult> {
    if !req.options.enable_fetch {
        return None;
    }

    let mut title = req.title.clone();
    let mut artist = req.artist.clone();
    let mut album = req.album.clone();
    let mut duration_secs = req.duration_secs;
    let mut release_mbid: Option<String> = None;

    let mut out = RemoteFetchResult {
        key: req.key.clone(),
        path: req.path.clone(),
        title: None,
        artist: None,
        album: None,
        lyrics: None,
        cover: None,
        cover_hash: None,
        cover_folder: None,
    };

    let metadata_missing = is_unknown(&artist) && is_unknown(&album);
    let need_lyrics = !req.has_lyrics;
    let need_cover = !req.has_cover;

    if metadata_missing && req.options.enable_fingerprint && (need_lyrics || need_cover) {
        if let (Some(path), Some(key)) = (req.path.as_deref(), req.options.acoustid_api_key.as_deref()) {
            if let Some((fp, fp_dur)) = chromaprint_fingerprint(path) {
                if duration_secs == 0 {
                    duration_secs = fp_dur as u64;
                }
                if let Some(ac) = acoustid_lookup(key, &fp, fp_dur as u32) {
                    if let Some(t) = ac.title {
                        if is_unknown(&title) {
                            title = t.clone();
                        }
                        out.title = Some(t);
                    }
                    if let Some(a) = ac.artist {
                        if is_unknown(&artist) {
                            artist = a.clone();
                        }
                        out.artist = Some(a);
                    }
                    if let Some(al) = ac.album {
                        if is_unknown(&album) {
                            album = al.clone();
                        }
                        out.album = Some(al);
                    }
                    if let Some(mbid) = ac.release_mbid {
                        release_mbid = Some(mbid);
                    }
                }
            }
        }
    }

    if need_lyrics {
        if duration_secs > 0 && !is_unknown(&title) && !is_unknown(&artist) && !is_unknown(&album) {
            if let Some(lrc) = lrclib_fetch(&title, &artist, &album, duration_secs) {
                if let Some(lines) = parse_lrc(&lrc).or_else(|| parse_plain_lyrics(&lrc)) {
                    out.lyrics = Some(lines);
                }
                if req.options.download {
                    if let Some(path) = req.path.as_deref() {
                        let _ = save_lrc(path, &lrc);
                    }
                }
            }
        }
    }

    if need_cover {
        if let Some(mbid) = release_mbid.clone().or_else(|| musicbrainz_release_id(&title, &artist, &album)) {
            if let Some((bytes, content_type)) = cover_art_archive_fetch(&mbid) {
                out.cover_hash = Some(hash_bytes(&bytes));
                out.cover = Some(bytes.clone());
                if let Some(path) = req.path.as_deref() {
                    if let Some(folder) = path.parent() {
                        out.cover_folder = Some(folder.to_path_buf());
                    }
                }

                if req.options.download {
                    if let Some(path) = req.path.as_deref() {
                        let _ = save_cover(path, &bytes, content_type.as_deref());
                    }
                }
            }
        }
    }

    let changed = out.title.is_some() || out.artist.is_some() || out.album.is_some() || out.lyrics.is_some() || out.cover.is_some();
    if changed { Some(out) } else { None }
}

fn is_unknown(s: &str) -> bool {
    let t = s.trim();
    t.is_empty() || t.eq_ignore_ascii_case("unknown")
}

fn lrclib_fetch(title: &str, artist: &str, album: &str, duration_secs: u64) -> Option<String> {
    let agent = "cli-music-player/0.1.0 (https://github.com)";
    let resp = http_agent()
        .get("https://lrclib.net/api/get")
        .set("User-Agent", agent)
        .query("track_name", title)
        .query("artist_name", artist)
        .query("album_name", album)
        .query("duration", &duration_secs.to_string())
        .call()
        .ok()?;

    if resp.status() != 200 {
        return None;
    }

    let body: LrclibResponse = resp.into_json().ok()?;
    if let Some(s) = body.synced_lyrics {
        if !s.trim().is_empty() {
            return Some(s);
        }
    }
    if let Some(p) = body.plain_lyrics {
        if !p.trim().is_empty() {
            return Some(p);
        }
    }
    None
}

#[derive(Debug, Deserialize)]
struct LrclibResponse {
    #[serde(default, rename = "syncedLyrics")]
    synced_lyrics: Option<String>,
    #[serde(default, rename = "plainLyrics")]
    plain_lyrics: Option<String>,
}

fn musicbrainz_release_id(title: &str, artist: &str, album: &str) -> Option<String> {
    if is_unknown(title) || is_unknown(artist) {
        return None;
    }

    let mut query = format!("recording:\"{}\" AND artist:\"{}\"", sanitize_mb(title), sanitize_mb(artist));
    if !is_unknown(album) {
        query.push_str(&format!(" AND release:\"{}\"", sanitize_mb(album)));
    }

    let agent = "cli-music-player/0.1.0 (https://github.com)";
    let resp = http_agent()
        .get("https://musicbrainz.org/ws/2/recording/")
        .set("User-Agent", agent)
        .query("query", &query)
        .query("fmt", "json")
        .query("limit", "1")
        .query("inc", "releases")
        .call()
        .ok()?;

    if resp.status() != 200 {
        return None;
    }

    let body: MbRecordingResponse = resp.into_json().ok()?;
    let rec = body.recordings?.into_iter().next()?;
    if let Some(releases) = rec.releases {
        return releases.into_iter().next().map(|r| r.id);
    }
    None
}

fn sanitize_mb(s: &str) -> String {
    s.replace('"', " ").trim().to_string()
}

#[derive(Debug, Deserialize)]
struct MbRecordingResponse {
    recordings: Option<Vec<MbRecording>>,
}

#[derive(Debug, Deserialize)]
struct MbRecording {
    #[serde(default)]
    releases: Option<Vec<MbRelease>>,
}

#[derive(Debug, Deserialize)]
struct MbRelease {
    id: String,
}

fn cover_art_archive_fetch(release_id: &str) -> Option<(Vec<u8>, Option<String>)> {
    let agent = "cli-music-player/0.1.0 (https://github.com)";
    let url = format!("https://coverartarchive.org/release/{}/front-500", release_id);
    let resp = http_agent().get(&url).set("User-Agent", agent).call().ok()?;
    if resp.status() != 200 {
        return None;
    }
    if let Some(len) = resp.header("Content-Length") {
        if let Ok(n) = len.parse::<u64>() {
            if n > 5 * 1024 * 1024 {
                return None;
            }
        }
    }
    let content_type = resp.header("Content-Type").map(|s| s.to_string());
    let mut reader = resp.into_reader();
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes).ok()?;
    if bytes.is_empty() {
        return None;
    }
    Some((bytes, content_type))
}

#[derive(Debug, Deserialize)]
struct AcoustidResponse {
    status: String,
    results: Option<Vec<AcoustidResult>>,
}

#[derive(Debug, Deserialize)]
struct AcoustidResult {
    recordings: Option<Vec<AcoustidRecording>>,
}

#[derive(Debug, Deserialize)]
struct AcoustidRecording {
    title: Option<String>,
    artists: Option<Vec<AcoustidArtist>>,
    releases: Option<Vec<AcoustidRelease>>,
}

#[derive(Debug, Deserialize)]
struct AcoustidArtist {
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AcoustidRelease {
    id: Option<String>,
    title: Option<String>,
}

struct AcoustidLookup {
    title: Option<String>,
    artist: Option<String>,
    album: Option<String>,
    release_mbid: Option<String>,
}

fn acoustid_lookup(api_key: &str, fingerprint: &str, duration_secs: u32) -> Option<AcoustidLookup> {
    if api_key.trim().is_empty() {
        return None;
    }

    let agent = "cli-music-player/0.1.0 (https://github.com)";
    let resp = http_agent()
        .get("https://api.acoustid.org/v2/lookup")
        .set("User-Agent", agent)
        .query("client", api_key)
        .query("meta", "recordings+releases")
        .query("duration", &duration_secs.to_string())
        .query("fingerprint", fingerprint)
        .query("format", "json")
        .call()
        .ok()?;

    if resp.status() != 200 {
        return None;
    }

    let body: AcoustidResponse = resp.into_json().ok()?;
    if body.status != "ok" {
        return None;
    }
    let result = body.results?.into_iter().next()?;
    let rec = result.recordings?.into_iter().next()?;

    let title = rec.title;
    let artist = rec.artists.and_then(|mut a| a.pop()).and_then(|a| a.name);
    let (album, release_mbid) = if let Some(mut releases) = rec.releases {
        if let Some(r) = releases.pop() {
            (r.title, r.id)
        } else {
            (None, None)
        }
    } else {
        (None, None)
    };

    Some(AcoustidLookup {
        title,
        artist,
        album,
        release_mbid,
    })
}

fn chromaprint_fingerprint(path: &Path) -> Option<(String, u32)> {
    let file = std::fs::File::open(path).ok()?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let hint = Hint::new();
    let format_opts: FormatOptions = Default::default();
    let metadata_opts: MetadataOptions = Default::default();
    let decoder_opts: DecoderOptions = Default::default();

    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &format_opts, &metadata_opts)
        .ok()?;
    let mut format = probed.format;

    let track = format.default_track()?;
    if track.codec_params.codec == symphonia::core::codecs::CODEC_TYPE_NULL {
        return None;
    }

    let track_id = track.id;
    let channels = track
        .codec_params
        .channels
        .map(|c| c.count() as u16)
        .unwrap_or(2)
        .max(1);
    let sample_rate = track.codec_params.sample_rate.unwrap_or(44100).max(1);

    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &decoder_opts)
        .ok()?;

    let mut chroma = Chromaprint::new();
    if !chroma.start(sample_rate as i32, channels as i32) {
        return None;
    }

    let mut total_frames: u64 = 0;
    let mut sample_buf: Option<SampleBuffer<i16>> = None;

    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(_) => break,
        };

        if packet.track_id() != track_id {
            continue;
        }

        match decoder.decode(&packet) {
            Ok(audio_buf) => {
                if sample_buf.is_none() {
                    let spec = *audio_buf.spec();
                    let duration = audio_buf.capacity() as u64;
                    sample_buf = Some(SampleBuffer::<i16>::new(duration, spec));
                }
                if let Some(sb) = &mut sample_buf {
                    sb.copy_interleaved_ref(audio_buf);
                    let samples = sb.samples();
                    if !chroma.feed(samples) {
                        return None;
                    }
                    let frames = samples.len() as u64 / channels as u64;
                    total_frames = total_frames.saturating_add(frames);
                }
            }
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(_) => break,
        }
    }

    if !chroma.finish() {
        return None;
    }

    let fp = chroma.fingerprint()?;
    let duration_secs = (total_frames as f64 / sample_rate as f64).round() as u32;
    Some((fp, duration_secs))
}

fn save_lrc(audio_path: &Path, lrc: &str) -> std::io::Result<()> {
    let Some(folder) = audio_path.parent() else {
        return Ok(());
    };
    let Some(stem) = audio_path.file_stem().and_then(|s| s.to_str()) else {
        return Ok(());
    };
    let dir = folder.join("lrc");
    std::fs::create_dir_all(&dir)?;
    let p = dir.join(format!("{stem}.lrc"));
    std::fs::write(p, lrc.as_bytes())
}

fn save_cover(audio_path: &Path, bytes: &[u8], content_type: Option<&str>) -> std::io::Result<()> {
    let Some(folder) = audio_path.parent() else {
        return Ok(());
    };
    let Some(stem) = audio_path.file_stem().and_then(|s| s.to_str()) else {
        return Ok(());
    };

    let ext = match content_type {
        Some(ct) if ct.contains("png") => "png",
        Some(ct) if ct.contains("jpeg") || ct.contains("jpg") => "jpg",
        _ => "jpg",
    };

    let dir = folder.join("cover");
    std::fs::create_dir_all(&dir)?;
    let p = dir.join(format!("{stem}.{ext}"));
    std::fs::write(p, bytes)
}

fn hash_bytes(bytes: &[u8]) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut h);
    h.finish()
}

fn http_agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(8))
        .build()
}
