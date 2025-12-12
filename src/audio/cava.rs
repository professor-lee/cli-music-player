use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct CavaRunner {
    bars: Arc<Mutex<[f32; 64]>>,
    child: Child,
    _reader: thread::JoinHandle<()>,
    cfg_path: String,
}

impl CavaRunner {
    pub fn start(framerate_hz: u32) -> Result<Self> {
        // Minimal config we generate ourselves (do not copy upstream example config).
        // Uses raw ascii output to stdout, 64 mono bars, newline-delimited frames.
        let framerate_hz = framerate_hz.clamp(10, 120);
        let cfg = format!(
            "[general]\nframerate = {fr}\nbars = 64\n\n[input]\n# Leave method/source unset: cava will pick the best supported backend (pipewire/pulse/etc).\n\n[output]\nmethod = raw\nchannels = mono\nmono_option = average\nraw_target = /dev/stdout\ndata_format = ascii\nascii_max_range = 1000\nbar_delimiter = 59\nframe_delimiter = 10\n",
            fr = framerate_hz
        );

        let cfg_path = temp_cfg_path();
        fs::write(&cfg_path, cfg).with_context(|| format!("write cava config: {cfg_path}"))?;

        let cava_exe = find_cava_executable();
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

        let bars: Arc<Mutex<[f32; 64]>> = Arc::new(Mutex::new([0.0; 64]));
        let bars_cloned = Arc::clone(&bars);

        let reader = thread::spawn(move || {
            let mut br = BufReader::new(stdout);
            let mut line = String::new();
            loop {
                line.clear();
                match br.read_line(&mut line) {
                    Ok(0) => break, // EOF
                    Ok(_) => {
                        if let Some(frame) = parse_frame_ascii(&line) {
                            let mut guard = bars_cloned.lock().unwrap();
                            *guard = frame;
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        Ok(Self {
            bars,
            child,
            _reader: reader,
            cfg_path,
        })
    }

    pub fn latest_bars(&self) -> [f32; 64] {
        *self.bars.lock().unwrap()
    }
}

fn find_cava_executable() -> PathBuf {
    // Resolution order:
    // 1) env var override
    // 2) bundled next to our executable or in ./third_party/cava/
    // 3) PATH fallback
    if let Some(p) = std::env::var_os("CLI_MUSIC_PLAYER_CAVA") {
        let p = PathBuf::from(p);
        if p.is_file() {
            return p;
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
            return p;
        }
    }

    PathBuf::from("cava")
}

impl Drop for CavaRunner {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = fs::remove_file(&self.cfg_path);
    }
}

fn parse_frame_ascii(s: &str) -> Option<[f32; 64]> {
    // ascii_max_range=1000, bar_delimiter=';'
    let mut out = [0.0f32; 64];
    let mut idx = 0usize;

    for part in s.split(|c: char| c == ';' || c == '\n' || c == '\r' || c == ' ' || c == '\t') {
        if part.is_empty() {
            continue;
        }
        let v: u32 = part.parse().ok()?;
        let v = (v as f32 / 1000.0).clamp(0.0, 1.0);
        if idx < 64 {
            out[idx] = v;
            idx += 1;
        } else {
            break;
        }
    }

    if idx == 64 {
        Some(out)
    } else {
        None
    }
}

fn temp_cfg_path() -> String {
    let pid = std::process::id();
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    format!("/tmp/cli-music-player-cava-{pid}-{ts}.conf")
}
