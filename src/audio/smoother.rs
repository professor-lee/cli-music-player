#[derive(Debug, Clone)]
pub struct Ema {
    alpha: f32,
    state: Vec<f32>,
}

impl Ema {
    pub fn new(alpha: f32, len: usize) -> Self {
        Self { alpha, state: vec![0.0; len] }
    }

    pub fn apply(&mut self, input: &[f32]) -> Vec<f32> {
        if self.state.len() != input.len() {
            self.state = vec![0.0; input.len()];
        }
        for i in 0..input.len() {
            self.state[i] = self.alpha * input[i] + (1.0 - self.alpha) * self.state[i];
        }
        self.state.clone()
    }
}
