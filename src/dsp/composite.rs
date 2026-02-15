//! Composite instrument node — combines multiple sound sources.
//!
//! Supports three modes:
//! - **Layer**: All children play simultaneously, mixed together
//! - **Split**: Route notes to children by MIDI key range
//! - **Chain**: Audio passes through children in series (for effects)

use super::sampler::{SamplerVoice, LoadedZone, Sampler};

/// Mode of combination for composite children.
#[derive(Debug, Clone, PartialEq)]
pub enum CompositeMode {
    /// All children play simultaneously.
    Layer,
    /// Route notes by MIDI key range.
    Split,
    /// Audio passes through in series (effects chain).
    Chain,
}

/// A loaded composite instrument.
#[derive(Debug, Clone)]
pub struct CompositeInstrument {
    pub mode: CompositeMode,
    pub children: Vec<CompositeChild>,
    /// Per-child mix levels (for Layer mode). Length should match children.
    pub mix_levels: Option<Vec<f64>>,
    /// Split points (MIDI note boundaries) for Split mode.
    pub split_points: Option<Vec<u8>>,
}

/// A child node in a composite instrument (resolved to a concrete type).
#[derive(Debug, Clone)]
pub enum CompositeChild {
    /// A sampler with zones.
    Sampler(Sampler),
    /// A nested composite.
    Composite(Box<CompositeInstrument>),
    // Future: Oscillator, Effect nodes
}

impl CompositeInstrument {
    pub fn new_layer(children: Vec<CompositeChild>, mix_levels: Option<Vec<f64>>) -> Self {
        CompositeInstrument {
            mode: CompositeMode::Layer,
            children,
            mix_levels,
            split_points: None,
        }
    }

    pub fn new_split(children: Vec<CompositeChild>, split_points: Option<Vec<u8>>) -> Self {
        CompositeInstrument {
            mode: CompositeMode::Split,
            children,
            mix_levels: None,
            split_points,
        }
    }

    /// Trigger a note and return all active voices for that note.
    pub fn trigger_note(
        &self,
        midi_note: u8,
        velocity: f64,
        tuning_pitch: f64,
        engine_sample_rate: f64,
    ) -> Vec<CompositeVoice> {
        match self.mode {
            CompositeMode::Layer => {
                // All children play simultaneously
                let mut voices = Vec::new();
                for (i, child) in self.children.iter().enumerate() {
                    let mix = self.mix_levels.as_ref()
                        .and_then(|levels| levels.get(i).copied())
                        .unwrap_or(1.0);

                    let child_voices = trigger_child(child, midi_note, velocity * mix, tuning_pitch, engine_sample_rate);
                    voices.extend(child_voices);
                }
                voices
            }
            CompositeMode::Split => {
                // Route to the child covering this note's range
                if let Some(split_pts) = &self.split_points {
                    // Find which child covers this MIDI note
                    let mut child_idx = 0;
                    for (i, &pt) in split_pts.iter().enumerate() {
                        if midi_note >= pt {
                            child_idx = i + 1;
                        }
                    }
                    child_idx = child_idx.min(self.children.len() - 1);

                    if let Some(child) = self.children.get(child_idx) {
                        trigger_child(child, midi_note, velocity, tuning_pitch, engine_sample_rate)
                    } else {
                        Vec::new()
                    }
                } else {
                    // No explicit split points — try each child and use the one
                    // that has a zone for this note
                    for child in &self.children {
                        let voices = trigger_child(child, midi_note, velocity, tuning_pitch, engine_sample_rate);
                        if !voices.is_empty() {
                            return voices;
                        }
                    }
                    Vec::new()
                }
            }
            CompositeMode::Chain => {
                // Chain mode: for now, use the first child as the sound source
                // (effects chain processing is a future enhancement)
                if let Some(child) = self.children.first() {
                    trigger_child(child, midi_note, velocity, tuning_pitch, engine_sample_rate)
                } else {
                    Vec::new()
                }
            }
        }
    }
}

fn trigger_child(
    child: &CompositeChild,
    midi_note: u8,
    velocity: f64,
    tuning_pitch: f64,
    engine_sample_rate: f64,
) -> Vec<CompositeVoice> {
    match child {
        CompositeChild::Sampler(sampler) => {
            if let Some(zone) = sampler.find_zone(midi_note) {
                let voice = SamplerVoice::new(zone, midi_note, velocity, tuning_pitch, engine_sample_rate);
                vec![CompositeVoice::Sampler(voice)]
            } else {
                Vec::new()
            }
        }
        CompositeChild::Composite(composite) => {
            composite.trigger_note(midi_note, velocity, tuning_pitch, engine_sample_rate)
        }
    }
}

/// A voice from a composite instrument (wraps the underlying voice type).
#[derive(Debug, Clone)]
pub enum CompositeVoice {
    Sampler(SamplerVoice),
    // Future: Oscillator, Effect voices
}

impl CompositeVoice {
    pub fn next_sample(&mut self) -> f64 {
        match self {
            CompositeVoice::Sampler(v) => v.next_sample(),
        }
    }

    pub fn note_off(&mut self) {
        match self {
            CompositeVoice::Sampler(v) => v.note_off(),
        }
    }

    pub fn is_finished(&self) -> bool {
        match self {
            CompositeVoice::Sampler(v) => v.is_finished(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::sampler::{SampleBuffer, LoadedZone};

    fn make_sine_buffer(freq: f64, duration: f64, sample_rate: u32) -> SampleBuffer {
        let num_samples = (sample_rate as f64 * duration) as usize;
        let data: Vec<f64> = (0..num_samples)
            .map(|i| {
                let t = i as f64 / sample_rate as f64;
                (2.0 * std::f64::consts::PI * freq * t).sin()
            })
            .collect();
        SampleBuffer::new(data, sample_rate)
    }

    fn make_zone(low: u8, high: u8, root: u8) -> LoadedZone {
        LoadedZone {
            key_range_low: low,
            key_range_high: high,
            root_note: root,
            fine_tune_cents: 0.0,
            sample_rate: 44100,
            loop_start: None,
            loop_end: None,
            buffer: make_sine_buffer(440.0, 0.5, 44100),
        }
    }

    #[test]
    fn layer_mode_multiple_voices() {
        let sampler1 = Sampler::new(vec![make_zone(0, 127, 60)], false);
        let sampler2 = Sampler::new(vec![make_zone(0, 127, 60)], false);

        let composite = CompositeInstrument::new_layer(
            vec![
                CompositeChild::Sampler(sampler1),
                CompositeChild::Sampler(sampler2),
            ],
            Some(vec![0.7, 0.3]),
        );

        let voices = composite.trigger_note(60, 1.0, 440.0, 44100.0);
        assert_eq!(voices.len(), 2, "Layer mode should produce 2 voices");
    }

    #[test]
    fn split_mode_routes_to_correct_child() {
        let low_sampler = Sampler::new(vec![make_zone(0, 60, 48)], false);
        let high_sampler = Sampler::new(vec![make_zone(61, 127, 72)], false);

        let composite = CompositeInstrument::new_split(
            vec![
                CompositeChild::Sampler(low_sampler),
                CompositeChild::Sampler(high_sampler),
            ],
            None,
        );

        // C4 (60) should find the low sampler
        let voices_low = composite.trigger_note(60, 1.0, 440.0, 44100.0);
        assert_eq!(voices_low.len(), 1, "Split should find zone for note 60");

        // C5 (72) should find the high sampler
        let voices_high = composite.trigger_note(72, 1.0, 440.0, 44100.0);
        assert_eq!(voices_high.len(), 1, "Split should find zone for note 72");
    }

    #[test]
    fn split_mode_with_explicit_points() {
        let child1 = Sampler::new(vec![make_zone(0, 127, 48)], false);
        let child2 = Sampler::new(vec![make_zone(0, 127, 72)], false);

        let composite = CompositeInstrument::new_split(
            vec![
                CompositeChild::Sampler(child1),
                CompositeChild::Sampler(child2),
            ],
            Some(vec![60]),  // Split at MIDI 60
        );

        // Note 50 should go to child 0
        let v1 = composite.trigger_note(50, 1.0, 440.0, 44100.0);
        assert_eq!(v1.len(), 1);

        // Note 72 should go to child 1
        let v2 = composite.trigger_note(72, 1.0, 440.0, 44100.0);
        assert_eq!(v2.len(), 1);
    }

    #[test]
    fn composite_voices_produce_sound() {
        let sampler = Sampler::new(vec![make_zone(0, 127, 69)], false);
        let composite = CompositeInstrument::new_layer(
            vec![CompositeChild::Sampler(sampler)],
            None,
        );

        let mut voices = composite.trigger_note(69, 1.0, 440.0, 44100.0);
        assert_eq!(voices.len(), 1);

        let mut max = 0.0_f64;
        for _ in 0..4410 {
            let s = voices[0].next_sample();
            max = max.max(s.abs());
        }

        assert!(max > 0.1, "Composite voice should produce sound, max={max}");
    }

    #[test]
    fn nested_composite() {
        let sampler = Sampler::new(vec![make_zone(0, 127, 60)], false);
        let inner = CompositeInstrument::new_layer(
            vec![CompositeChild::Sampler(sampler)],
            None,
        );
        let outer = CompositeInstrument::new_layer(
            vec![CompositeChild::Composite(Box::new(inner))],
            None,
        );

        let voices = outer.trigger_note(60, 1.0, 440.0, 44100.0);
        assert_eq!(voices.len(), 1, "Nested composite should produce 1 voice");
    }

    #[test]
    fn layer_mix_levels() {
        let sampler1 = Sampler::new(vec![make_zone(0, 127, 69)], false);
        let sampler2 = Sampler::new(vec![make_zone(0, 127, 69)], false);

        // Full volume vs half volume
        let composite = CompositeInstrument::new_layer(
            vec![
                CompositeChild::Sampler(sampler1),
                CompositeChild::Sampler(sampler2),
            ],
            Some(vec![1.0, 0.5]),
        );

        let mut voices = composite.trigger_note(69, 1.0, 440.0, 44100.0);

        // Skip attack transient
        for _ in 0..500 {
            for v in voices.iter_mut() { v.next_sample(); }
        }

        let s1 = voices[0].next_sample().abs();
        let s2 = voices[1].next_sample().abs();

        // s2 should be about half of s1 (due to mix_levels applied via velocity)
        if s1 > 0.01 {
            let ratio = s2 / s1;
            assert!(
                (ratio - 0.5).abs() < 0.2,
                "Mix level 0.5 should produce ~50% amplitude, ratio={ratio}"
            );
        }
    }

    #[test]
    fn voice_note_off_and_finish() {
        let sampler = Sampler::new(vec![make_zone(0, 127, 69)], false);
        let composite = CompositeInstrument::new_layer(
            vec![CompositeChild::Sampler(sampler)],
            None,
        );

        let mut voices = composite.trigger_note(69, 1.0, 440.0, 44100.0);
        assert!(!voices[0].is_finished());

        // Play a bit, then release
        for _ in 0..100 { voices[0].next_sample(); }
        voices[0].note_off();

        // Should eventually finish
        let mut finished = false;
        for _ in 0..100000 {
            voices[0].next_sample();
            if voices[0].is_finished() {
                finished = true;
                break;
            }
        }
        assert!(finished, "Voice should finish after note_off");
    }
}
