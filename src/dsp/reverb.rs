//! Reverb effect — Schroeder-style algorithmic reverb.
//!
//! Uses parallel comb filters followed by series allpass filters,
//! based on the classic Schroeder/Moorer reverb design.

/// A comb filter delay line with feedback.
#[derive(Debug, Clone)]
struct CombFilter {
    buffer: Vec<f32>,
    index: usize,
    feedback: f32,
    damp1: f32,
    damp2: f32,
    filterstore: f32,
}

impl CombFilter {
    fn new(size: usize, feedback: f32, damp: f32) -> Self {
        Self {
            buffer: vec![0.0; size],
            index: 0,
            feedback,
            damp1: damp,
            damp2: 1.0 - damp,
            filterstore: 0.0,
        }
    }

    #[inline]
    fn process(&mut self, input: f32) -> f32 {
        let output = self.buffer[self.index];
        
        // Apply lowpass filter to feedback (damping)
        self.filterstore = output * self.damp2 + self.filterstore * self.damp1;
        
        self.buffer[self.index] = input + self.filterstore * self.feedback;
        self.index = (self.index + 1) % self.buffer.len();
        
        output
    }

    fn set_damp(&mut self, damp: f32) {
        self.damp1 = damp;
        self.damp2 = 1.0 - damp;
    }

    fn set_feedback(&mut self, feedback: f32) {
        self.feedback = feedback;
    }

    fn clear(&mut self) {
        self.buffer.fill(0.0);
        self.filterstore = 0.0;
    }
}

/// An allpass filter delay line.
#[derive(Debug, Clone)]
struct AllpassFilter {
    buffer: Vec<f32>,
    index: usize,
    feedback: f32,
}

impl AllpassFilter {
    fn new(size: usize) -> Self {
        Self {
            buffer: vec![0.0; size],
            index: 0,
            feedback: 0.5,
        }
    }

    #[inline]
    fn process(&mut self, input: f32) -> f32 {
        let bufout = self.buffer[self.index];
        let output = bufout - input;
        
        self.buffer[self.index] = input + bufout * self.feedback;
        self.index = (self.index + 1) % self.buffer.len();
        
        output
    }

    fn clear(&mut self) {
        self.buffer.fill(0.0);
    }
}

// Tuning constants (scaled for 44100 Hz sample rate)
const COMB_TUNING: [usize; 8] = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
const ALLPASS_TUNING: [usize; 4] = [556, 441, 341, 225];
const STEREO_SPREAD: usize = 23;

/// A stereo algorithmic reverb using the Schroeder/Freeverb design.
#[derive(Debug, Clone)]
pub struct Reverb {
    comb_l: Vec<CombFilter>,
    comb_r: Vec<CombFilter>,
    allpass_l: Vec<AllpassFilter>,
    allpass_r: Vec<AllpassFilter>,
    
    /// Room size (0.0 to 1.0). Affects decay time.
    pub room_size: f64,
    /// Damping (0.0 to 1.0). Higher = darker sound.
    pub damping: f64,
    /// Dry/wet mix (0.0 = fully dry, 1.0 = fully wet).
    pub mix: f64,
    /// Stereo width (0.0 to 1.0).
    pub width: f64,

    gain: f32,
}

impl Reverb {
    /// Create a new reverb effect.
    ///
    /// # Arguments
    /// - `sample_rate`: Audio sample rate in Hz.
    pub fn new(sample_rate: f64) -> Self {
        let scale = sample_rate / 44100.0;
        
        // Create comb filters for left and right channels
        let comb_l: Vec<_> = COMB_TUNING.iter()
            .map(|&t| {
                let size = ((t as f64) * scale) as usize;
                CombFilter::new(size, 0.84, 0.2)
            })
            .collect();
        
        let comb_r: Vec<_> = COMB_TUNING.iter()
            .map(|&t| {
                let size = ((t as f64) * scale + STEREO_SPREAD as f64) as usize;
                CombFilter::new(size, 0.84, 0.2)
            })
            .collect();
        
        // Create allpass filters
        let allpass_l: Vec<_> = ALLPASS_TUNING.iter()
            .map(|&t| {
                let size = ((t as f64) * scale) as usize;
                AllpassFilter::new(size)
            })
            .collect();
        
        let allpass_r: Vec<_> = ALLPASS_TUNING.iter()
            .map(|&t| {
                let size = ((t as f64) * scale + STEREO_SPREAD as f64) as usize;
                AllpassFilter::new(size)
            })
            .collect();
        
        let mut reverb = Self {
            comb_l,
            comb_r,
            allpass_l,
            allpass_r,
            room_size: 0.5,
            damping: 0.5,
            mix: 0.3,
            width: 1.0,
            gain: 0.015,
        };
        
        reverb.update_parameters();
        reverb
    }

    /// Create a reverb with specific parameters.
    pub fn with_params(sample_rate: f64, room_size: f64, damping: f64, mix: f64) -> Self {
        let mut r = Self::new(sample_rate);
        r.room_size = room_size.clamp(0.0, 1.0);
        r.damping = damping.clamp(0.0, 1.0);
        r.mix = mix.clamp(0.0, 1.0);
        r.update_parameters();
        r
    }

    /// Update internal parameters after changing room_size or damping.
    pub fn update_parameters(&mut self) {
        let room_scale = 0.28;
        let room_offset = 0.7;
        let feedback = (self.room_size * room_scale + room_offset) as f32;
        let damp = self.damping as f32;
        
        for comb in &mut self.comb_l {
            comb.set_feedback(feedback);
            comb.set_damp(damp);
        }
        for comb in &mut self.comb_r {
            comb.set_feedback(feedback);
            comb.set_damp(damp);
        }
    }

    /// Process a stereo sample pair, returning the processed output.
    #[inline]
    pub fn process(&mut self, left: f32, right: f32) -> (f32, f32) {
        let input = (left + right) * self.gain;
        
        // Sum comb filters in parallel
        let mut out_l = 0.0f32;
        let mut out_r = 0.0f32;
        
        for comb in &mut self.comb_l {
            out_l += comb.process(input);
        }
        for comb in &mut self.comb_r {
            out_r += comb.process(input);
        }
        
        // Process through allpass filters in series
        for allpass in &mut self.allpass_l {
            out_l = allpass.process(out_l);
        }
        for allpass in &mut self.allpass_r {
            out_r = allpass.process(out_r);
        }
        
        // Apply stereo width
        let width = self.width as f32;
        let wet1 = width / 2.0 + 0.5;
        let wet2 = (1.0 - width) / 2.0;
        
        let wet_l = out_l * wet1 + out_r * wet2;
        let wet_r = out_r * wet1 + out_l * wet2;
        
        // Mix dry/wet
        let mix = self.mix as f32;
        let final_l = left * (1.0 - mix) + wet_l * mix;
        let final_r = right * (1.0 - mix) + wet_r * mix;
        
        (final_l, final_r)
    }

    /// Process a block of stereo audio in-place.
    pub fn process_block(&mut self, left: &mut [f32], right: &mut [f32]) {
        for i in 0..left.len().min(right.len()) {
            let (out_l, out_r) = self.process(left[i], right[i]);
            left[i] = out_l;
            right[i] = out_r;
        }
    }

    /// Clear all internal buffers.
    pub fn clear(&mut self) {
        for comb in &mut self.comb_l {
            comb.clear();
        }
        for comb in &mut self.comb_r {
            comb.clear();
        }
        for allpass in &mut self.allpass_l {
            allpass.clear();
        }
        for allpass in &mut self.allpass_r {
            allpass.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reverb_passthrough_when_dry() {
        let mut reverb = Reverb::with_params(44100.0, 0.5, 0.5, 0.0);
        
        // With mix = 0, output should equal input
        let (out_l, out_r) = reverb.process(0.5, -0.5);
        assert!((out_l - 0.5).abs() < 1e-6);
        assert!((out_r - (-0.5)).abs() < 1e-6);
    }

    #[test]
    fn test_reverb_produces_output() {
        let mut reverb = Reverb::with_params(44100.0, 0.5, 0.5, 1.0);
        
        // Send an impulse
        reverb.process(1.0, 1.0);
        
        // Process silent samples and verify reverb tail
        let mut found_reverb = false;
        for _ in 0..5000 {
            let (out_l, out_r) = reverb.process(0.0, 0.0);
            if out_l.abs() > 0.001 || out_r.abs() > 0.001 {
                found_reverb = true;
                break;
            }
        }
        assert!(found_reverb, "Reverb should produce output after impulse");
    }

    #[test]
    fn test_reverb_decays() {
        let mut reverb = Reverb::with_params(44100.0, 0.3, 0.5, 1.0);
        
        // Send an impulse
        reverb.process(1.0, 1.0);
        
        // Let reverb decay for a while
        let mut max_output = 0.0f32;
        for _ in 0..2000 {
            let (out_l, out_r) = reverb.process(0.0, 0.0);
            max_output = max_output.max(out_l.abs().max(out_r.abs()));
        }
        
        // After 2000 samples, reverb should have started
        assert!(max_output > 0.0, "Reverb should have some output");
        
        // Continue to check decay
        let mut later_max = 0.0f32;
        for _ in 0..44100 {
            let (out_l, out_r) = reverb.process(0.0, 0.0);
            later_max = later_max.max(out_l.abs().max(out_r.abs()));
        }
        
        // After ~1 second, reverb should be quieter
        // (with room_size 0.3, it should decay relatively quickly)
        assert!(later_max < 0.1, "Reverb should decay over time");
    }
}
