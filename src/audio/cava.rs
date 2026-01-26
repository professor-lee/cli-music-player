use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};
use tempfile::TempDir;

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CavaChannels {
    Stereo,
    Mono,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CavaConfig {
    pub framerate_hz: u32,
    pub bars: usize,
    pub channels: CavaChannels,
    pub reverse: bool,
}

pub struct CavaRunner {
    left: Arc<Mutex<Vec<f32>>>,
    right: Arc<Mutex<Vec<f32>>>,
    channels: CavaChannels,
    child: Child,
    _reader: thread::JoinHandle<()>,
    cfg_path: String,
    _temp_dir: Option<TempDir>,
}

impl CavaRunner {
    pub fn start(cfg: CavaConfig) -> Result<Self> {
        // Minimal config we generate ourselves (do not copy upstream example config).
        // Uses raw ascii output to stdout.
        // We request stereo; depending on cava version/backend, it may emit:
        // - 2 lines per frame (one per channel, each 64 values), OR
        // - 1 line per frame containing 128 values.
        let framerate_hz = cfg.framerate_hz.clamp(10, 120);
        let bars = cfg.bars.clamp(8, 96);
        let channels = cfg.channels;
        let channels_str = match channels {
            CavaChannels::Stereo => "stereo",
            CavaChannels::Mono => "mono",
        };
        let reverse = if cfg.reverse { 1 } else { 0 };
        let cfg = format!(
            "[general]\nframerate = {fr}\nbars = {bars}\nreverse = {reverse}\n\n[input]\n# Leave method/source unset: cava will pick the best supported backend (pipewire/pulse/etc).\n\n[output]\nmethod = raw\nchannels = {channels}\nraw_target = /dev/stdout\ndata_format = ascii\nascii_max_range = 1000\nbar_delimiter = 59\nframe_delimiter = 10\n",
            fr = framerate_hz,
            bars = bars,
            reverse = reverse,
            channels = channels_str
        );

        let cfg_path = temp_cfg_path();
        fs::write(&cfg_path, cfg).with_context(|| format!("write cava config: {cfg_path}"))?;

        let (cava_exe, temp_dir) = resolve_cava_executable()?;
        let mut child = Command::new(&cava_exe)
            .arg("-p")
            .arg(&cfg_path)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .with_context(|| format!("spawn cava: {}", cava_exe.display()))?;

        let stdout = child
            .stdout
            .take()
            .context("failed to capture cava stdout")?;

        let left: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(vec![0.0; bars]));
        let right: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(vec![0.0; bars]));
        let left_cloned = Arc::clone(&left);
        let right_cloned = Arc::clone(&right);

        let reader = thread::spawn(move || {
            let mut br = BufReader::new(stdout);
            let mut line = String::new();
            let mut next_is_left = true;
            loop {
                line.clear();
                match br.read_line(&mut line) {
                    Ok(0) => break, // EOF
                    Ok(_) => {
                        let frames = parse_frames_ascii(&line, bars);
                        match channels {
                            CavaChannels::Mono => {
                                if let Some(frame) = frames.get(0) {
                                    let mut g = left_cloned.lock().unwrap();
                                    *g = frame.clone();
                                    let mut r = right_cloned.lock().unwrap();
                                    *r = frame.clone();
                                }
                            }
                            CavaChannels::Stereo => match frames.len() {
                                1 => {
                                    let frame = frames[0].clone();
                                    if next_is_left {
                                        let mut g = left_cloned.lock().unwrap();
                                        *g = frame;
                                    } else {
                                        let mut g = right_cloned.lock().unwrap();
                                        *g = frame;
                                    }
                                    next_is_left = !next_is_left;
                                }
                                2 => {
                                    {
                                        let mut g = left_cloned.lock().unwrap();
                                        *g = frames[0].clone();
                                    }
                                    {
                                        let mut g = right_cloned.lock().unwrap();
                                        *g = frames[1].clone();
                                    }
                                    next_is_left = true;
                                }
                                _ => {}
                            },
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        Ok(Self {
            left,
            right,
            channels,
            child,
            _reader: reader,
            cfg_path,
            _temp_dir: temp_dir,
        })
    }

    pub fn latest_bars(&self) -> Vec<f32> {
        let l = self.left.lock().unwrap().clone();
        let r = self.right.lock().unwrap().clone();
        if self.channels == CavaChannels::Mono {
            return l;
        }
        let mut out = vec![0.0f32; l.len()];
        for i in 0..l.len().min(r.len()) {
            out[i] = ((l[i] + r[i]) * 0.5).clamp(0.0, 1.0);
        }
        out
    }

    pub fn latest_stereo_bars(&self) -> (Vec<f32>, Vec<f32>) {
        (self.left.lock().unwrap().clone(), self.right.lock().unwrap().clone())
    }
}

fn find_cava_executable() -> Option<PathBuf> {
    // Resolution order:
    // 1) env var override
    // 2) bundled next to our executable or in ./third_party/cava/
    // 3) PATH fallback
    if let Some(p) = std::env::var_os("CLI_MUSIC_PLAYER_CAVA") {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Some(p);
        }
    }

    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            candidates.push(exe_dir.join("cava"));
            candidates.push(exe_dir.join("third_party").join("cava").join("cava"));
        }
    }

    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join("third_party").join("cava").join("cava"));
    }

    for p in candidates {
        if p.is_file() {
            return Some(p);
        }
    }

    if which_in_path("cava").is_some() {
        return Some(PathBuf::from("cava"));
    }

    None
}

fn resolve_cava_executable() -> Result<(PathBuf, Option<TempDir>)> {
    if let Some(p) = find_cava_executable() {
        return Ok((p, None));
    }

    #[cfg(feature = "bundle-cava")]
    {
        let temp_dir = tempfile::Builder::new()
            .prefix("cli-music-player-cava-")
            .tempdir()
            .context("create temp dir for cava")?;
        let path = temp_dir.path().join("cava");
        fs::write(&path, embedded_cava_bytes()).with_context(|| format!("write temp cava: {}", path.display()))?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&path)?.permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&path, perms)?;
        }

        return Ok((path, Some(temp_dir)));
    }

    Err(anyhow::anyhow!("cava not found"))
}

fn which_in_path(bin: &str) -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    for p in std::env::split_paths(&paths) {
        let cand = p.join(bin);
        if cand.is_file() {
            return Some(cand);
        }
    }
    None
}

#[cfg(feature = "bundle-cava")]
fn embedded_cava_bytes() -> &'static [u8] {
    include_bytes!(concat!(env!("OUT_DIR"), "/cava.bin"))
}

impl Drop for CavaRunner {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = fs::remove_file(&self.cfg_path);
    }
}

fn parse_frames_ascii(s: &str, bars: usize) -> Vec<Vec<f32>> {
    // ascii_max_range=1000, bar_delimiter=';'
    // Can be N values (one channel) or 2N values (two channels) on a single line.
    let mut vals: Vec<f32> = Vec::new();
    for part in s.split(|c: char| c == ';' || c == '\n' || c == '\r' || c == ' ' || c == '\t') {
        if part.is_empty() {
            continue;
        }
        if let Ok(v) = part.parse::<u32>() {
            vals.push((v as f32 / 1000.0).clamp(0.0, 1.0));
        }
    }

    if bars == 0 {
        return Vec::new();
    }

    let mut out: Vec<Vec<f32>> = Vec::new();
    let mut idx = 0usize;
    while idx + bars <= vals.len() {
        let mut frame = vec![0.0f32; bars];
        for i in 0..bars {
            frame[i] = vals[idx + i];
        }
        out.push(frame);
        idx += bars;
    }

    out
}

fn temp_cfg_path() -> String {
    let pid = std::process::id();
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    format!("/tmp/cli-music-player-cava-{pid}-{ts}.conf")
}
