//! Biquad filter — matches WebAudio BiquadFilterNode coefficients.

use std::f64::consts::PI;

/// Filter type.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FilterType {
    Lowpass,
    Highpass,
    Bandpass,
    Notch,
    Peaking,
}

/// A biquad IIR filter (2nd order).
///
/// Implements the standard Direct Form II Transposed structure.
/// Coefficient formulas from the Audio EQ Cookbook (Robert Bristow-Johnson).
#[derive(Debug, Clone)]
pub struct BiquadFilter {
    pub filter_type: FilterType,
    pub frequency: f64,
    pub q: f64,
    pub gain_db: f64, // only used for Peaking

    // Coefficients
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,

    // State (Direct Form II Transposed)
    z1: f64,
    z2: f64,

    sample_rate: f64,
    dirty: bool,
}

impl BiquadFilter {
    pub fn new(filter_type: FilterType, sample_rate: f64) -> Self {
        let mut f = BiquadFilter {
            filter_type,
            frequency: 1000.0,
            q: 0.707, // Butterworth
            gain_db: 0.0,
            b0: 1.0,
            b1: 0.0,
            b2: 0.0,
            a1: 0.0,
            a2: 0.0,
            z1: 0.0,
            z2: 0.0,
            sample_rate,
            dirty: true,
        };
        f.update_coefficients();
        f
    }

    /// Recompute filter coefficients from current parameters.
    pub fn update_coefficients(&mut self) {
        let w0 = 2.0 * PI * self.frequency / self.sample_rate;
        let cos_w0 = w0.cos();
        let sin_w0 = w0.sin();
        let alpha = sin_w0 / (2.0 * self.q);

        let (b0, b1, b2, a0, a1, a2) = match self.filter_type {
            FilterType::Lowpass => {
                let b1 = 1.0 - cos_w0;
                let b0 = b1 / 2.0;
                let b2 = b0;
                let a0 = 1.0 + alpha;
                let a1 = -2.0 * cos_w0;
                let a2 = 1.0 - alpha;
                (b0, b1, b2, a0, a1, a2)
            }
            FilterType::Highpass => {
                let b1_raw = 1.0 + cos_w0;
                let b0 = b1_raw / 2.0;
                let b1 = -(1.0 + cos_w0);
                let b2 = b0;
                let a0 = 1.0 + alpha;
                let a1 = -2.0 * cos_w0;
                let a2 = 1.0 - alpha;
                (b0, b1, b2, a0, a1, a2)
            }
            FilterType::Bandpass => {
                let b0 = alpha;
                let b1 = 0.0;
                let b2 = -alpha;
                let a0 = 1.0 + alpha;
                let a1 = -2.0 * cos_w0;
                let a2 = 1.0 - alpha;
                (b0, b1, b2, a0, a1, a2)
            }
            FilterType::Notch => {
                let b0 = 1.0;
                let b1 = -2.0 * cos_w0;
                let b2 = 1.0;
                let a0 = 1.0 + alpha;
                let a1 = -2.0 * cos_w0;
                let a2 = 1.0 - alpha;
                (b0, b1, b2, a0, a1, a2)
            }
            FilterType::Peaking => {
                let a_lin = (10.0_f64).powf(self.gain_db / 40.0);
                let b0 = 1.0 + alpha * a_lin;
                let b1 = -2.0 * cos_w0;
                let b2 = 1.0 - alpha * a_lin;
                let a0 = 1.0 + alpha / a_lin;
                let a1 = -2.0 * cos_w0;
                let a2 = 1.0 - alpha / a_lin;
                (b0, b1, b2, a0, a1, a2)
            }
        };

        // Normalize by a0
        self.b0 = b0 / a0;
        self.b1 = b1 / a0;
        self.b2 = b2 / a0;
        self.a1 = a1 / a0;
        self.a2 = a2 / a0;
        self.dirty = false;
    }

    /// Process a single sample through the filter.
    pub fn process(&mut self, input: f64) -> f64 {
        if self.dirty {
            self.update_coefficients();
        }

        let output = self.b0 * input + self.z1;
        self.z1 = self.b1 * input - self.a1 * output + self.z2;
        self.z2 = self.b2 * input - self.a2 * output;
        output
    }

    /// Reset filter state.
    pub fn reset(&mut self) {
        self.z1 = 0.0;
        self.z2 = 0.0;
    }

    /// Set frequency and mark coefficients dirty.
    pub fn set_frequency(&mut self, freq: f64) {
        self.frequency = freq;
        self.dirty = true;
    }

    /// Set Q and mark coefficients dirty.
    pub fn set_q(&mut self, q: f64) {
        self.q = q;
        self.dirty = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lowpass_passes_dc() {
        let mut f = BiquadFilter::new(FilterType::Lowpass, 44100.0);
        f.frequency = 5000.0;
        f.update_coefficients();

        // Feed DC signal (1.0) — should converge to 1.0
        let mut output = 0.0;
        for _ in 0..1000 {
            output = f.process(1.0);
        }
        assert!(
            (output - 1.0).abs() < 0.001,
            "Lowpass should pass DC, got {output}"
        );
    }

    #[test]
    fn highpass_blocks_dc() {
        let mut f = BiquadFilter::new(FilterType::Highpass, 44100.0);
        f.frequency = 1000.0;
        f.update_coefficients();

        // Feed DC — should converge to 0
        let mut output = 0.0;
        for _ in 0..1000 {
            output = f.process(1.0);
        }
        assert!(
            output.abs() < 0.001,
            "Highpass should block DC, got {output}"
        );
    }

    #[test]
    fn lowpass_attenuates_high_freq() {
        let mut f = BiquadFilter::new(FilterType::Lowpass, 44100.0);
        f.frequency = 200.0;
        f.q = 0.707;
        f.update_coefficients();

        // Generate a 10kHz sine and measure output amplitude
        let freq = 10000.0;
        let mut max_out = 0.0_f64;
        for i in 0..4410 {
            let t = i as f64 / 44100.0;
            let input = (2.0 * PI * freq * t).sin();
            let out = f.process(input);
            if i > 1000 {
                // skip transient
                max_out = max_out.max(out.abs());
            }
        }
        assert!(
            max_out < 0.01,
            "Lowpass@200Hz should strongly attenuate 10kHz, got amplitude {max_out}"
        );
    }

    #[test]
    fn filter_output_finite() {
        let mut f = BiquadFilter::new(FilterType::Bandpass, 44100.0);
        f.frequency = 1000.0;
        f.update_coefficients();

        for i in 0..10000 {
            let input = if i % 100 == 0 { 1.0 } else { 0.0 };
            let out = f.process(input);
            assert!(out.is_finite(), "Filter output not finite at sample {i}");
        }
    }
}
