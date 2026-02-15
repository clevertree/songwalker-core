//! Sample Tuner — pitch detection and analysis for audio samples.
//!
//! Uses autocorrelation-based pitch detection (YIN-inspired) to
//! estimate the fundamental frequency of a sample, then computes
//! the MIDI note number and fine-tune cents needed for preset metadata.

/// Result of pitch detection on a sample.
#[derive(Debug, Clone, PartialEq)]
pub struct PitchEstimate {
    /// Estimated fundamental frequency in Hz.
    pub frequency: f64,
    /// Confidence in [0, 1] — higher is better.
    pub confidence: f64,
    /// Nearest MIDI note number.
    pub midi_note: u8,
    /// Fine-tune offset in cents from the nearest MIDI note.
    pub fine_tune_cents: f64,
    /// Whether the sample appears to be non-melodic (e.g. noise, drums).
    pub is_noise: bool,
}

/// Detect the fundamental frequency of a mono audio buffer.
///
/// Uses a simplified YIN algorithm (autocorrelation + difference function).
///
/// - `samples`: mono audio data (f64)
/// - `sample_rate`: audio sample rate in Hz
/// - `min_freq`: minimum detectable frequency (default: 50 Hz)
/// - `max_freq`: maximum detectable frequency (default: 2000 Hz)
pub fn detect_pitch(
    samples: &[f64],
    sample_rate: u32,
    min_freq: Option<f64>,
    max_freq: Option<f64>,
) -> PitchEstimate {
    let sr = sample_rate as f64;
    let min_f = min_freq.unwrap_or(50.0);
    let max_f = max_freq.unwrap_or(2000.0);

    // Convert frequency bounds to lag bounds
    let min_lag = (sr / max_f).ceil() as usize;
    let max_lag = (sr / min_f).floor() as usize;

    if samples.len() < max_lag * 2 || samples.is_empty() {
        return PitchEstimate {
            frequency: 0.0,
            confidence: 0.0,
            midi_note: 0,
            fine_tune_cents: 0.0,
            is_noise: true,
        };
    }

    let window_size = max_lag.min(samples.len() / 2);

    // Step 1: Compute the difference function (YIN step 2)
    let mut diff = vec![0.0f64; window_size + 1];
    for tau in 1..=window_size {
        let mut sum = 0.0;
        for j in 0..window_size {
            let d = samples[j] - samples[j + tau];
            sum += d * d;
        }
        diff[tau] = sum;
    }

    // Step 2: Cumulative mean normalized difference (YIN step 3)
    let mut cmnd = vec![1.0f64; window_size + 1];
    cmnd[0] = 1.0;
    let mut running_sum = 0.0;
    for tau in 1..=window_size {
        running_sum += diff[tau];
        if running_sum > 0.0 {
            cmnd[tau] = diff[tau] * tau as f64 / running_sum;
        }
    }

    // Step 3: Absolute threshold — find first dip below threshold
    let threshold = 0.15;
    let mut best_tau = 0usize;
    let mut best_val = 1.0f64;

    for tau in min_lag..=window_size.min(max_lag) {
        if cmnd[tau] < threshold {
            // Find the local minimum after this point
            let mut t = tau;
            while t + 1 <= window_size.min(max_lag) && cmnd[t + 1] < cmnd[t] {
                t += 1;
            }
            best_tau = t;
            best_val = cmnd[t];
            break;
        }
    }

    // Fallback: if no period found below threshold, use the global minimum
    if best_tau == 0 {
        for tau in min_lag..=window_size.min(max_lag) {
            if cmnd[tau] < best_val {
                best_val = cmnd[tau];
                best_tau = tau;
            }
        }
    }

    if best_tau == 0 {
        return PitchEstimate {
            frequency: 0.0,
            confidence: 0.0,
            midi_note: 0,
            fine_tune_cents: 0.0,
            is_noise: true,
        };
    }

    // Step 4: Parabolic interpolation for sub-sample accuracy
    let tau_refined = if best_tau > 0 && best_tau < window_size {
        let alpha = cmnd[best_tau - 1];
        let beta = cmnd[best_tau];
        let gamma = cmnd[best_tau + 1];
        let denom = alpha - 2.0 * beta + gamma;
        if denom.abs() > 1e-12 {
            best_tau as f64 + 0.5 * (alpha - gamma) / denom
        } else {
            best_tau as f64
        }
    } else {
        best_tau as f64
    };

    let frequency = sr / tau_refined;
    let confidence = 1.0 - best_val;
    let is_noise = confidence < 0.5;

    // Convert to MIDI note + cents
    let (midi_note, fine_tune_cents) = freq_to_midi_cents(frequency, 440.0);

    PitchEstimate {
        frequency,
        confidence,
        midi_note,
        fine_tune_cents,
        is_noise,
    }
}

/// Convert a frequency to the nearest MIDI note + fine-tune cents.
fn freq_to_midi_cents(freq: f64, a4_freq: f64) -> (u8, f64) {
    if freq <= 0.0 {
        return (0, 0.0);
    }
    let midi_float = 69.0 + 12.0 * (freq / a4_freq).log2();
    let midi_note = midi_float.round() as i32;
    let cents = (midi_float - midi_note as f64) * 100.0;

    let clamped = midi_note.clamp(0, 127) as u8;
    (clamped, cents)
}

/// Analyse all zones in a preset, detecting the actual pitch of each sample.
/// Returns a list of (zone_index, pitch_estimate) pairs.
pub fn analyse_zones(
    zones: &[(Vec<f64>, u32)],  // (sample_data, sample_rate) per zone
) -> Vec<(usize, PitchEstimate)> {
    zones.iter().enumerate().map(|(i, (data, sr))| {
        let estimate = detect_pitch(data, *sr, None, None);
        (i, estimate)
    }).collect()
}

/// Suggest tuning corrections for zones based on pitch detection.
#[derive(Debug, Clone)]
pub struct TuningCorrection {
    pub zone_index: usize,
    /// Detected pitch.
    pub detected: PitchEstimate,
    /// Recommended root_note for the zone.
    pub suggested_root: u8,
    /// Recommended fine_tune_cents for the zone.
    pub suggested_fine_tune: f64,
    /// Deviation from declared pitch in cents.
    pub deviation_cents: f64,
}

pub fn suggest_corrections(
    zones: &[(Vec<f64>, u32, u8, f64)],  // (samples, rate, declared_root, declared_cents)
) -> Vec<TuningCorrection> {
    zones.iter().enumerate().map(|(i, (data, sr, declared_root, declared_cents))| {
        let detected = detect_pitch(data, *sr, None, None);

        // Expected frequency from declared pitch
        let declared_freq = 440.0 * 2.0_f64.powf(
            (*declared_root as f64 - 69.0 + *declared_cents / 100.0) / 12.0
        );

        let deviation_cents = if detected.frequency > 0.0 && declared_freq > 0.0 {
            1200.0 * (detected.frequency / declared_freq).log2()
        } else {
            0.0
        };

        TuningCorrection {
            zone_index: i,
            suggested_root: detected.midi_note,
            suggested_fine_tune: detected.fine_tune_cents,
            deviation_cents,
            detected,
        }
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    fn generate_sine(freq: f64, sample_rate: u32, duration: f64) -> Vec<f64> {
        let num_samples = (sample_rate as f64 * duration) as usize;
        (0..num_samples)
            .map(|i| {
                let t = i as f64 / sample_rate as f64;
                (2.0 * PI * freq * t).sin()
            })
            .collect()
    }

    #[test]
    fn detect_a4_440hz() {
        let samples = generate_sine(440.0, 44100, 0.5);
        let result = detect_pitch(&samples, 44100, None, None);

        assert!(!result.is_noise, "Pure sine should not be noise");
        assert!(result.confidence > 0.8, "Confidence should be high: {}", result.confidence);
        assert!((result.frequency - 440.0).abs() < 5.0,
            "Expected ~440Hz, got {}", result.frequency);
        assert_eq!(result.midi_note, 69, "A4 should be MIDI 69");
        assert!(result.fine_tune_cents.abs() < 20.0,
            "Fine tune should be near 0: {}", result.fine_tune_cents);
    }

    #[test]
    fn detect_c4_262hz() {
        let samples = generate_sine(261.63, 44100, 0.5);
        let result = detect_pitch(&samples, 44100, None, None);

        assert!(!result.is_noise);
        assert!((result.frequency - 261.63).abs() < 5.0,
            "Expected ~261.63Hz, got {}", result.frequency);
        assert_eq!(result.midi_note, 60, "C4 should be MIDI 60");
    }

    #[test]
    fn detect_low_frequency() {
        let samples = generate_sine(82.41, 44100, 1.0);  // E2
        let result = detect_pitch(&samples, 44100, Some(50.0), None);

        assert!(!result.is_noise);
        assert!((result.frequency - 82.41).abs() < 3.0,
            "Expected ~82.41Hz, got {}", result.frequency);
        assert_eq!(result.midi_note, 40, "E2 should be MIDI 40");
    }

    #[test]
    fn detect_high_frequency() {
        let samples = generate_sine(1046.5, 44100, 0.3);  // C6
        let result = detect_pitch(&samples, 44100, None, None);

        assert!(!result.is_noise);
        assert!((result.frequency - 1046.5).abs() < 10.0,
            "Expected ~1046.5Hz, got {}", result.frequency);
    }

    #[test]
    fn noise_detection() {
        // White noise should be flagged as non-melodic
        let mut rng: u64 = 12345;
        let samples: Vec<f64> = (0..44100)
            .map(|_| {
                // Simple LCG pseudo-random
                rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                (rng as f64 / u64::MAX as f64) * 2.0 - 1.0
            })
            .collect();

        let result = detect_pitch(&samples, 44100, None, None);
        // We expect low confidence for noise
        assert!(result.confidence < 0.6,
            "Noise should have low confidence: {}", result.confidence);
    }

    #[test]
    fn empty_buffer() {
        let result = detect_pitch(&[], 44100, None, None);
        assert!(result.is_noise);
        assert_eq!(result.frequency, 0.0);
    }

    #[test]
    fn freq_to_midi_a4() {
        let (note, cents) = freq_to_midi_cents(440.0, 440.0);
        assert_eq!(note, 69);
        assert!(cents.abs() < 0.1);
    }

    #[test]
    fn freq_to_midi_a432() {
        let (note, cents) = freq_to_midi_cents(432.0, 440.0);
        // 432 Hz is about 31.8 cents flat of A4
        assert_eq!(note, 69);
        assert!((cents - (-31.77)).abs() < 1.0,
            "Expected ~-31.8 cents, got {}", cents);
    }

    #[test]
    fn tuning_correction_suggestion() {
        let samples = generate_sine(442.0, 44100, 0.5);  // Slightly sharp A4
        let zones = vec![(samples, 44100u32, 69u8, 0.0f64)];
        let corrections = suggest_corrections(&zones);

        assert_eq!(corrections.len(), 1);
        assert!(corrections[0].deviation_cents > 0.0,
            "442Hz should be sharp of 440Hz");
        assert!(corrections[0].deviation_cents < 20.0,
            "Deviation should be small: {}", corrections[0].deviation_cents);
    }
}
