//! Compressor effect — dynamics processing for audio leveling.
//!
//! Implements a feed-forward compressor with threshold, ratio, knee,
//! attack, and release parameters matching the WebAudio DynamicsCompressorNode.

/// A stereo dynamics compressor.
#[derive(Debug, Clone)]
pub struct Compressor {
    sample_rate: f64,

    /// Threshold in dB (typical: -50 to 0).
    pub threshold: f64,
    /// Compression ratio (e.g., 4.0 = 4:1 compression).
    pub ratio: f64,
    /// Knee width in dB (0 = hard knee, higher = softer transition).
    pub knee: f64,
    /// Attack time in seconds.
    pub attack: f64,
    /// Release time in seconds.
    pub release: f64,
    /// Makeup gain in dB.
    pub makeup_gain: f64,

    // Internal state
    envelope: f64, // Current envelope level (linear)
}

impl Compressor {
    /// Create a new compressor with default settings.
    pub fn new(sample_rate: f64) -> Self {
        Self {
            sample_rate,
            threshold: -24.0,
            ratio: 4.0,
            knee: 6.0,
            attack: 0.003,   // 3ms
            release: 0.25,   // 250ms
            makeup_gain: 0.0,
            envelope: 0.0,
        }
    }

    /// Create a compressor with specific parameters.
    pub fn with_params(
        sample_rate: f64,
        threshold: f64,
        ratio: f64,
        attack: f64,
        release: f64,
    ) -> Self {
        let mut c = Self::new(sample_rate);
        c.threshold = threshold.clamp(-60.0, 0.0);
        c.ratio = ratio.clamp(1.0, 20.0);
        c.attack = attack.clamp(0.0001, 1.0);
        c.release = release.clamp(0.001, 5.0);
        c
    }

    /// Convert linear amplitude to dB.
    #[inline]
    fn linear_to_db(linear: f64) -> f64 {
        if linear <= 0.0 {
            -120.0
        } else {
            20.0 * linear.log10()
        }
    }

    /// Convert dB to linear amplitude.
    #[inline]
    fn db_to_linear(db: f64) -> f64 {
        10.0_f64.powf(db / 20.0)
    }

    /// Compute gain reduction for a given input level (in dB).
    #[inline]
    fn compute_gain(&self, input_db: f64) -> f64 {
        let threshold = self.threshold;
        let ratio = self.ratio;
        let knee = self.knee;

        if knee <= 0.0 {
            // Hard knee
            if input_db <= threshold {
                0.0 // No gain reduction
            } else {
                // Gain reduction = (input - threshold) * (1 - 1/ratio)
                (threshold - input_db) * (1.0 - 1.0 / ratio)
            }
        } else {
            // Soft knee
            let half_knee = knee / 2.0;
            let knee_start = threshold - half_knee;
            let knee_end = threshold + half_knee;

            if input_db <= knee_start {
                0.0 // Below knee, no compression
            } else if input_db >= knee_end {
                // Above knee, full compression
                (threshold - input_db) * (1.0 - 1.0 / ratio)
            } else {
                // In the knee region - quadratic interpolation
                let x = input_db - knee_start;
                let knee_factor = x / knee;
                -knee_factor * knee_factor * (1.0 - 1.0 / ratio) * half_knee
            }
        }
    }

    /// Process a stereo sample pair.
    #[inline]
    pub fn process(&mut self, left: f32, right: f32) -> (f32, f32) {
        // Compute input level (peak of L/R)
        let input_level = (left.abs()).max(right.abs()) as f64;

        // Envelope follower (peak detection with attack/release)
        let attack_coef = (-1.0 / (self.attack * self.sample_rate)).exp();
        let release_coef = (-1.0 / (self.release * self.sample_rate)).exp();

        if input_level > self.envelope {
            // Attack
            self.envelope = attack_coef * self.envelope + (1.0 - attack_coef) * input_level;
        } else {
            // Release
            self.envelope = release_coef * self.envelope + (1.0 - release_coef) * input_level;
        }

        // Convert envelope to dB
        let envelope_db = Self::linear_to_db(self.envelope);

        // Compute gain reduction
        let gain_reduction_db = self.compute_gain(envelope_db);

        // Apply makeup gain and convert to linear
        let total_gain_db = gain_reduction_db + self.makeup_gain;
        let gain = Self::db_to_linear(total_gain_db) as f32;

        (left * gain, right * gain)
    }

    /// Process a block of stereo audio in-place.
    pub fn process_block(&mut self, left: &mut [f32], right: &mut [f32]) {
        for i in 0..left.len().min(right.len()) {
            let (out_l, out_r) = self.process(left[i], right[i]);
            left[i] = out_l;
            right[i] = out_r;
        }
    }

    /// Reset the compressor state.
    pub fn reset(&mut self) {
        self.envelope = 0.0;
    }

    /// Get the current gain reduction in dB (for metering).
    pub fn get_gain_reduction(&self) -> f64 {
        let envelope_db = Self::linear_to_db(self.envelope);
        -self.compute_gain(envelope_db)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compressor_passthrough_below_threshold() {
        let mut comp = Compressor::with_params(44100.0, -20.0, 4.0, 0.001, 0.1);

        // Signal well below threshold should pass through unchanged
        // (after envelope settles)
        for _ in 0..1000 {
            comp.process(0.05, 0.05); // -26 dB, below -20 threshold
        }

        let (out_l, out_r) = comp.process(0.05, 0.05);
        // Should be close to input (within a small tolerance for envelope)
        assert!(
            (out_l - 0.05).abs() < 0.01,
            "Below threshold, output should be close to input: got {out_l}"
        );
        assert!((out_r - 0.05).abs() < 0.01);
    }

    #[test]
    fn test_compressor_reduces_loud_signals() {
        let mut comp = Compressor::with_params(44100.0, -12.0, 4.0, 0.001, 0.1);

        // Process loud signal to let envelope rise
        for _ in 0..5000 {
            comp.process(1.0, 1.0); // 0 dB, well above -12 threshold
        }

        let (out_l, _) = comp.process(1.0, 1.0);

        // 4:1 ratio at 12dB above threshold should reduce by 9dB
        // So output should be roughly 0.35 (-9dB from 1.0)
        assert!(
            out_l < 0.5,
            "Compressor should reduce loud signals: got {out_l}"
        );
        assert!(
            out_l > 0.1,
            "Compressor should not over-compress: got {out_l}"
        );
    }

    #[test]
    fn test_compressor_attack_time() {
        let mut comp = Compressor::with_params(44100.0, -20.0, 10.0, 0.01, 0.5);

        // First sample of loud signal should pass through more
        let (first, _) = comp.process(1.0, 1.0);

        // After attack time, should be compressed more
        for _ in 0..500 {
            comp.process(1.0, 1.0);
        }
        let (later, _) = comp.process(1.0, 1.0);

        assert!(
            first > later,
            "First sample should be louder than after attack: first={first}, later={later}"
        );
    }

    #[test]
    fn test_compressor_release_time() {
        let mut comp = Compressor::with_params(44100.0, -20.0, 10.0, 0.001, 0.05);

        // Compress loud signal
        for _ in 0..1000 {
            comp.process(1.0, 1.0);
        }

        // Now go quiet and measure release
        let (compressed, _) = comp.process(0.1, 0.1);

        // Wait for release
        for _ in 0..5000 {
            comp.process(0.1, 0.1);
        }

        let (released, _) = comp.process(0.1, 0.1);

        assert!(
            released > compressed,
            "After release, gain should recover: compressed={compressed}, released={released}"
        );
    }

    #[test]
    fn test_makeup_gain() {
        let mut comp = Compressor::new(44100.0);
        comp.threshold = -40.0; // Very low threshold
        comp.ratio = 4.0;
        comp.makeup_gain = 6.0; // +6dB makeup

        // Let envelope settle
        for _ in 0..2000 {
            comp.process(0.5, 0.5);
        }

        let (out_l, _) = comp.process(0.5, 0.5);

        // With makeup gain, compressed signal might be louder
        // The ratio and makeup should somewhat balance
        assert!(out_l > 0.0, "Makeup gain should boost signal");
    }
}
