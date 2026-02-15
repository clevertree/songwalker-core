//! Voice — A single note instance combining oscillator + envelope.

use crate::compiler::InstrumentConfig;

use super::envelope::Envelope;
use super::oscillator::{Oscillator, Waveform};

/// A single voice: one oscillator shaped by an ADSR envelope.
#[derive(Debug, Clone)]
pub struct Voice {
    pub oscillator: Oscillator,
    pub envelope: Envelope,
    /// Velocity gain [0, 1].
    pub velocity: f64,
    /// Sample offset when this voice should be released (gate off).
    pub release_sample: usize,
    /// Whether this voice has been released and envelope is done.
    finished: bool,
}

/// Parse a waveform string to a Waveform enum value.
fn parse_waveform(s: &str) -> Waveform {
    match s {
        "sine" => Waveform::Sine,
        "square" => Waveform::Square,
        "sawtooth" | "saw" => Waveform::Sawtooth,
        "triangle" => Waveform::Triangle,
        _ => Waveform::Triangle,
    }
}

impl Voice {
    pub fn new(sample_rate: f64) -> Self {
        Voice {
            oscillator: Oscillator::new(Waveform::Triangle, sample_rate),
            envelope: Envelope::new(sample_rate),
            velocity: 1.0,
            release_sample: usize::MAX,
            finished: false,
        }
    }

    /// Create a voice configured from an InstrumentConfig.
    pub fn with_config(sample_rate: f64, config: &InstrumentConfig) -> Self {
        let waveform = parse_waveform(&config.waveform);
        let mut osc = Oscillator::new(waveform, sample_rate);
        if let Some(detune) = config.detune {
            osc.detune = detune;
        }

        let mut env = Envelope::new(sample_rate);
        if let Some(a) = config.attack {
            env.attack = a;
        }
        if let Some(d) = config.decay {
            env.decay = d;
        }
        if let Some(s) = config.sustain {
            env.sustain = s;
        }
        if let Some(r) = config.release {
            env.release = r;
        }

        Voice {
            oscillator: osc,
            envelope: env,
            velocity: 1.0,
            release_sample: usize::MAX,
            finished: false,
        }
    }

    /// Start playing a note.
    pub fn note_on(&mut self, frequency: f64, velocity: f64) {
        self.oscillator.frequency = frequency;
        self.oscillator.reset();
        self.velocity = velocity;
        self.finished = false;
        self.envelope.gate_on();
    }

    /// Release the note.
    pub fn note_off(&mut self) {
        self.envelope.gate_off();
    }

    /// Generate the next sample.
    pub fn next_sample(&mut self) -> f64 {
        if self.finished {
            return 0.0;
        }

        let osc = self.oscillator.next_sample();
        let env = self.envelope.next_sample();

        if self.envelope.is_finished() {
            self.finished = true;
        }

        osc * env * self.velocity
    }

    /// Is this voice done (envelope finished)?
    pub fn is_finished(&self) -> bool {
        self.finished
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn voice_produces_sound() {
        let mut v = Voice::new(44100.0);
        v.note_on(440.0, 0.8);

        let mut has_nonzero = false;
        for _ in 0..4410 {
            let s = v.next_sample();
            if s.abs() > 0.001 {
                has_nonzero = true;
            }
        }
        assert!(has_nonzero, "Voice should produce non-zero output");
    }

    #[test]
    fn voice_silent_after_release() {
        let mut v = Voice::new(44100.0);
        v.envelope.attack = 0.001;
        v.envelope.decay = 0.001;
        v.envelope.sustain = 0.5;
        v.envelope.release = 0.01;

        v.note_on(440.0, 1.0);

        // Let it play for a bit
        for _ in 0..500 {
            v.next_sample();
        }

        v.note_off();

        // Run through release
        for _ in 0..2000 {
            v.next_sample();
        }

        assert!(v.is_finished(), "Voice should be finished after release");
        let s = v.next_sample();
        assert!(s.abs() < 0.001, "Voice should be silent, got {s}");
    }

    #[test]
    fn voice_output_range() {
        let mut v = Voice::new(44100.0);
        v.note_on(880.0, 1.0);

        for _ in 0..44100 {
            let s = v.next_sample();
            assert!(
                s.abs() <= 1.01,
                "Voice output should be within [-1, 1], got {s}"
            );
        }
    }
}
