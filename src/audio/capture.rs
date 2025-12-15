use anyhow::Result;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub struct AudioCapture {
    samples: Arc<Mutex<Vec<f32>>>,
    last_sample_at: Arc<Mutex<Option<Instant>>>,
    last_restart_at: Instant,
    _stream: cpal::Stream,
}

impl AudioCapture {
    pub fn start() -> Result<Self> {
        Self::build()
    }

    pub fn maybe_restart_for_system_playback(&mut self, now: Instant) {
        // If system monitor is playing but we haven't received samples recently,
        // try re-selecting the best input device (monitor/loopback may appear later).
        // Throttle restarts to avoid flapping.
        let since_restart = now.duration_since(self.last_restart_at);
        if since_restart < Duration::from_secs(2) {
            return;
        }

        let stale = match self.last_sample_age(now) {
            None => true,
            Some(age) => age > Duration::from_millis(900),
        };

        if stale {
            log::info!("cpal capture stale; restarting");
            if let Ok(newcap) = Self::build() {
                *self = newcap;
            }
        }
    }

    pub fn last_sample_age(&self, now: Instant) -> Option<Duration> {
        let guard = self.last_sample_at.lock().unwrap();
        guard.as_ref().map(|t| now.duration_since(*t))
    }

    fn build() -> Result<Self> {
        let device = pick_best_input_device_any_host().or_else(|| {
            let host = cpal::default_host();
            pick_best_input_device(&host).or_else(|| host.default_input_device())
        });
        let Some(device) = device else {
            // no device: still create empty capture
            let dummy = Arc::new(Mutex::new(Vec::new()));
            let last_sample_at = Arc::new(Mutex::new(None));
            let (_stream, _rx) = dummy_stream()?;
            return Ok(Self {
                samples: dummy,
                last_sample_at,
                last_restart_at: Instant::now(),
                _stream,
            });
        };

        if let Ok(name) = device.name() {
            log::info!("cpal input device: {name}");
        }

        let config = device.default_input_config()?;
        let samples = Arc::new(Mutex::new(Vec::with_capacity(8192)));
        let samples_cloned = Arc::clone(&samples);

        let last_sample_at = Arc::new(Mutex::new(None));
        let last_sample_cloned = Arc::clone(&last_sample_at);

        let err_fn = |err| {
            log::warn!("cpal stream error: {err}");
        };

        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => device.build_input_stream(
                &config.into(),
                move |data: &[f32], _| push_samples(&samples_cloned, &last_sample_cloned, data),
                err_fn,
                None,
            )?,
            cpal::SampleFormat::I16 => device.build_input_stream(
                &config.into(),
                move |data: &[i16], _| push_samples_i16(&samples_cloned, &last_sample_cloned, data),
                err_fn,
                None,
            )?,
            cpal::SampleFormat::U16 => device.build_input_stream(
                &config.into(),
                move |data: &[u16], _| push_samples_u16(&samples_cloned, &last_sample_cloned, data),
                err_fn,
                None,
            )?,
            _ => {
                let dummy = Arc::new(Mutex::new(Vec::new()));
                let (_stream, _) = dummy_stream()?;
                return Ok(Self {
                    samples: dummy,
                    last_sample_at,
                    last_restart_at: Instant::now(),
                    _stream,
                });
            }
        };

        stream.play()?;
        Ok(Self {
            samples,
            last_sample_at,
            last_restart_at: Instant::now(),
            _stream: stream,
        })
    }

    pub fn latest_samples(&self, n: usize) -> Vec<f32> {
        let guard = self.samples.lock().unwrap();
        if guard.len() <= n {
            return guard.clone();
        }
        guard[guard.len() - n..].to_vec()
    }
}

fn push_samples(buf: &Arc<Mutex<Vec<f32>>>, last_sample_at: &Arc<Mutex<Option<Instant>>>, data: &[f32]) {
    let mut guard = buf.lock().unwrap();
    guard.extend_from_slice(data);

    if !data.is_empty() {
        let mut t = last_sample_at.lock().unwrap();
        *t = Some(Instant::now());
    }

    // keep last ~16384 samples
    const CAP: usize = 16384;
    if guard.len() > CAP {
        let drop = guard.len() - CAP;
        guard.drain(0..drop);
    }
}

fn push_samples_i16(
    buf: &Arc<Mutex<Vec<f32>>>,
    last_sample_at: &Arc<Mutex<Option<Instant>>>,
    data: &[i16],
) {
    let mut guard = buf.lock().unwrap();
    guard.reserve(data.len());
    for &s in data {
        guard.push(s as f32 / i16::MAX as f32);
    }

    if !data.is_empty() {
        let mut t = last_sample_at.lock().unwrap();
        *t = Some(Instant::now());
    }

    // keep last ~16384 samples
    const CAP: usize = 16384;
    if guard.len() > CAP {
        let drop = guard.len() - CAP;
        guard.drain(0..drop);
    }
}

fn push_samples_u16(
    buf: &Arc<Mutex<Vec<f32>>>,
    last_sample_at: &Arc<Mutex<Option<Instant>>>,
    data: &[u16],
) {
    let mut guard = buf.lock().unwrap();
    guard.reserve(data.len());
    for &s in data {
        guard.push((s as f32 / u16::MAX as f32) * 2.0 - 1.0);
    }

    if !data.is_empty() {
        let mut t = last_sample_at.lock().unwrap();
        *t = Some(Instant::now());
    }

    // keep last ~16384 samples
    const CAP: usize = 16384;
    if guard.len() > CAP {
        let drop = guard.len() - CAP;
        guard.drain(0..drop);
    }
}

fn pick_best_input_device(host: &cpal::Host) -> Option<cpal::Device> {
    // Prefer loopback/monitor devices for capturing system audio.
    // Avoid typical mic devices when possible.
    let devices = host.input_devices().ok()?;
    let mut monitor: Option<cpal::Device> = None;
    let mut other: Option<cpal::Device> = None;

    for d in devices {
        let name = d.name().unwrap_or_default();
        let lname = name.to_lowercase();

        let looks_like_mic = lname.contains("mic") || lname.contains("microphone") || lname.contains("input") && !lname.contains("monitor");
        let looks_like_monitor = lname.contains("monitor") || lname.contains("loopback") || lname.contains("stereo mix") || lname.contains("what u hear") || lname.contains("mix");

        if looks_like_monitor {
            monitor = Some(d);
            continue;
        }
        if !looks_like_mic {
            other = Some(d);
        }
    }

    monitor.or(other)
}

fn pick_best_input_device_any_host() -> Option<cpal::Device> {
    // On Linux, CPAL may expose multiple backends (ALSA / JACK / PipeWire, etc.).
    // We try all available hosts and pick the highest-scoring device.
    let mut best: Option<(i32, cpal::Device)> = None;

    for host_id in cpal::available_hosts() {
        let Ok(host) = cpal::host_from_id(host_id) else {
            continue;
        };

        let host_bonus = {
            let s = format!("{host_id:?}").to_lowercase();
            if s.contains("pipe") {
                50
            } else if s.contains("pulse") {
                40
            } else if s.contains("jack") {
                10
            } else {
                0
            }
        };

        let Ok(devices) = host.input_devices() else {
            continue;
        };

        for d in devices {
            let name = d.name().unwrap_or_default();
            let score = host_bonus + device_name_score(&name);
            match &best {
                None => best = Some((score, d)),
                Some((best_score, _)) if score > *best_score => best = Some((score, d)),
                _ => {}
            }
        }
    }

    best.map(|(_, d)| d)
}

fn device_name_score(name: &str) -> i32 {
    let lname = name.to_lowercase();

    let mut score = 0;

    // strong positives for loopback/monitor
    if lname.contains("monitor") {
        score += 200;
    }
    if lname.contains("loopback") {
        score += 160;
    }
    if lname.contains("stereo mix") || lname.contains("what u hear") || lname.contains("mix") {
        score += 120;
    }

    // prefer outputs/virtual sinks (pipewire/pulse naming)
    if lname.contains("output") {
        score += 40;
    }
    if lname.contains("sink") {
        score += 30;
    }

    // negatives for mic-like devices
    if lname.contains("microphone") || lname.contains("mic") {
        score -= 120;
    }
    if lname.contains("headset") {
        score -= 50;
    }
    if lname.contains("webcam") || lname.contains("camera") {
        score -= 80;
    }

    // mild penalty for generic "input" unless it also looks like monitor
    if lname.contains("input") && !lname.contains("monitor") {
        score -= 40;
    }

    score
}

fn dummy_stream() -> Result<(cpal::Stream, ())> {
    // create a no-op output stream if possible; fall back to panic avoided by using a minimal input stream impossible.
    // Here we use default host output with silent callback if available.
    let host = cpal::default_host();
    let device = host.default_output_device().ok_or_else(|| anyhow::anyhow!("no audio device"))?;
    let config = device.default_output_config()?;
    let err_fn = |err| {
        log::warn!("cpal dummy stream error: {err}");
    };

    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => device.build_output_stream(&config.into(), move |out: &mut [f32], _| {
            for s in out.iter_mut() { *s = 0.0; }
        }, err_fn, None)?,
        cpal::SampleFormat::I16 => device.build_output_stream(&config.into(), move |out: &mut [i16], _| {
            for s in out.iter_mut() { *s = 0; }
        }, err_fn, None)?,
        cpal::SampleFormat::U16 => device.build_output_stream(&config.into(), move |out: &mut [u16], _| {
            for s in out.iter_mut() { *s = u16::MAX / 2; }
        }, err_fn, None)?,
        _ => device.build_output_stream(&config.into(), move |_out: &mut [f32], _| {}, err_fn, None)?,
    };
    let _ = stream.play();
    Ok((stream, ()))
}
