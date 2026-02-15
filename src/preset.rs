//! Preset system types for the unified preset format.
//!
//! Supports oscillators, samplers, effects, and composite instruments.
//! These types map directly to the `preset.json` schema used by the
//! songwalker-library repository.

use serde::{Deserialize, Serialize};

// ── Preset Descriptor (top-level) ───────────────────────────

/// Top-level preset descriptor. Each preset file (`preset.json`)
/// contains one of these.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresetDescriptor {
    /// Unique identifier (e.g., "fluidr3-gm-acoustic-grand-piano").
    pub id: String,
    /// Human-readable name (e.g., "Acoustic Grand Piano").
    pub name: String,
    /// Category of this preset.
    pub category: PresetCategory,
    /// Searchable tags (e.g., ["melodic", "piano", "gm:0"]).
    pub tags: Vec<String>,
    /// Optional metadata about the preset's origin and classification.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<PresetMetadata>,
    /// Tuning analysis results (populated by the tuner tool).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tuning: Option<TuningInfo>,
    /// The actual instrument/effect graph.
    pub graph: PresetNode,
}

/// Preset categories.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PresetCategory {
    Synth,
    Sampler,
    Effect,
    Composite,
}

/// Metadata about a preset's source and classification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresetMetadata {
    /// GM program number (0-127).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gm_program: Option<u8>,
    /// GM category name (e.g., "Piano", "Guitar").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gm_category: Option<String>,
    /// Source SF2 library name (e.g., "FluidR3_GM").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_library: Option<String>,
    /// Variant index within library.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variant: Option<u32>,
    /// Author of this preset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    /// License (e.g., "MIT", "CC0").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
}

/// Tuning analysis results for a preset (populated by the tuner).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TuningInfo {
    /// Has a human/tool verified the tuning?
    pub verified: bool,
    /// Does pitch detection find a clear fundamental?
    pub is_melodic: bool,
    /// Measured fundamental frequency in Hz (if melodic).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detected_pitch_hz: Option<f64>,
    /// Expected frequency from rootNote at A4=440.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_pitch_hz: Option<f64>,
    /// Difference in cents (detected vs expected).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deviation_cents: Option<f64>,
    /// |deviationCents| > threshold (default: 10 cents).
    pub needs_adjustment: bool,
}

// ── Preset Node Graph ───────────────────────────────────────

/// A node in the preset graph. Presets are modular — they can be
/// oscillators, samplers, effects, or composites of other nodes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum PresetNode {
    Oscillator {
        config: OscillatorConfig,
    },
    Sampler {
        config: SamplerConfig,
    },
    Effect {
        #[serde(rename = "effectType")]
        effect_type: EffectType,
        config: serde_json::Value,
    },
    Composite {
        mode: CompositeMode,
        children: Vec<PresetNode>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        config: Option<CompositeConfig>,
    },
}

// ── Oscillator ──────────────────────────────────────────────

/// Configuration for an oscillator node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OscillatorConfig {
    /// Waveform type.
    pub waveform: WaveformType,
    /// Detune in cents.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detune: Option<f64>,
    /// ADSR envelope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub envelope: Option<ADSRConfig>,
    /// Mix level [0.0, 1.0].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mixer: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WaveformType {
    Sine,
    Square,
    Sawtooth,
    Triangle,
    Custom,
}

// ── Sampler ─────────────────────────────────────────────────

/// Configuration for a sampler node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SamplerConfig {
    /// Sample zones covering the MIDI key range.
    pub zones: Vec<SampleZone>,
    /// Whether this sampler is a drum kit (percussion tuning rules apply).
    #[serde(default, rename = "isDrumKit")]
    pub is_drum_kit: bool,
    /// Optional ADSR envelope override for all zones.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub envelope: Option<ADSRConfig>,
}

/// A single sample zone within a sampler.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SampleZone {
    /// MIDI key range this zone covers.
    #[serde(rename = "keyRange")]
    pub key_range: KeyRange,
    /// Optional velocity range for velocity layers (future use).
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "velocityRange")]
    pub velocity_range: Option<VelocityRange>,
    /// Pitch information for this zone's sample.
    pub pitch: ZonePitch,
    /// Native sample rate of the audio.
    #[serde(rename = "sampleRate")]
    pub sample_rate: u32,
    /// Loop points (sample offsets).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r#loop: Option<LoopPoints>,
    /// Reference to the audio data.
    pub audio: AudioReference,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyRange {
    pub low: u8,
    pub high: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VelocityRange {
    pub low: u8,
    pub high: u8,
}

/// Pitch information for a sample zone.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZonePitch {
    /// The MIDI note the sample was recorded at (0-127).
    #[serde(rename = "rootNote")]
    pub root_note: u8,
    /// Fine tune offset in cents (-100 to +100).
    #[serde(rename = "fineTuneCents")]
    pub fine_tune_cents: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopPoints {
    pub start: u64,
    pub end: u64,
}

/// Reference to audio data — can be inline or external.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum AudioReference {
    /// Raw 16-bit PCM data, base64 encoded.
    InlinePcm {
        data: String,
        #[serde(rename = "bitsPerSample")]
        bits_per_sample: u8,
    },
    /// Compressed audio file, base64 encoded.
    InlineFile {
        data: String,
        codec: AudioCodec,
    },
    /// External URL (relative to preset.json location).
    External {
        url: String,
        codec: AudioCodec,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sha256: Option<String>,
    },
    /// Content-addressed storage (hash-based lookup).
    ContentAddressed {
        hash: String,
        codec: AudioCodec,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AudioCodec {
    Wav,
    Mp3,
    Ogg,
    Flac,
}

// ── Effects ─────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EffectType {
    Reverb,
    Delay,
    Chorus,
    Eq,
    Compressor,
    Filter,
}

// ── Composite ───────────────────────────────────────────────

/// How children in a composite node are combined.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CompositeMode {
    /// All children play simultaneously, mixed together.
    Layer,
    /// Route notes to children by key range (key split).
    Split,
    /// Audio passes through children in series (effects chain).
    Chain,
}

/// Configuration for a composite node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompositeConfig {
    /// For split mode: MIDI note boundaries between children.
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "splitPoints")]
    pub split_points: Option<Vec<u8>>,
    /// For layer mode: per-child mix levels.
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "mixLevels")]
    pub mix_levels: Option<Vec<f64>>,
}

// ── ADSR Envelope ───────────────────────────────────────────

/// ADSR envelope configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ADSRConfig {
    /// Attack time in seconds.
    pub attack: f64,
    /// Decay time in seconds.
    pub decay: f64,
    /// Sustain level [0.0, 1.0].
    pub sustain: f64,
    /// Release time in seconds.
    pub release: f64,
}

// ── Catalog Entry (from index.json) ─────────────────────────

/// An entry in the library's root `index.json` catalog.
/// Contains enough metadata for search/filter without loading full preset.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogEntry {
    pub id: String,
    pub name: String,
    /// Relative path to preset.json in the library repo.
    pub path: String,
    pub category: PresetCategory,
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "gmProgram")]
    pub gm_program: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "sourceLibrary")]
    pub source_library: Option<String>,
    #[serde(default, rename = "zoneCount")]
    pub zone_count: u32,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "keyRange")]
    pub key_range: Option<KeyRange>,
    #[serde(default, rename = "tuningVerified")]
    pub tuning_verified: bool,
}

/// The root index.json structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryIndex {
    pub version: u32,
    #[serde(rename = "generatedAt")]
    pub generated_at: String,
    pub presets: Vec<CatalogEntry>,
}

// ── Playback Rate Calculations ──────────────────────────────

/// Calculate the playback rate for a sample to sound at the target pitch.
///
/// # Arguments
/// * `target_midi_note` - The MIDI note being played (0-127)
/// * `root_note` - The MIDI note the sample was recorded at
/// * `fine_tune_cents` - Fine tune offset in cents
/// * `tuning_pitch` - A4 frequency (default 440.0)
///
/// # Returns
/// The playback rate multiplier. 1.0 = original speed, 2.0 = one octave up.
pub fn sample_playback_rate(
    target_midi_note: u8,
    root_note: u8,
    fine_tune_cents: f64,
    tuning_pitch: f64,
) -> f64 {
    // Standard rate (pitch shift from root) 
    let semitone_diff = target_midi_note as f64 - root_note as f64 - fine_tune_cents / 100.0;
    let base_rate = (2.0_f64).powf(semitone_diff / 12.0);

    // Tuning adjustment (ratio to standard 440 Hz)
    let tuning_ratio = tuning_pitch / 440.0;

    base_rate * tuning_ratio
}

/// Convert legacy originalPitch/coarseTune/fineTune to rootNote and fineTuneCents.
///
/// Legacy format:
/// - `originalPitch`: centitones (MIDI × 100), e.g., 6000 = C4
/// - `coarseTune`: centitones offset
/// - `fineTune`: cents offset
///
/// Returns (root_note, fine_tune_cents).
pub fn normalize_legacy_pitch(
    original_pitch: i32,
    coarse_tune: i32,
    fine_tune: i32,
) -> (u8, f64) {
    // base_detune = originalPitch - 100 * coarseTune - fineTune
    let base_detune = original_pitch - 100 * coarse_tune - fine_tune;
    let root_note = (base_detune / 100).clamp(0, 127) as u8;
    // Remainder becomes fine tune (note: sign convention means
    // negative fineTune in legacy = positive adjustment needed)
    let fine_tune_cents = (base_detune % 100) as f64;
    (root_note, fine_tune_cents)
}

// ── GM Category Mapping ─────────────────────────────────────

/// Map a GM program number (0-127) to its category name.
pub fn gm_category(program: u8) -> &'static str {
    match program {
        0..=7 => "piano",
        8..=15 => "chromatic-percussion",
        16..=23 => "organ",
        24..=31 => "guitar",
        32..=39 => "bass",
        40..=47 => "strings",
        48..=55 => "ensemble",
        56..=63 => "brass",
        64..=71 => "reed",
        72..=79 => "pipe",
        80..=87 => "synth-lead",
        88..=95 => "synth-pad",
        96..=103 => "synth-effects",
        104..=111 => "ethnic",
        112..=119 => "percussive",
        120..=127 => "sound-effects",
        _ => "unknown",
    }
}

/// Map a GM program number to a human-readable category name.
pub fn gm_category_display(program: u8) -> &'static str {
    match program {
        0..=7 => "Piano",
        8..=15 => "Chromatic Percussion",
        16..=23 => "Organ",
        24..=31 => "Guitar",
        32..=39 => "Bass",
        40..=47 => "Strings",
        48..=55 => "Ensemble",
        56..=63 => "Brass",
        64..=71 => "Reed",
        72..=79 => "Pipe",
        80..=87 => "Synth Lead",
        88..=95 => "Synth Pad",
        96..=103 => "Synth Effects",
        104..=111 => "Ethnic",
        112..=119 => "Percussive",
        120..=127 => "Sound Effects",
        _ => "Unknown",
    }
}

// ── Tests ───────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Playback Rate Tests (S-1 through S-10) ──

    #[test]
    fn playback_rate_root_note() {
        // S-1: Play root note of sample at 440 Hz → rate = 1.0
        let rate = sample_playback_rate(60, 60, 0.0, 440.0);
        assert!((rate - 1.0).abs() < 0.0001, "Root note rate should be 1.0, got {rate}");
    }

    #[test]
    fn playback_rate_octave_up() {
        // S-2: Play one octave above root → rate = 2.0
        let rate = sample_playback_rate(72, 60, 0.0, 440.0);
        assert!((rate - 2.0).abs() < 0.0001, "Octave up rate should be 2.0, got {rate}");
    }

    #[test]
    fn playback_rate_octave_down() {
        // S-3: Play one octave below root → rate = 0.5
        let rate = sample_playback_rate(48, 60, 0.0, 440.0);
        assert!((rate - 0.5).abs() < 0.0001, "Octave down rate should be 0.5, got {rate}");
    }

    #[test]
    fn playback_rate_fine_tune() {
        // S-4: Sample with fineTune = -6 cents, play root note
        let rate = sample_playback_rate(60, 60, -6.0, 440.0);
        let expected = (2.0_f64).powf(6.0 / 1200.0); // compensate +6 cents
        assert!(
            (rate - expected).abs() < 0.0001,
            "FineTune -6¢ rate should be ~{expected}, got {rate}"
        );
    }

    #[test]
    fn playback_rate_432_tuning() {
        // S-5: Play root note at 432 Hz tuning → rate = 432/440
        let rate = sample_playback_rate(60, 60, 0.0, 432.0);
        let expected = 432.0 / 440.0;
        assert!(
            (rate - expected).abs() < 0.0001,
            "432Hz tuning rate should be ~{expected}, got {rate}"
        );
    }

    #[test]
    fn playback_rate_c4_on_c4_sample() {
        // S-6: Play C4 on sample with rootNote=60, tuning 440 → rate = 1.0
        let rate = sample_playback_rate(60, 60, 0.0, 440.0);
        assert!((rate - 1.0).abs() < 0.0001);
    }

    #[test]
    fn playback_rate_c4_on_c3_sample() {
        // S-7: Play C4 on sample with rootNote=48, tuning 440 → rate = 2.0
        let rate = sample_playback_rate(60, 48, 0.0, 440.0);
        assert!((rate - 2.0).abs() < 0.0001);
    }

    // ── Legacy Pitch Conversion Tests (S-8, S-9) ──

    #[test]
    fn normalize_pitch_simple() {
        // S-8: originalPitch=2800 → rootNote=28, fineTuneCents=0
        let (root, fine) = normalize_legacy_pitch(2800, 0, 0);
        assert_eq!(root, 28);
        assert!((fine - 0.0).abs() < 0.001);
    }

    #[test]
    fn normalize_pitch_with_fine_tune() {
        // S-9: originalPitch=6012, coarseTune=0, fineTune=-6
        // base_detune = 6012 - 0 - (-6) = 6018
        // rootNote = 6018/100 = 60, fineTuneCents = 6018%100 = 18
        let (root, fine) = normalize_legacy_pitch(6012, 0, -6);
        assert_eq!(root, 60);
        assert!((fine - 18.0).abs() < 0.001);
    }

    #[test]
    fn normalize_pitch_with_coarse_tune() {
        // originalPitch=6000, coarseTune=12, fineTune=0
        // base_detune = 6000 - 1200 - 0 = 4800
        let (root, fine) = normalize_legacy_pitch(6000, 12, 0);
        assert_eq!(root, 48);
        assert!((fine - 0.0).abs() < 0.001);
    }

    // ── GM Category Tests ──

    #[test]
    fn gm_category_mapping() {
        assert_eq!(gm_category(0), "piano");
        assert_eq!(gm_category(7), "piano");
        assert_eq!(gm_category(8), "chromatic-percussion");
        assert_eq!(gm_category(24), "guitar");
        assert_eq!(gm_category(32), "bass");
        assert_eq!(gm_category(40), "strings");
        assert_eq!(gm_category(56), "brass");
        assert_eq!(gm_category(80), "synth-lead");
        assert_eq!(gm_category(127), "sound-effects");
    }

    // ── Serialization roundtrip ──

    #[test]
    fn preset_descriptor_roundtrip() {
        let preset = PresetDescriptor {
            id: "test-oscillator".to_string(),
            name: "Test Oscillator".to_string(),
            category: PresetCategory::Synth,
            tags: vec!["melodic".to_string(), "synth".to_string()],
            metadata: None,
            tuning: None,
            graph: PresetNode::Oscillator {
                config: OscillatorConfig {
                    waveform: WaveformType::Triangle,
                    detune: None,
                    envelope: Some(ADSRConfig {
                        attack: 0.01,
                        decay: 0.1,
                        sustain: 0.7,
                        release: 0.3,
                    }),
                    mixer: None,
                },
            },
        };

        let json = serde_json::to_string(&preset).unwrap();
        let deserialized: PresetDescriptor = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.id, "test-oscillator");
        assert_eq!(deserialized.category, PresetCategory::Synth);
        assert_eq!(deserialized.tags.len(), 2);
    }

    #[test]
    fn sampler_preset_roundtrip() {
        let preset = PresetDescriptor {
            id: "test-piano".to_string(),
            name: "Test Piano".to_string(),
            category: PresetCategory::Sampler,
            tags: vec!["melodic".to_string(), "piano".to_string(), "gm:0".to_string()],
            metadata: Some(PresetMetadata {
                gm_program: Some(0),
                gm_category: Some("Piano".to_string()),
                source_library: Some("FluidR3_GM".to_string()),
                variant: Some(0),
                author: None,
                license: Some("MIT".to_string()),
            }),
            tuning: None,
            graph: PresetNode::Sampler {
                config: SamplerConfig {
                    zones: vec![
                        SampleZone {
                            key_range: KeyRange { low: 0, high: 60 },
                            velocity_range: None,
                            pitch: ZonePitch {
                                root_note: 48,
                                fine_tune_cents: 0.0,
                            },
                            sample_rate: 44100,
                            r#loop: Some(LoopPoints {
                                start: 12345,
                                end: 56789,
                            }),
                            audio: AudioReference::External {
                                url: "zone_C3.wav".to_string(),
                                codec: AudioCodec::Wav,
                                sha256: None,
                            },
                        },
                        SampleZone {
                            key_range: KeyRange { low: 61, high: 127 },
                            velocity_range: None,
                            pitch: ZonePitch {
                                root_note: 72,
                                fine_tune_cents: 0.0,
                            },
                            sample_rate: 44100,
                            r#loop: None,
                            audio: AudioReference::External {
                                url: "zone_C5.wav".to_string(),
                                codec: AudioCodec::Wav,
                                sha256: None,
                            },
                        },
                    ],
                    is_drum_kit: false,
                    envelope: None,
                },
            },
        };

        let json = serde_json::to_string_pretty(&preset).unwrap();
        let deserialized: PresetDescriptor = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.id, "test-piano");
        if let PresetNode::Sampler { config } = &deserialized.graph {
            assert_eq!(config.zones.len(), 2);
            assert_eq!(config.zones[0].pitch.root_note, 48);
            assert_eq!(config.zones[1].key_range.low, 61);
        } else {
            panic!("Expected sampler node");
        }
    }

    #[test]
    fn composite_preset_roundtrip() {
        let preset = PresetDescriptor {
            id: "layered-test".to_string(),
            name: "Layered Test".to_string(),
            category: PresetCategory::Composite,
            tags: vec!["composite".to_string(), "layered".to_string()],
            metadata: None,
            tuning: None,
            graph: PresetNode::Composite {
                mode: CompositeMode::Layer,
                children: vec![
                    PresetNode::Oscillator {
                        config: OscillatorConfig {
                            waveform: WaveformType::Sine,
                            detune: None,
                            envelope: None,
                            mixer: Some(0.5),
                        },
                    },
                    PresetNode::Oscillator {
                        config: OscillatorConfig {
                            waveform: WaveformType::Triangle,
                            detune: Some(7.0),
                            envelope: None,
                            mixer: Some(0.3),
                        },
                    },
                ],
                config: Some(CompositeConfig {
                    split_points: None,
                    mix_levels: Some(vec![0.7, 0.3]),
                }),
            },
        };

        let json = serde_json::to_string_pretty(&preset).unwrap();
        let deserialized: PresetDescriptor = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.category, PresetCategory::Composite);
        if let PresetNode::Composite { mode, children, .. } = &deserialized.graph {
            assert_eq!(*mode, CompositeMode::Layer);
            assert_eq!(children.len(), 2);
        } else {
            panic!("Expected composite node");
        }
    }

    #[test]
    fn catalog_entry_roundtrip() {
        let entry = CatalogEntry {
            id: "test".to_string(),
            name: "Test".to_string(),
            path: "instruments/piano/test/preset.json".to_string(),
            category: PresetCategory::Sampler,
            tags: vec!["piano".to_string()],
            gm_program: Some(0),
            source_library: Some("FluidR3_GM".to_string()),
            zone_count: 22,
            key_range: Some(KeyRange { low: 0, high: 127 }),
            tuning_verified: false,
        };

        let json = serde_json::to_string(&entry).unwrap();
        let deserialized: CatalogEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.zone_count, 22);
    }
}
