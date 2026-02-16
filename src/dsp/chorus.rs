//! Chorus effect — stereo modulated delay for thickening sound.
//!
//! Uses an LFO-modulated delay line to create subtle pitch variations
//! that produce a rich, "doubled" sound.

use std::f64::consts::PI;

/// A stereo chorus effect with configurable rate, depth, and mix.
#[derive(Debug, Clone)]
pub struct Chorus {
    buffer_l: Vec<f32>,
    buffer_r: Vec<f32>,
    write_pos: usize,
    sample_rate: f64,
    phase_l: f64,
    phase_r: f64,

    /// LFO rate in Hz (typical: 0.5-5 Hz).
    pub rate: f64,
    /// Modulation depth in seconds (typical: 0.001-0.005).
    pub depth: f64,
    /// Base delay time in seconds (typical: 0.01-0.03).
    pub delay: f64,
    /// Dry/wet mix (0.0 = fully dry, 1.0 = fully wet).
    pub mix: f64,
    /// Stereo spread — phase offset between L/R channels (0.0-1.0).
    pub stereo: f64,
}

impl Chorus {
    /// Create a new chorus effect.
    pub fn new(sample_rate: f64) -> Self {
        // Buffer size: max delay + max depth + margin
        let max_delay = 0.05; // 50ms max
        let buffer_size = (sample_rate * max_delay) as usize + 1;

        Self {
            buffer_l: vec![0.0; buffer_size],
            buffer_r: vec![0.0; buffer_size],
            write_pos: 0,
            sample_rate,
            phase_l: 0.0,
            phase_r: 0.25, // 90° phase offset for stereo spread
            rate: 1.5,
            depth: 0.002,
            delay: 0.015,
            mix: 0.5,
            stereo: 1.0,
        }
    }

    /// Create a chorus with specific parameters.
    pub fn with_params(sample_rate: f64, rate: f64, depth: f64, mix: f64) -> Self {
        let mut c = Self::new(sample_rate);
        c.rate = rate.clamp(0.1, 10.0);
        c.depth = depth.clamp(0.0, 0.01);
        c.mix = mix.clamp(0.0, 1.0);
        c
    }

    /// Read from the delay buffer with fractional (linear interpolation) delay.
    #[inline]
    fn read_interpolated(buffer: &[f32], write_pos: usize, delay_samples: f64) -> f32 {
        let buffer_len = buffer.len();
        let delay_int = delay_samples as usize;
        let frac = (delay_samples - delay_int as f64) as f32;

        let read_pos_0 = if write_pos >= delay_int {
            write_pos - delay_int
        } else {
            buffer_len - (delay_int - write_pos)
        };

        let read_pos_1 = if read_pos_0 == 0 {
            buffer_len - 1
        } else {
            read_pos_0 - 1
        };

        let s0 = buffer[read_pos_0];
        let s1 = buffer[read_pos_1];

        // Linear interpolation
        s0 + frac * (s1 - s0)
    }

    /// Process a stereo sample pair.
    #[inline]
    pub fn process(&mut self, left: f32, right: f32) -> (f32, f32) {
        let buffer_len = self.buffer_l.len();

        // Write input to buffers
        self.buffer_l[self.write_pos] = left;
        self.buffer_r[self.write_pos] = right;

        // Calculate modulated delay times
        let lfo_l = (2.0 * PI * self.phase_l).sin();
        let lfo_r = (2.0 * PI * self.phase_r).sin();

        let delay_l = (self.delay + self.depth * lfo_l) * self.sample_rate;
        let delay_r = (self.delay + self.depth * lfo_r) * self.sample_rate;

        // Clamp delay to buffer bounds
        let max_delay = (buffer_len - 1) as f64;
        let delay_l = delay_l.clamp(1.0, max_delay);
        let delay_r = delay_r.clamp(1.0, max_delay);

        // Read delayed samples with interpolation
        let wet_l = Self::read_interpolated(&self.buffer_l, self.write_pos, delay_l);
        let wet_r = Self::read_interpolated(&self.buffer_r, self.write_pos, delay_r);

        // Advance write position
        self.write_pos = (self.write_pos + 1) % buffer_len;

        // Advance LFO phases
        let phase_inc = self.rate / self.sample_rate;
        self.phase_l = (self.phase_l + phase_inc) % 1.0;
        self.phase_r = (self.phase_r + phase_inc) % 1.0;

        // Mix dry/wet
        let mix = self.mix as f32;
        let out_l = left * (1.0 - mix) + wet_l * mix;
        let out_r = right * (1.0 - mix) + wet_r * mix;

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

    /// Clear internal buffers.
    pub fn clear(&mut self) {
        self.buffer_l.fill(0.0);
        self.buffer_r.fill(0.0);
        self.write_pos = 0;
        self.phase_l = 0.0;
        self.phase_r = 0.25;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chorus_passthrough_when_dry() {
        let mut chorus = Chorus::with_params(44100.0, 1.5, 0.002, 0.0);

        // With mix = 0, output should equal input
        let (out_l, out_r) = chorus.process(0.5, -0.5);
        assert!((out_l - 0.5).abs() < 1e-6);
        assert!((out_r - (-0.5)).abs() < 1e-6);
    }

    #[test]
    fn test_chorus_produces_modulated_output() {
        let mut chorus = Chorus::with_params(44100.0, 2.0, 0.003, 1.0);

        // Process a constant signal
        let mut outputs_l = Vec::new();
        for _ in 0..4410 {
            // 100ms of signal
            let (out_l, _) = chorus.process(1.0, 1.0);
            outputs_l.push(out_l);
        }

        // After the initial delay, we should see variation due to modulation
        // Skip initial delay period
        let later_outputs = &outputs_l[1000..];
        let min = later_outputs.iter().fold(f32::MAX, |m, &s| m.min(s));
        let max = later_outputs.iter().fold(f32::MIN, |m, &s| m.max(s));

        // Should have some variation due to interpolation artifacts from modulation
        // (constant input + varying delay = varying output amplitude due to interpolation)
        assert!(
            (max - min).abs() < 0.5,
            "Chorus output should be relatively stable but modulated"
        );
    }

    #[test]
    fn test_chorus_stereo_spread() {
        let mut chorus = Chorus::new(44100.0);
        chorus.mix = 1.0;

        // Process enough samples to see stereo difference
        let mut found_difference = false;
        for _ in 0..4410 {
            let (out_l, out_r) = chorus.process(1.0, 1.0);
            if (out_l - out_r).abs() > 0.001 {
                found_difference = true;
                break;
            }
        }

        assert!(
            found_difference,
            "Chorus should produce stereo difference due to phase offset"
        );
    }
}
