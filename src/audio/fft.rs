use rustfft::{FftPlanner, num_complex::Complex};

pub struct FftEngine {
    fft_size: usize,
    window: Vec<f32>,
    fft: std::sync::Arc<dyn rustfft::Fft<f32>>,
    buf: Vec<Complex<f32>>,
    mags: Vec<f32>,
}

impl FftEngine {
    pub fn new(fft_size: usize) -> Self {
        let mut planner = FftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(fft_size);
        let window = hann_window(fft_size);
        let buf = vec![Complex::new(0.0, 0.0); fft_size];
        let mags = vec![0.0f32; fft_size / 2];
        Self { fft_size, window, fft, buf, mags }
    }

    pub fn magnitudes(&mut self, input: &[f32]) -> &[f32] {
        // window + copy into reusable complex buffer
        for i in 0..self.fft_size {
            let x = input.get(i).copied().unwrap_or(0.0) * self.window[i];
            self.buf[i] = Complex::new(x, 0.0);
        }

        self.fft.process(&mut self.buf);

        // take first half magnitudes into reusable buffer
        let half = self.fft_size / 2;
        if self.mags.len() != half {
            self.mags.resize(half, 0.0);
        }
        for i in 0..half {
            self.mags[i] = self.buf[i].norm();
        }
        &self.mags
    }
}

fn hann_window(n: usize) -> Vec<f32> {
    let mut w = vec![0.0; n];
    for i in 0..n {
        w[i] = 0.5 - 0.5 * ((2.0 * std::f32::consts::PI * i as f32) / (n as f32)).cos();
    }
    w
}
