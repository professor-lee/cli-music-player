use crate::audio::fft::FftEngine;
use crate::audio::smoother::Ema;

pub struct SpectrumProcessor {
    _hz: u32,
    fft: FftEngine,
    smooth: Ema,
}

impl SpectrumProcessor {
    pub fn new(hz: u32, fft_size: usize) -> Self {
        Self {
            _hz: hz,
            fft: FftEngine::new(fft_size),
            smooth: Ema::new(0.30),
        }
    }

    pub fn process(&mut self, samples: &[f32]) -> [f32; 64] {
        let mags = self.fft.magnitudes(samples);
        let grouped = group_linear(mags);
        let scaled = log_scale(grouped);
        let smoothed = self.smooth.apply(scaled);
        normalize(smoothed)
    }
}

fn group_linear(mags: &[f32]) -> [f32; 64] {
    let mut out = [0.0f32; 64];
    if mags.is_empty() {
        return out;
    }
    let bin = mags.len() / 64.max(1);
    for i in 0..64 {
        let start = i * bin;
        let end = if i == 63 { mags.len() } else { ((i + 1) * bin).min(mags.len()) };
        let mut sum = 0.0;
        let mut n = 0;
        for &v in &mags[start..end] {
            sum += v;
            n += 1;
        }
        out[i] = if n > 0 { sum / n as f32 } else { 0.0 };
    }
    out
}

fn log_scale(mut x: [f32; 64]) -> [f32; 64] {
    for v in x.iter_mut() {
        *v = (1.0 + *v).log10() * 10.0;
    }
    x
}

fn normalize(mut x: [f32; 64]) -> [f32; 64] {
    let mut maxv = 1e-6;
    for &v in &x {
        if v > maxv {
            maxv = v;
        }
    }
    for v in x.iter_mut() {
        *v = (*v / maxv).clamp(0.0, 1.0);
    }
    x
}
