//! Delay effect — stereo delay line with feedback and mix control.

/// A stereo delay effect with configurable time, feedback, and dry/wet mix.
///
/// The delay buffer can hold up to `max_delay_seconds` of audio at the given
/// sample rate. The actual delay time can be changed dynamically.
#[derive(Debug, Clone)]
pub struct Delay {
    buffer_l: Vec<f32>,
    buffer_r: Vec<f32>,
    write_pos: usize,
    sample_rate: f64,

    /// Delay time in seconds.
    pub delay_time: f64,
    /// Feedback amount (0.0 = no feedback, 1.0 = infinite feedback).
    pub feedback: f64,
    /// Dry/wet mix (0.0 = fully dry, 1.0 = fully wet).
    pub mix: f64,
}

impl Delay {
    /// Create a new delay effect.
    ///
    /// # Arguments
    /// - `sample_rate`: Audio sample rate in Hz.
    /// - `max_delay_seconds`: Maximum supported delay time.
    pub fn new(sample_rate: f64, max_delay_seconds: f64) -> Self {
        let buffer_size = (sample_rate * max_delay_seconds) as usize + 1;
        Self {
            buffer_l: vec![0.0; buffer_size],
            buffer_r: vec![0.0; buffer_size],
            write_pos: 0,
            sample_rate,
            delay_time: 0.5,
            feedback: 0.3,
            mix: 0.5,
        }
    }

    /// Create a delay with specific parameters.
    pub fn with_params(sample_rate: f64, max_delay_seconds: f64, delay_time: f64, feedback: f64, mix: f64) -> Self {
        let mut d = Self::new(sample_rate, max_delay_seconds);
        d.delay_time = delay_time.clamp(0.0, max_delay_seconds);
        d.feedback = feedback.clamp(0.0, 0.99);
        d.mix = mix.clamp(0.0, 1.0);
        d
    }

    /// Process a stereo sample pair, returning the processed output.
    #[inline]
    pub fn process(&mut self, left: f32, right: f32) -> (f32, f32) {
        let buffer_len = self.buffer_l.len();
        let delay_samples = (self.delay_time * self.sample_rate) as usize;
        let delay_samples = delay_samples.min(buffer_len - 1);

        // Calculate read position
        let read_pos = if self.write_pos >= delay_samples {
            self.write_pos - delay_samples
        } else {
            buffer_len - (delay_samples - self.write_pos)
        };

        // Read delayed samples
        let delayed_l = self.buffer_l[read_pos];
        let delayed_r = self.buffer_r[read_pos];

        // Write input + feedback to buffer
        let feedback_l = left + delayed_l * self.feedback as f32;
        let feedback_r = right + delayed_r * self.feedback as f32;
        self.buffer_l[self.write_pos] = feedback_l;
        self.buffer_r[self.write_pos] = feedback_r;

        // Advance write position
        self.write_pos = (self.write_pos + 1) % buffer_len;

        // Mix dry/wet
        let mix = self.mix as f32;
        let out_l = left * (1.0 - mix) + delayed_l * mix;
        let out_r = right * (1.0 - mix) + delayed_r * mix;

        (out_l, out_r)
    }

    /// Process a block of stereo audio in-place.
    pub fn process_block(&mut self, left: &mut [f32], right: &mut [f32]) {
        for i in 0..left.len().min(right.len()) {
            let (out_l, out_r) = self.process(left[i], right[i]);
            left[i] = out_l;
            right[i] = out_r;
        }
    }

    /// Clear the delay buffers.
    pub fn clear(&mut self) {
        self.buffer_l.fill(0.0);
        self.buffer_r.fill(0.0);
        self.write_pos = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_delay_passthrough_when_dry() {
        let mut delay = Delay::with_params(44100.0, 2.0, 0.5, 0.0, 0.0);
        
        // With mix = 0, output should equal input
        let (out_l, out_r) = delay.process(0.5, -0.5);
        assert!((out_l - 0.5).abs() < 1e-6);
        assert!((out_r - (-0.5)).abs() < 1e-6);
    }

    #[test]
    fn test_delay_outputs_delayed_signal() {
        let sample_rate = 44100.0;
        let delay_time = 0.01; // 10ms = 441 samples
        let mut delay = Delay::with_params(sample_rate, 1.0, delay_time, 0.0, 1.0);
        
        // Send an impulse
        delay.process(1.0, 1.0);
        
        // Process samples until we hit the delay time
        let delay_samples = (delay_time * sample_rate) as usize;
        for _ in 1..delay_samples {
            let (out_l, _) = delay.process(0.0, 0.0);
            // Before delay time, output should be 0 (wet only)
            assert!(out_l.abs() < 1e-6);
        }
        
        // At delay time, the impulse should appear
        let (out_l, out_r) = delay.process(0.0, 0.0);
        assert!((out_l - 1.0).abs() < 1e-6);
        assert!((out_r - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_delay_feedback() {
        let sample_rate = 1000.0; // Simple sample rate for testing
        let delay_time = 0.01; // 10 samples
        let feedback = 0.5;
        let mut delay = Delay::with_params(sample_rate, 1.0, delay_time, feedback, 1.0);
        
        // Send impulse
        delay.process(1.0, 1.0);
        
        // Wait for first echo
        let delay_samples = (delay_time * sample_rate) as usize;
        for _ in 1..delay_samples {
            delay.process(0.0, 0.0);
        }
        
        // First echo
        let (first_echo, _) = delay.process(0.0, 0.0);
        assert!((first_echo - 1.0).abs() < 1e-6);
        
        // Wait for second echo
        for _ in 1..delay_samples {
            delay.process(0.0, 0.0);
        }
        
        // Second echo should be attenuated by feedback
        let (second_echo, _) = delay.process(0.0, 0.0);
        assert!((second_echo - 0.5).abs() < 1e-6);
    }
}
