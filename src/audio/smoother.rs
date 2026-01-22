#[derive(Debug, Clone, Copy)]
pub struct Ema {
    alpha: f32,
    state: [f32; 64],
}

impl Ema {
    pub fn new(alpha: f32) -> Self {
        Self { alpha, state: [0.0; 64] }
    }

    pub fn apply(&mut self, input: [f32; 64]) -> [f32; 64] {
        for i in 0..64 {
            self.state[i] = self.alpha * input[i] + (1.0 - self.alpha) * self.state[i];
        }
        self.state
    }
}
