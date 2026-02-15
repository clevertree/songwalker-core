//! Audio Engine — renders an EventList to audio samples.
//!
//! The engine manages voices, processes events at the correct sample offsets,
//! and produces interleaved stereo f32 output.

use crate::compiler::{EndMode, EventKind, EventList, InstrumentConfig};

use super::mixer::Mixer;
use super::voice::Voice;

/// Parse a note name (e.g. "C4", "F#3", "Bb5") into a MIDI note number.
pub fn note_to_midi(note: &str) -> Option<i32> {
    let bytes = note.as_bytes();
    if bytes.is_empty() {
        return None;
    }

    // Parse note name (A-G)
    let name = bytes[0] as char;
    let base_semitone = match name {
        'C' => 0,
        'D' => 2,
        'E' => 4,
        'F' => 5,
        'G' => 7,
        'A' => 9,
        'B' => 11,
        _ => return None,
    };

    let mut idx = 1;
    let mut semitone = base_semitone;

    // Parse accidental
    if idx < bytes.len() {
        match bytes[idx] as char {
            '#' => {
                semitone += 1;
                idx += 1;
            }
            'b' => {
                semitone -= 1;
                idx += 1;
            }
            _ => {}
        }
    }

    // Parse octave number
    let octave_str = &note[idx..];
    let octave: i32 = octave_str.parse().ok()?;

    // MIDI note number: C4 = 60
    Some((octave + 1) * 12 + semitone)
}

/// Convert a MIDI note number to frequency using the given tuning pitch.
///
/// `tuning_pitch` is the frequency of A4 (MIDI 69). Default is 440.0 Hz.
/// Formula: `tuning_pitch * 2^((midi - 69) / 12)`
pub fn midi_to_frequency(midi: i32, tuning_pitch: f64) -> f64 {
    tuning_pitch * (2.0_f64).powf((midi as f64 - 69.0) / 12.0)
}

/// Note-to-frequency conversion matching the JS `noteToFrequency`.
///
/// Uses the standard A4 = 440 Hz tuning. For custom tuning, use
/// `note_to_midi()` + `midi_to_frequency()`.
pub fn note_to_frequency(note: &str) -> Option<f64> {
    note_to_frequency_with_tuning(note, 440.0)
}

/// Note-to-frequency conversion with configurable tuning pitch.
///
/// `tuning_pitch` is the frequency of A4. Common values: 440.0, 432.0.
pub fn note_to_frequency_with_tuning(note: &str, tuning_pitch: f64) -> Option<f64> {
    let midi = note_to_midi(note)?;
    Some(midi_to_frequency(midi, tuning_pitch))
}

/// Scheduled voice event for the engine.
struct ScheduledNote {
    /// Sample offset when the note starts.
    start_sample: usize,
    /// Sample offset when the note should be released (gate off).
    release_sample: usize,
    frequency: f64,
    velocity: f64,
    /// Instrument configuration for this note.
    instrument: InstrumentConfig,
}

/// The audio rendering engine.
pub struct AudioEngine {
    pub sample_rate: f64,
    pub bpm: f64,
    /// Tuning pitch for A4 in Hz. Default is 440.0.
    pub tuning_pitch: f64,
    max_voices: usize,
}

impl AudioEngine {
    pub fn new(sample_rate: f64) -> Self {
        AudioEngine {
            sample_rate,
            bpm: 120.0,
            tuning_pitch: 440.0,
            max_voices: 64,
        }
    }

    /// Render an entire EventList to mono f64 samples.
    pub fn render(&self, event_list: &EventList) -> Vec<f64> {
        // Extract BPM and tuning from events
        let mut bpm = self.bpm;
        let mut tuning_pitch = self.tuning_pitch;
        for evt in &event_list.events {
            if let EventKind::SetProperty { target, value } = &evt.kind {
                if target == "track.beatsPerMinute" {
                    if let Ok(v) = value.parse::<f64>() {
                        bpm = v;
                    }
                } else if target == "track.tuningPitch" {
                    if let Ok(v) = value.parse::<f64>() {
                        tuning_pitch = v;
                    }
                }
            }
        }

        let cursor_samples = {
            let seconds = event_list.total_beats * 60.0 / bpm;
            (seconds * self.sample_rate) as usize
        };

        // Collect note events with their sample timings
        let mut scheduled: Vec<ScheduledNote> = Vec::new();
        for evt in &event_list.events {
            if let EventKind::Note {
                pitch,
                velocity,
                gate,
                instrument,
                ..
            } = &evt.kind
            {
                if let Some(freq) = note_to_frequency_with_tuning(pitch, tuning_pitch) {
                    let start = {
                        let s = evt.time * 60.0 / bpm;
                        (s * self.sample_rate) as usize
                    };
                    let gate_seconds = gate * 60.0 / bpm;
                    let release = start + (gate_seconds * self.sample_rate) as usize;
                    scheduled.push(ScheduledNote {
                        start_sample: start,
                        release_sample: release,
                        frequency: freq,
                        velocity: *velocity / 127.0,
                        instrument: instrument.clone(),
                    });
                }
            }
        }

        // Sort by start time
        scheduled.sort_by_key(|n| n.start_sample);

        // Compute total output length based on EndMode
        // Default envelope release is 0.3s (from Envelope::new)
        let default_release = 0.3_f64;
        // Extra tail for effects (reverb, etc.) — future-proofing
        let effects_tail_samples = (0.5 * self.sample_rate) as usize;

        let total_samples = match event_list.end_mode {
            EndMode::Gate => {
                // End at the latest gate-off (release_sample)
                let max_gate = scheduled.iter().map(|n| n.release_sample).max().unwrap_or(0);
                cursor_samples.max(max_gate)
            }
            EndMode::Release => {
                // End after all envelopes finish (per-note release time)
                let max_release = scheduled
                    .iter()
                    .map(|n| {
                        let rel = n.instrument.release.unwrap_or(default_release);
                        n.release_sample + (rel * self.sample_rate) as usize
                    })
                    .max()
                    .unwrap_or(0);
                cursor_samples.max(max_release)
            }
            EndMode::Tail => {
                // End after all notes + effect tails finish
                let max_tail = scheduled
                    .iter()
                    .map(|n| {
                        let rel = n.instrument.release.unwrap_or(default_release);
                        n.release_sample + (rel * self.sample_rate) as usize + effects_tail_samples
                    })
                    .max()
                    .unwrap_or(0);
                cursor_samples.max(max_tail)
            }
        };

        // Render in blocks
        let block_size = 128;
        let mut mixer = Mixer::new();
        let mut voices: Vec<Voice> = Vec::new();
        let mut output = vec![0.0_f64; total_samples];
        let mut next_note_idx = 0;

        let mut block_start = 0;
        while block_start < total_samples {
            let block_end = (block_start + block_size).min(total_samples);
            let this_block = block_end - block_start;

            // Activate new notes that start in this block
            while next_note_idx < scheduled.len()
                && scheduled[next_note_idx].start_sample < block_end
            {
                let note = &scheduled[next_note_idx];
                if voices.len() < self.max_voices {
                    let mut voice = Voice::with_config(self.sample_rate, &note.instrument);
                    voice.release_sample = note.release_sample;
                    voice.note_on(note.frequency, note.velocity);
                    voices.push(voice);
                }
                next_note_idx += 1;
            }

            // Check for note releases — each voice carries its own release_sample
            for voice in voices.iter_mut() {
                if voice.release_sample >= block_start && voice.release_sample < block_end {
                    voice.note_off();
                }
            }

            // Render voices into mixer
            mixer.clear(this_block);
            for voice in voices.iter_mut() {
                if !voice.is_finished() {
                    for i in 0..this_block {
                        let sample = voice.next_sample();
                        mixer.add(i, sample);
                    }
                }
            }

            // Copy mixer output to main buffer
            let mixed = mixer.output();
            for (i, &s) in mixed.iter().enumerate() {
                output[block_start + i] = s;
            }

            // Remove finished voices
            voices.retain(|v| !v.is_finished());

            block_start = block_end;
        }

        output
    }

    /// Render to interleaved stereo i16 PCM (for WAV export).
    pub fn render_pcm_i16(&self, event_list: &EventList) -> Vec<i16> {
        let mono = self.render(event_list);
        let mut stereo = Vec::with_capacity(mono.len() * 2);
        for &s in &mono {
            let sample = (s * 32767.0).round().clamp(-32768.0, 32767.0) as i16;
            stereo.push(sample); // L
            stereo.push(sample); // R
        }
        stereo
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::{EndMode, Event, EventKind, EventList, InstrumentConfig};

    fn make_simple_song() -> EventList {
        EventList {
            events: vec![
                Event {
                    time: 0.0,
                    kind: EventKind::SetProperty {
                        target: "track.beatsPerMinute".to_string(),
                        value: "120".to_string(),
                    },
                },
                Event {
                    time: 0.0,
                    kind: EventKind::Note {
                        pitch: "C4".to_string(),
                        velocity: 100.0,
                        gate: 1.0,
                        instrument: InstrumentConfig::default(),
                        source_start: 0,
                        source_end: 0,
                    },
                },
                Event {
                    time: 1.0,
                    kind: EventKind::Note {
                        pitch: "E4".to_string(),
                        velocity: 80.0,
                        gate: 1.0,
                        instrument: InstrumentConfig::default(),
                        source_start: 0,
                        source_end: 0,
                    },
                },
            ],
            total_beats: 2.0,
            end_mode: EndMode::Gate,
        }
    }

    #[test]
    fn note_to_freq_a4() {
        let f = note_to_frequency("A4").unwrap();
        assert!((f - 440.0).abs() < 0.01, "A4 should be 440Hz, got {f}");
    }

    #[test]
    fn note_to_freq_c4() {
        let f = note_to_frequency("C4").unwrap();
        assert!(
            (f - 261.63).abs() < 0.1,
            "C4 should be ~261.63Hz, got {f}"
        );
    }

    #[test]
    fn note_to_freq_accidentals() {
        let sharp = note_to_frequency("F#4").unwrap();
        let flat = note_to_frequency("Gb4").unwrap();
        assert!(
            (sharp - flat).abs() < 0.01,
            "F#4 and Gb4 should be the same frequency"
        );
    }

    // ── Tuning tests (T-1 through T-6 from test plan) ──

    #[test]
    fn tuning_default_a4_440() {
        // T-1: Default tuning, A4 = 440 Hz
        let f = note_to_frequency_with_tuning("A4", 440.0).unwrap();
        assert!((f - 440.0).abs() < 0.01, "A4@440 should be 440Hz, got {f}");
    }

    #[test]
    fn tuning_432_a4() {
        // T-2: tuningPitch = 432, A4 = 432 Hz
        let f = note_to_frequency_with_tuning("A4", 432.0).unwrap();
        assert!((f - 432.0).abs() < 0.01, "A4@432 should be 432Hz, got {f}");
    }

    #[test]
    fn tuning_432_c4() {
        // T-3: tuningPitch = 432, C4 (MIDI 60) ≈ 256.87 Hz
        let f = note_to_frequency_with_tuning("C4", 432.0).unwrap();
        let expected = 432.0 * (2.0_f64).powf((60.0 - 69.0) / 12.0);
        assert!(
            (f - expected).abs() < 0.01,
            "C4@432 should be ~{expected:.2}Hz, got {f}"
        );
    }

    #[test]
    fn tuning_440_all_midi_notes() {
        // T-4: All 128 MIDI notes match standard 12-TET table at 440Hz
        let note_names = [
            "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
        ];
        for midi in 0..128 {
            let octave = (midi / 12) - 1;
            let note_idx = midi % 12;
            let name = format!("{}{}", note_names[note_idx as usize], octave);
            if let Some(f) = note_to_frequency_with_tuning(&name, 440.0) {
                let expected = 440.0 * (2.0_f64).powf((midi as f64 - 69.0) / 12.0);
                assert!(
                    (f - expected).abs() < 0.001,
                    "MIDI {midi} ({name}) should be {expected:.3}Hz, got {f:.3}"
                );
            }
        }
    }

    #[test]
    fn note_to_midi_basic() {
        assert_eq!(note_to_midi("A4"), Some(69));
        assert_eq!(note_to_midi("C4"), Some(60));
        assert_eq!(note_to_midi("C0"), Some(12));
        assert_eq!(note_to_midi("C-1"), Some(0));
    }

    #[test]
    fn midi_to_frequency_basic() {
        assert!((midi_to_frequency(69, 440.0) - 440.0).abs() < 0.001);
        assert!((midi_to_frequency(69, 432.0) - 432.0).abs() < 0.001);
        assert!((midi_to_frequency(60, 440.0) - 261.626).abs() < 0.01);
    }

    #[test]
    fn render_with_tuning_pitch() {
        // T-5/T-6: Engine respects track.tuningPitch from events
        let engine = AudioEngine::new(44100.0);
        let song = EventList {
            events: vec![
                Event {
                    time: 0.0,
                    kind: EventKind::SetProperty {
                        target: "track.beatsPerMinute".to_string(),
                        value: "120".to_string(),
                    },
                },
                Event {
                    time: 0.0,
                    kind: EventKind::SetProperty {
                        target: "track.tuningPitch".to_string(),
                        value: "432".to_string(),
                    },
                },
                Event {
                    time: 0.0,
                    kind: EventKind::Note {
                        pitch: "A4".to_string(),
                        velocity: 100.0,
                        gate: 1.0,
                        instrument: InstrumentConfig::default(),
                        source_start: 0,
                        source_end: 0,
                    },
                },
            ],
            total_beats: 1.0,
            end_mode: EndMode::Gate,
        };
        let audio = engine.render(&song);
        // Should produce non-silent output (the tuning change is applied)
        let max = audio.iter().fold(0.0_f64, |m, &s| m.max(s.abs()));
        assert!(max > 0.01, "Tuned audio should be non-silent, max={max}");
        // Audio length should be correct: 1 beat at 120 BPM = 0.5s = 22050 samples
        assert_eq!(audio.len(), 22050);
    }

    #[test]
    fn render_produces_output() {
        let engine = AudioEngine::new(44100.0);
        let song = make_simple_song();
        let audio = engine.render(&song);

        // EndMode::Gate: last note gate ends at beat 2.0 = 1s = 44100 samples
        assert_eq!(audio.len(), 44100);

        // Should have non-zero output
        let max = audio.iter().fold(0.0_f64, |m, &s| m.max(s.abs()));
        assert!(max > 0.01, "Rendered audio should be non-silent, max={max}");
    }

    #[test]
    fn render_output_bounded() {
        let engine = AudioEngine::new(44100.0);
        let song = make_simple_song();
        let audio = engine.render(&song);

        for (i, &s) in audio.iter().enumerate() {
            assert!(
                s.abs() <= 1.0,
                "Output should be bounded to [-1, 1], sample {i} = {s}"
            );
        }
    }

    #[test]
    fn render_pcm_i16_stereo() {
        let engine = AudioEngine::new(44100.0);
        let song = make_simple_song();
        let pcm = engine.render_pcm_i16(&song);

        // Stereo: 2 channels * 44100 samples = 88200
        assert_eq!(pcm.len(), 88200);
    }

    #[test]
    fn empty_song_renders_silent() {
        let engine = AudioEngine::new(44100.0);
        let song = EventList {
            events: vec![],
            total_beats: 1.0,
            end_mode: EndMode::Gate,
        };
        let audio = engine.render(&song);

        // 1 beat at 120 BPM = 0.5s = 22050 samples
        assert_eq!(audio.len(), 22050);
        assert!(audio.iter().all(|&s| s == 0.0));
    }

    #[test]
    fn end_mode_gate_vs_tail() {
        let engine = AudioEngine::new(44100.0);

        let gate_song = EventList {
            events: vec![Event {
                time: 0.0,
                kind: EventKind::Note {
                    pitch: "A4".to_string(),
                    velocity: 100.0,
                    gate: 1.0,
                    instrument: InstrumentConfig::default(),
                    source_start: 0,
                    source_end: 0,
                },
            }],
            total_beats: 1.0,
            end_mode: EndMode::Gate,
        };

        let tail_song = EventList {
            events: vec![Event {
                time: 0.0,
                kind: EventKind::Note {
                    pitch: "A4".to_string(),
                    velocity: 100.0,
                    gate: 1.0,
                    instrument: InstrumentConfig::default(),
                    source_start: 0,
                    source_end: 0,
                },
            }],
            total_beats: 1.0,
            end_mode: EndMode::Tail,
        };

        let gate_audio = engine.render(&gate_song);
        let tail_audio = engine.render(&tail_song);

        // Tail mode should produce a longer output (includes release + effects tail)
        assert!(
            tail_audio.len() > gate_audio.len(),
            "Tail ({}) should be longer than Gate ({})",
            tail_audio.len(),
            gate_audio.len()
        );
    }

    #[test]
    fn notes_actually_stop_after_gate() {
        let engine = AudioEngine::new(44100.0);
        // Short note: gate = 0.1 beats at 120 BPM = 0.05s = 2205 samples
        // Default envelope release = 0.3s = 13230 samples
        // So after ~15435 samples + margin, output should be silent
        let song = EventList {
            events: vec![
                Event {
                    time: 0.0,
                    kind: EventKind::SetProperty {
                        target: "track.beatsPerMinute".to_string(),
                        value: "120".to_string(),
                    },
                },
                Event {
                    time: 0.0,
                    kind: EventKind::Note {
                        pitch: "A4".to_string(),
                        velocity: 100.0,
                        gate: 0.1,
                        instrument: InstrumentConfig::default(),
                        source_start: 0,
                        source_end: 0,
                    },
                },
            ],
            total_beats: 2.0,
            end_mode: EndMode::Tail,
        };

        let audio = engine.render(&song);
        // Check samples well past the gate+release are silent
        // gate=0.05s + release=0.3s = 0.35s ≈ 15435 samples, check at 20000+
        let check_start = 20000;
        let tail_max = audio[check_start..]
            .iter()
            .fold(0.0_f64, |m, &s| m.max(s.abs()));
        assert!(
            tail_max < 0.001,
            "Audio should be silent after note gate + release, max={tail_max}"
        );
    }
}
