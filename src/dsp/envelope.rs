//! ADSR Envelope generator.

/// Envelope stages.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Stage {
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

/// ADSR Envelope with linear attack/decay/release curves.
#[derive(Debug, Clone)]
pub struct Envelope {
    /// Attack time in seconds.
    pub attack: f64,
    /// Decay time in seconds.
    pub decay: f64,
    /// Sustain level [0, 1].
    pub sustain: f64,
    /// Release time in seconds.
    pub release: f64,

    stage: Stage,
    level: f64,
    sample_rate: f64,
    /// Samples remaining in current stage.
    stage_samples: usize,
    stage_counter: usize,
    /// Level at the start of the current stage (for release).
    start_level: f64,
}

impl Envelope {
    pub fn new(sample_rate: f64) -> Self {
        Envelope {
            attack: 0.01,
            decay: 0.1,
            sustain: 0.7,
            release: 0.3,
            stage: Stage::Idle,
            level: 0.0,
            sample_rate,
            stage_samples: 0,
            stage_counter: 0,
            start_level: 0.0,
        }
    }

    /// Trigger the envelope (note on).
    pub fn gate_on(&mut self) {
        self.stage = Stage::Attack;
        self.stage_samples = (self.attack * self.sample_rate) as usize;
        self.stage_counter = 0;
        self.start_level = self.level; // retrigger from current level
    }

    /// Release the envelope (note off).
    pub fn gate_off(&mut self) {
        if self.stage == Stage::Idle {
            return;
        }
        self.stage = Stage::Release;
        self.stage_samples = (self.release * self.sample_rate) as usize;
        self.stage_counter = 0;
        self.start_level = self.level;
    }

    /// Generate the next envelope sample [0, 1].
    pub fn next_sample(&mut self) -> f64 {
        match self.stage {
            Stage::Idle => {
                self.level = 0.0;
            }
            Stage::Attack => {
                if self.stage_samples == 0 {
                    self.level = 1.0;
                    self.enter_decay();
                } else {
                    let t = self.stage_counter as f64 / self.stage_samples as f64;
                    self.level = self.start_level + (1.0 - self.start_level) * t;
                    self.stage_counter += 1;
                    if self.stage_counter >= self.stage_samples {
                        self.level = 1.0;
                        self.enter_decay();
                    }
                }
            }
            Stage::Decay => {
                if self.stage_samples == 0 {
                    self.level = self.sustain;
                    self.stage = Stage::Sustain;
                } else {
                    let t = self.stage_counter as f64 / self.stage_samples as f64;
                    self.level = 1.0 - (1.0 - self.sustain) * t;
                    self.stage_counter += 1;
                    if self.stage_counter >= self.stage_samples {
                        self.level = self.sustain;
                        self.stage = Stage::Sustain;
                    }
                }
            }
            Stage::Sustain => {
                self.level = self.sustain;
            }
            Stage::Release => {
                if self.stage_samples == 0 {
                    self.level = 0.0;
                    self.stage = Stage::Idle;
                } else {
                    let t = self.stage_counter as f64 / self.stage_samples as f64;
                    self.level = self.start_level * (1.0 - t);
                    self.stage_counter += 1;
                    if self.stage_counter >= self.stage_samples {
                        self.level = 0.0;
                        self.stage = Stage::Idle;
                    }
                }
            }
        }
        self.level
    }

    /// Returns true if the envelope has finished (idle after release).
    pub fn is_finished(&self) -> bool {
        self.stage == Stage::Idle
    }

    fn enter_decay(&mut self) {
        self.stage = Stage::Decay;
        self.stage_samples = (self.decay * self.sample_rate) as usize;
        self.stage_counter = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_idle() {
        let env = Envelope::new(44100.0);
        assert!(env.is_finished());
    }

    #[test]
    fn attack_reaches_one() {
        let mut env = Envelope::new(44100.0);
        env.attack = 0.01; // 441 samples
        env.gate_on();

        let mut max_level = 0.0;
        for _ in 0..500 {
            let s = env.next_sample();
            if s > max_level {
                max_level = s;
            }
        }
        assert!(
            (max_level - 1.0).abs() < 0.01,
            "Attack should reach ~1.0, got {max_level}"
        );
    }

    #[test]
    fn sustain_holds() {
        let mut env = Envelope::new(44100.0);
        env.attack = 0.001;
        env.decay = 0.001;
        env.sustain = 0.6;
        env.gate_on();

        // Run past attack + decay
        for _ in 0..500 {
            env.next_sample();
        }

        // Should be at sustain level
        let s = env.next_sample();
        assert!(
            (s - 0.6).abs() < 0.01,
            "Should sustain at 0.6, got {s}"
        );
    }

    #[test]
    fn release_to_zero() {
        let mut env = Envelope::new(44100.0);
        env.attack = 0.001;
        env.decay = 0.001;
        env.sustain = 0.7;
        env.release = 0.01;
        env.gate_on();

        // Run past attack + decay into sustain
        for _ in 0..500 {
            env.next_sample();
        }

        env.gate_off();

        // Run through release
        for _ in 0..1000 {
            env.next_sample();
        }

        assert!(env.is_finished(), "Should be finished after release");
        assert!(env.level.abs() < 0.001, "Level should be ~0 after release");
    }

    #[test]
    fn full_cycle_range() {
        let mut env = Envelope::new(44100.0);
        env.attack = 0.01;
        env.decay = 0.05;
        env.sustain = 0.5;
        env.release = 0.1;
        env.gate_on();

        for _ in 0..10000 {
            let s = env.next_sample();
            assert!(s >= 0.0 && s <= 1.0, "Envelope out of range: {s}");
        }

        env.gate_off();
        for _ in 0..10000 {
            let s = env.next_sample();
            assert!(s >= 0.0 && s <= 1.0, "Envelope out of range after release: {s}");
        }

        assert!(env.is_finished());
    }
}
