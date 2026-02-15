//! Sample-based synthesis engine.
//!
//! Plays back audio samples with pitch-shifting via linear interpolation
//! resampling. Supports multi-zone key splits, loop points, and
//! tuning-aware playback rate calculation.

use crate::preset::{sample_playback_rate, SampleZone};

/// A single sample buffer loaded into memory.
#[derive(Debug, Clone)]
pub struct SampleBuffer {
    /// Mono f64 samples.
    pub data: Vec<f64>,
    /// Native sample rate of the audio.
    pub sample_rate: u32,
}

impl SampleBuffer {
    pub fn new(data: Vec<f64>, sample_rate: u32) -> Self {
        SampleBuffer { data, sample_rate }
    }

    /// Create from 16-bit signed PCM data.
    pub fn from_i16(pcm: &[i16], sample_rate: u32) -> Self {
        let data: Vec<f64> = pcm.iter().map(|&s| s as f64 / 32768.0).collect();
        SampleBuffer { data, sample_rate }
    }

    /// Create from f32 samples.
    pub fn from_f32(samples: &[f32], sample_rate: u32) -> Self {
        let data: Vec<f64> = samples.iter().map(|&s| s as f64).collect();
        SampleBuffer { data, sample_rate }
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// Read a sample with linear interpolation at a fractional position.
    pub fn read_interpolated(&self, position: f64) -> f64 {
        if self.data.is_empty() || position < 0.0 {
            return 0.0;
        }

        let idx = position as usize;
        if idx >= self.data.len() - 1 {
            return if idx < self.data.len() {
                self.data[idx]
            } else {
                0.0
            };
        }

        let frac = position - idx as f64;
        self.data[idx] * (1.0 - frac) + self.data[idx + 1] * frac
    }
}

/// A loaded zone: metadata + its audio buffer.
#[derive(Debug, Clone)]
pub struct LoadedZone {
    pub key_range_low: u8,
    pub key_range_high: u8,
    pub root_note: u8,
    pub fine_tune_cents: f64,
    pub sample_rate: u32,
    pub loop_start: Option<u64>,
    pub loop_end: Option<u64>,
    pub buffer: SampleBuffer,
}

impl LoadedZone {
    /// Create from a SampleZone descriptor and a sample buffer.
    pub fn from_zone(zone: &SampleZone, buffer: SampleBuffer) -> Self {
        LoadedZone {
            key_range_low: zone.key_range.low,
            key_range_high: zone.key_range.high,
            root_note: zone.pitch.root_note,
            fine_tune_cents: zone.pitch.fine_tune_cents,
            sample_rate: zone.sample_rate,
            loop_start: zone.r#loop.as_ref().map(|l| l.start),
            loop_end: zone.r#loop.as_ref().map(|l| l.end),
            buffer,
        }
    }

    /// Check if a MIDI note falls within this zone's key range.
    pub fn contains_note(&self, midi_note: u8) -> bool {
        midi_note >= self.key_range_low && midi_note <= self.key_range_high
    }
}

/// A sampler instrument with loaded zone data.
#[derive(Debug, Clone)]
pub struct Sampler {
    pub zones: Vec<LoadedZone>,
    pub is_drum_kit: bool,
}

impl Sampler {
    pub fn new(zones: Vec<LoadedZone>, is_drum_kit: bool) -> Self {
        Sampler { zones, is_drum_kit }
    }

    /// Find the best zone for a given MIDI note.
    pub fn find_zone(&self, midi_note: u8) -> Option<&LoadedZone> {
        self.zones
            .iter()
            .find(|z| z.contains_note(midi_note))
    }
}

/// A playing sampler voice — reads from a zone buffer at a calculated rate.
#[derive(Debug, Clone)]
pub struct SamplerVoice {
    /// Current read position in the sample buffer (fractional).
    position: f64,
    /// Playback rate (1.0 = original speed).
    playback_rate: f64,
    /// Sample rate ratio (zone sample rate / engine sample rate).
    sample_rate_ratio: f64,
    /// Loop start in samples.
    loop_start: Option<u64>,
    /// Loop end in samples.
    loop_end: Option<u64>,
    /// Velocity (0.0 - 1.0).
    velocity: f64,
    /// Reference to the zone's buffer length.
    buffer_len: usize,
    /// Whether the voice has finished playing.
    finished: bool,
    /// Whether the note has been released.
    released: bool,
    /// The release sample offset (set by the engine).
    pub release_sample: usize,
    /// Simple envelope state.
    envelope: SamplerEnvelope,
    /// Reference data (clone of the buffer for self-contained voice).
    buffer: SampleBuffer,
}

/// Simple ADSR envelope for sampler voices.
#[derive(Debug, Clone)]
struct SamplerEnvelope {
    attack: f64,
    decay: f64,
    sustain: f64,
    release: f64,
    sample_rate: f64,
    state: EnvState,
    level: f64,
    samples_in_state: usize,
}

#[derive(Debug, Clone, PartialEq)]
enum EnvState {
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
    Done,
}

impl SamplerEnvelope {
    fn new(sample_rate: f64) -> Self {
        SamplerEnvelope {
            attack: 0.005,  // 5ms click-free attack
            decay: 0.1,
            sustain: 1.0,   // Samplers typically use full sustain
            release: 0.1,   // Short release for samples
            sample_rate,
            state: EnvState::Idle,
            level: 0.0,
            samples_in_state: 0,
        }
    }

    fn note_on(&mut self) {
        self.state = EnvState::Attack;
        self.samples_in_state = 0;
    }

    fn note_off(&mut self) {
        if self.state != EnvState::Done && self.state != EnvState::Idle {
            self.state = EnvState::Release;
            self.samples_in_state = 0;
        }
    }

    fn next_sample(&mut self) -> f64 {
        self.samples_in_state += 1;
        match self.state {
            EnvState::Idle => 0.0,
            EnvState::Attack => {
                let attack_samples = (self.attack * self.sample_rate) as usize;
                if attack_samples == 0 || self.samples_in_state >= attack_samples {
                    self.state = EnvState::Decay;
                    self.samples_in_state = 0;
                    self.level = 1.0;
                } else {
                    self.level = self.samples_in_state as f64 / attack_samples as f64;
                }
                self.level
            }
            EnvState::Decay => {
                let decay_samples = (self.decay * self.sample_rate) as usize;
                if decay_samples == 0 || self.samples_in_state >= decay_samples {
                    self.state = EnvState::Sustain;
                    self.samples_in_state = 0;
                    self.level = self.sustain;
                } else {
                    let t = self.samples_in_state as f64 / decay_samples as f64;
                    self.level = 1.0 - t * (1.0 - self.sustain);
                }
                self.level
            }
            EnvState::Sustain => {
                self.level = self.sustain;
                self.level
            }
            EnvState::Release => {
                let release_samples = (self.release * self.sample_rate) as usize;
                if release_samples == 0 || self.samples_in_state >= release_samples {
                    self.state = EnvState::Done;
                    self.level = 0.0;
                } else {
                    let t = self.samples_in_state as f64 / release_samples as f64;
                    self.level = self.sustain * (1.0 - t);
                }
                self.level
            }
            EnvState::Done => 0.0,
        }
    }

    fn is_done(&self) -> bool {
        self.state == EnvState::Done
    }
}

impl SamplerVoice {
    /// Create a new sampler voice for a zone playing at a given MIDI note.
    ///
    /// # Arguments
    /// * `zone` - The loaded zone to play
    /// * `midi_note` - The target MIDI note
    /// * `velocity` - Note velocity (0.0 - 1.0)
    /// * `tuning_pitch` - A4 frequency (440.0 default)
    /// * `engine_sample_rate` - The output sample rate
    pub fn new(
        zone: &LoadedZone,
        midi_note: u8,
        velocity: f64,
        tuning_pitch: f64,
        engine_sample_rate: f64,
    ) -> Self {
        // Calculate playback rate from pitch
        let pitch_rate = sample_playback_rate(
            midi_note,
            zone.root_note,
            zone.fine_tune_cents,
            tuning_pitch,
        );

        // Sample rate conversion factor
        let sr_ratio = zone.sample_rate as f64 / engine_sample_rate;

        let mut envelope = SamplerEnvelope::new(engine_sample_rate);
        envelope.note_on();

        SamplerVoice {
            position: 0.0,
            playback_rate: pitch_rate,
            sample_rate_ratio: sr_ratio,
            loop_start: zone.loop_start,
            loop_end: zone.loop_end,
            velocity,
            buffer_len: zone.buffer.len(),
            finished: false,
            released: false,
            release_sample: usize::MAX,
            envelope,
            buffer: zone.buffer.clone(),
        }
    }

    /// Generate the next audio sample.
    pub fn next_sample(&mut self) -> f64 {
        if self.finished {
            return 0.0;
        }

        // Read from buffer with interpolation
        let sample = self.buffer.read_interpolated(self.position);

        // Advance position
        let step = self.playback_rate * self.sample_rate_ratio;
        self.position += step;

        // Handle looping
        if let (Some(loop_start), Some(loop_end)) = (self.loop_start, self.loop_end) {
            let loop_start = loop_start as f64;
            let loop_end = loop_end as f64;
            if !self.released && self.position >= loop_end && loop_end > loop_start {
                let loop_length = loop_end - loop_start;
                self.position = loop_start + (self.position - loop_end) % loop_length;
            }
        }

        // Check if past end of buffer
        if self.position >= self.buffer_len as f64 {
            self.finished = true;
            return 0.0;
        }

        // Apply envelope and velocity
        let env = self.envelope.next_sample();
        if self.envelope.is_done() {
            self.finished = true;
        }

        sample * env * self.velocity
    }

    /// Trigger note release.
    pub fn note_off(&mut self) {
        self.released = true;
        self.envelope.note_off();
    }

    /// Check if this voice has finished playing.
    pub fn is_finished(&self) -> bool {
        self.finished
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_buffer() -> SampleBuffer {
        // Create a simple sine wave at 440 Hz, 44100 sample rate, 1 second
        let sample_rate = 44100;
        let freq = 440.0;
        let duration = 1.0;
        let num_samples = (sample_rate as f64 * duration) as usize;

        let data: Vec<f64> = (0..num_samples)
            .map(|i| {
                let t = i as f64 / sample_rate as f64;
                (2.0 * std::f64::consts::PI * freq * t).sin()
            })
            .collect();

        SampleBuffer::new(data, sample_rate)
    }

    fn make_test_zone() -> LoadedZone {
        LoadedZone {
            key_range_low: 0,
            key_range_high: 127,
            root_note: 69, // A4
            fine_tune_cents: 0.0,
            sample_rate: 44100,
            loop_start: None,
            loop_end: None,
            buffer: make_test_buffer(),
        }
    }

    #[test]
    fn sample_buffer_interpolation() {
        let buf = SampleBuffer::new(vec![0.0, 1.0, 0.0, -1.0], 44100);

        assert!((buf.read_interpolated(0.0) - 0.0).abs() < 0.001);
        assert!((buf.read_interpolated(0.5) - 0.5).abs() < 0.001);
        assert!((buf.read_interpolated(1.0) - 1.0).abs() < 0.001);
        assert!((buf.read_interpolated(1.5) - 0.5).abs() < 0.001);
    }

    #[test]
    fn sample_buffer_from_i16() {
        let pcm: Vec<i16> = vec![0, 16384, -16384, 32767];
        let buf = SampleBuffer::from_i16(&pcm, 44100);

        assert_eq!(buf.len(), 4);
        assert!((buf.data[0]).abs() < 0.001);
        assert!((buf.data[1] - 0.5).abs() < 0.01);
        assert!((buf.data[2] + 0.5).abs() < 0.01);
    }

    #[test]
    fn zone_contains_note() {
        let zone = make_test_zone();
        assert!(zone.contains_note(60));
        assert!(zone.contains_note(0));
        assert!(zone.contains_note(127));
    }

    #[test]
    fn sampler_find_zone() {
        let zone1 = LoadedZone {
            key_range_low: 0,
            key_range_high: 60,
            ..make_test_zone()
        };
        let zone2 = LoadedZone {
            key_range_low: 61,
            key_range_high: 127,
            ..make_test_zone()
        };

        let sampler = Sampler::new(vec![zone1, zone2], false);

        // C4 (60) should find zone1
        assert_eq!(sampler.find_zone(60).unwrap().key_range_high, 60);
        // C5 (72) should find zone2
        assert_eq!(sampler.find_zone(72).unwrap().key_range_low, 61);
    }

    #[test]
    fn sampler_voice_produces_sound() {
        let zone = make_test_zone();
        let mut voice = SamplerVoice::new(&zone, 69, 1.0, 440.0, 44100.0);

        let mut max_val = 0.0_f64;
        for _ in 0..4410 {
            let s = voice.next_sample();
            max_val = max_val.max(s.abs());
        }

        assert!(max_val > 0.1, "Voice should produce audible output, max={max_val}");
    }

    #[test]
    fn sampler_voice_at_root_pitch() {
        // Playing A4 on a sample recorded at A4 should play at rate ~1.0
        let zone = make_test_zone();
        let mut voice = SamplerVoice::new(&zone, 69, 1.0, 440.0, 44100.0);

        // After 100 samples, position should be ~100 (rate 1.0)
        for _ in 0..100 {
            voice.next_sample();
        }

        // Position should be close to 100 (slight offset from envelope attack)
        assert!(
            (voice.position - 100.0).abs() < 2.0,
            "Position should be ~100 at root note, got {}",
            voice.position
        );
    }

    #[test]
    fn sampler_voice_octave_up() {
        // Playing A5 (note 81) on A4 sample should advance at rate 2.0
        let zone = make_test_zone();
        let mut voice = SamplerVoice::new(&zone, 81, 1.0, 440.0, 44100.0);

        for _ in 0..100 {
            voice.next_sample();
        }

        // Position should be ~200 (rate 2.0)
        assert!(
            (voice.position - 200.0).abs() < 4.0,
            "Position should be ~200 one octave up, got {}",
            voice.position
        );
    }

    #[test]
    fn sampler_voice_finishes() {
        let short_buf = SampleBuffer::new(vec![1.0; 100], 44100);
        let zone = LoadedZone {
            buffer: short_buf,
            ..make_test_zone()
        };

        let mut voice = SamplerVoice::new(&zone, 69, 1.0, 440.0, 44100.0);

        // Play past the buffer
        for _ in 0..200 {
            voice.next_sample();
        }

        assert!(voice.is_finished(), "Voice should finish after buffer ends");
    }

    #[test]
    fn sampler_voice_looping() {
        let buf = SampleBuffer::new(vec![0.5; 1000], 44100);
        let zone = LoadedZone {
            loop_start: Some(500),
            loop_end: Some(900),
            buffer: buf,
            ..make_test_zone()
        };

        let mut voice = SamplerVoice::new(&zone, 69, 1.0, 440.0, 44100.0);

        // Play well past the loop end — should not finish if looping
        for _ in 0..2000 {
            voice.next_sample();
        }

        assert!(
            !voice.is_finished(),
            "Looping voice should not finish while sustaining"
        );
    }

    #[test]
    fn sampler_voice_release() {
        let buf = SampleBuffer::new(vec![0.5; 10000], 44100);
        let zone = LoadedZone {
            loop_start: Some(500),
            loop_end: Some(9000),
            buffer: buf,
            ..make_test_zone()
        };

        let mut voice = SamplerVoice::new(&zone, 69, 1.0, 440.0, 44100.0);

        // Play and then release
        for _ in 0..500 {
            voice.next_sample();
        }
        voice.note_off();

        // After release, voice should eventually finish
        let mut finished = false;
        for _ in 0..50000 {
            voice.next_sample();
            if voice.is_finished() {
                finished = true;
                break;
            }
        }

        assert!(finished, "Voice should finish after release + buffer end");
    }

    #[test]
    fn sampler_voice_tuning_432() {
        // At 432 Hz tuning, playing A4 should advance slower (432/440 rate)
        let zone = make_test_zone();
        let mut voice = SamplerVoice::new(&zone, 69, 1.0, 432.0, 44100.0);

        for _ in 0..1000 {
            voice.next_sample();
        }

        let expected_rate = 432.0 / 440.0;
        let expected_pos = expected_rate * 1000.0;
        assert!(
            (voice.position - expected_pos).abs() < 5.0,
            "Position should be ~{expected_pos} at 432Hz tuning, got {}",
            voice.position
        );
    }

    #[test]
    fn sampler_voice_velocity_scaling() {
        let zone = make_test_zone();

        // Play at full velocity
        let mut loud = SamplerVoice::new(&zone, 69, 1.0, 440.0, 44100.0);
        // Play at half velocity
        let mut quiet = SamplerVoice::new(&zone, 69, 0.5, 440.0, 44100.0);

        // Skip past attack
        for _ in 0..500 {
            loud.next_sample();
            quiet.next_sample();
        }

        let loud_sample = loud.next_sample().abs();
        let quiet_sample = quiet.next_sample().abs();

        if loud_sample > 0.001 && quiet_sample > 0.001 {
            let ratio = quiet_sample / loud_sample;
            assert!(
                (ratio - 0.5).abs() < 0.15,
                "Half velocity should produce ~half amplitude, ratio={ratio}"
            );
        }
    }
}
