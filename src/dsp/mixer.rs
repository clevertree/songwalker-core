//! Mixer — Sums multiple voice outputs with master gain.

/// A simple summing mixer that accumulates audio from multiple sources.
#[derive(Debug, Clone)]
pub struct Mixer {
    pub master_gain: f64,
    buffer: Vec<f64>,
}

impl Mixer {
    pub fn new() -> Self {
        Mixer {
            master_gain: 0.8,
            buffer: Vec::new(),
        }
    }

    /// Prepare a buffer of `num_samples` filled with zeros.
    pub fn clear(&mut self, num_samples: usize) {
        self.buffer.clear();
        self.buffer.resize(num_samples, 0.0);
    }

    /// Add a sample at the given index.
    pub fn add(&mut self, index: usize, sample: f64) {
        if index < self.buffer.len() {
            self.buffer[index] += sample;
        }
    }

    /// Get the mixed output buffer, with master gain and soft clipping applied.
    pub fn output(&self) -> Vec<f64> {
        self.buffer
            .iter()
            .map(|&s| soft_clip(s * self.master_gain))
            .collect()
    }

    /// Access the raw buffer length.
    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    /// Is the buffer empty?
    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }
}

/// Soft clipper using tanh to prevent harsh digital clipping.
fn soft_clip(x: f64) -> f64 {
    x.tanh()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_buffer() {
        let mut m = Mixer::new();
        m.clear(128);
        let out = m.output();
        assert_eq!(out.len(), 128);
        assert!(out.iter().all(|&s| s == 0.0));
    }

    #[test]
    fn accumulates_samples() {
        let mut m = Mixer::new();
        m.master_gain = 1.0;
        m.clear(4);
        m.add(0, 0.5);
        m.add(0, 0.3);
        m.add(1, 1.0);
        let out = m.output();
        assert!((out[0] - soft_clip(0.8)).abs() < 1e-10);
        assert!((out[1] - soft_clip(1.0)).abs() < 1e-10);
        assert!((out[2] - 0.0).abs() < 1e-10);
    }

    #[test]
    fn soft_clip_prevents_overflow() {
        let mut m = Mixer::new();
        m.master_gain = 1.0;
        m.clear(1);
        m.add(0, 100.0);
        let out = m.output();
        assert!(
            out[0].abs() <= 1.0,
            "Soft clip should keep output <= 1.0, got {}",
            out[0]
        );
    }
}
