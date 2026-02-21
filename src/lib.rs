pub mod ast;
pub mod compiler;
pub mod dsp;
pub mod error;
pub mod lexer;
pub mod parser;
pub mod preset;
pub mod token;

use crate::error::SongWalkerError;
use crate::lexer::Lexer;
use crate::parser::Parser;
use wasm_bindgen::prelude::*;

/// The crate version, read from Cargo.toml at compile time.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// WASM-exposed: return the songwalker-core version string.
#[wasm_bindgen]
pub fn core_version() -> String {
    VERSION.to_string()
}

/// Parse a `.sw` source string into a `Program` AST.
pub fn parse(input: &str) -> Result<ast::Program, SongWalkerError> {
    let tokens = Lexer::new(input).tokenize()?;
    let mut parser = Parser::new(tokens);
    Ok(parser.parse_program()?)
}

/// WASM-exposed: compile `.sw` source into a JSON event list (strict/editor mode).
/// Errors if a note plays before track.instrument is set.
#[wasm_bindgen]
pub fn compile_song(source: &str) -> Result<JsValue, JsValue> {
    let program = parse(source).map_err(|e| JsValue::from_str(&format!("{e}")))?;
    let event_list =
        compiler::compile_strict(&program).map_err(|e| JsValue::from_str(&e))?;
    serde_wasm_bindgen::to_value(&event_list).map_err(|e| JsValue::from_str(&format!("{e}")))
}

/// WASM-exposed: compile and render `.sw` source to a WAV byte array.
#[wasm_bindgen]
pub fn render_song_wav(source: &str, sample_rate: u32) -> Result<Vec<u8>, JsValue> {
    let program = parse(source).map_err(|e| JsValue::from_str(&format!("{e}")))?;
    let event_list =
        compiler::compile(&program).map_err(|e| JsValue::from_str(&e))?;
    Ok(dsp::renderer::render_wav(&event_list, sample_rate))
}

/// WASM-exposed: compile and render `.sw` source to mono f32 samples.
/// Returns the raw audio buffer for AudioWorklet playback.
#[wasm_bindgen]
pub fn render_song_samples(source: &str, sample_rate: u32) -> Result<Vec<f32>, JsValue> {
    let program = parse(source).map_err(|e| JsValue::from_str(&format!("{e}")))?;
    let event_list =
        compiler::compile(&program).map_err(|e| JsValue::from_str(&e))?;
    let engine = dsp::engine::AudioEngine::new(sample_rate as f64);
    let samples_f64 = engine.render(&event_list);
    Ok(samples_f64.iter().map(|&s| s as f32).collect())
}

/// A loaded preset zone transferred from JS → WASM.
#[derive(serde::Deserialize, Clone)]
struct WasmLoadedZone {
    #[serde(rename = "keyRangeLow")]
    key_range_low: u8,
    #[serde(rename = "keyRangeHigh")]
    key_range_high: u8,
    #[serde(rename = "rootNote")]
    root_note: u8,
    #[serde(rename = "fineTuneCents")]
    fine_tune_cents: f64,
    #[serde(rename = "sampleRate")]
    sample_rate: u32,
    #[serde(rename = "loopStart")]
    loop_start: Option<u64>,
    #[serde(rename = "loopEnd")]
    loop_end: Option<u64>,
    /// Mono f32 PCM samples, decoded on the JS side.
    samples: Vec<f32>,
}

/// A child node in a composite preset.
#[derive(serde::Deserialize, Clone)]
#[serde(tag = "type", rename_all = "lowercase")]
enum WasmLoadedChild {
    Sampler {
        zones: Vec<WasmLoadedZone>,
        #[serde(default, rename = "isDrumKit")]
        is_drum_kit: bool,
    },
    Oscillator {
        waveform: String,
        #[serde(default)]
        mixer: Option<f64>,
        #[serde(default)]
        attack: Option<f64>,
        #[serde(default)]
        decay: Option<f64>,
        #[serde(default)]
        sustain: Option<f64>,
        #[serde(default)]
        release: Option<f64>,
    },
}

/// A loaded preset transferred from JS → WASM.
/// Can be a simple sampler or a composite with multiple children.
#[derive(serde::Deserialize)]
struct WasmLoadedPreset {
    /// The preset name as it appears in loadPreset("name").
    name: String,
    /// Preset type: "sampler" or "composite"
    #[serde(default, rename = "presetType")]
    preset_type: Option<String>,
    /// Whether this is a drum kit (percussion mode) — for simple samplers.
    #[serde(default, rename = "isDrumKit")]
    is_drum_kit: bool,
    /// Loaded sample zones with PCM data — for simple samplers.
    #[serde(default)]
    zones: Vec<WasmLoadedZone>,
    /// Composite mode: "layer", "split", or "chain"
    #[serde(default)]
    mode: Option<String>,
    /// Children for composite presets.
    #[serde(default)]
    children: Vec<WasmLoadedChild>,
    /// Mix levels for layer mode.
    #[serde(default, rename = "mixLevels")]
    mix_levels: Option<Vec<f64>>,
}

/// Build a sampler from zones.
fn build_sampler_from_zones(zones: &[WasmLoadedZone], is_drum_kit: bool) -> dsp::sampler::Sampler {
    let loaded_zones = zones.iter().map(|z| {
        let buffer = dsp::sampler::SampleBuffer::from_f32(&z.samples, z.sample_rate);
        dsp::sampler::LoadedZone {
            key_range_low: z.key_range_low,
            key_range_high: z.key_range_high,
            root_note: z.root_note,
            fine_tune_cents: z.fine_tune_cents,
            sample_rate: z.sample_rate,
            loop_start: z.loop_start,
            loop_end: z.loop_end,
            buffer,
        }
    }).collect();
    dsp::sampler::Sampler::new(loaded_zones, is_drum_kit)
}

/// Build a composite child from the WASM data.
fn build_composite_child(child: &WasmLoadedChild) -> dsp::composite::CompositeChild {
    match child {
        WasmLoadedChild::Sampler { zones, is_drum_kit } => {
            dsp::composite::CompositeChild::Sampler(
                build_sampler_from_zones(zones, *is_drum_kit)
            )
        }
        WasmLoadedChild::Oscillator { waveform, mixer, attack, decay, sustain, release } => {
            dsp::composite::CompositeChild::Oscillator(compiler::InstrumentConfig {
                waveform: waveform.clone(),
                mixer: *mixer,
                attack: *attack,
                decay: *decay,
                sustain: *sustain,
                release: *release,
                ..Default::default()
            })
        }
    }
}

/// Build a preset (sampler or composite) from the WASM-transferred data.
fn build_preset(preset: &WasmLoadedPreset) -> dsp::engine::RegisteredPreset {
    // Check if this is a composite preset
    let is_composite = preset.preset_type.as_deref() == Some("composite") 
        || !preset.children.is_empty();

    if is_composite {
        let children: Vec<dsp::composite::CompositeChild> = preset.children
            .iter()
            .map(build_composite_child)
            .collect();

        let mode = match preset.mode.as_deref() {
            Some("split") => dsp::composite::CompositeMode::Split,
            Some("chain") => dsp::composite::CompositeMode::Chain,
            _ => dsp::composite::CompositeMode::Layer,
        };

        let composite = match mode {
            dsp::composite::CompositeMode::Layer => 
                dsp::composite::CompositeInstrument::new_layer(children, preset.mix_levels.clone()),
            dsp::composite::CompositeMode::Split => 
                dsp::composite::CompositeInstrument::new_split(children, None),
            dsp::composite::CompositeMode::Chain => {
                // Chain mode uses layer structure for now (effects not fully impl)
                dsp::composite::CompositeInstrument::new_layer(children, None)
            }
        };

        dsp::engine::RegisteredPreset::Composite(composite)
    } else {
        // Simple sampler preset
        let sampler = build_sampler_from_zones(&preset.zones, preset.is_drum_kit);
        dsp::engine::RegisteredPreset::Sampler(sampler)
    }
}

/// WASM-exposed: compile and render `.sw` source to mono f32 samples
/// with loaded preset data for sampler-based instruments.
///
/// `presets_json` is a JSON array of `WasmLoadedPreset` objects, each
/// containing the preset name and pre-decoded PCM zone data.
#[wasm_bindgen]
pub fn render_song_samples_with_presets(
    source: &str,
    sample_rate: u32,
    presets_json: &str,
) -> Result<Vec<f32>, JsValue> {
    let program = parse(source).map_err(|e| JsValue::from_str(&format!("{e}")))?;
    let event_list =
        compiler::compile(&program).map_err(|e| JsValue::from_str(&e))?;

    let mut engine = dsp::engine::AudioEngine::new(sample_rate as f64);

    // Deserialize and register presets (sampler or composite)
    let presets: Vec<WasmLoadedPreset> = serde_json::from_str(presets_json)
        .map_err(|e| JsValue::from_str(&format!("Failed to parse presets JSON: {e}")))?;
    for preset in &presets {
        let registered = build_preset(preset);
        match registered {
            dsp::engine::RegisteredPreset::Sampler(s) => 
                engine.register_preset(preset.name.clone(), s),
            dsp::engine::RegisteredPreset::Composite(c) => 
                engine.register_composite(preset.name.clone(), c),
        }
    }

    let samples_f64 = engine.render(&event_list);
    Ok(samples_f64.iter().map(|&s| s as f32).collect())
}

/// WASM-exposed: compile and render `.sw` source to a WAV byte array
/// with loaded preset data for sampler-based instruments.
#[wasm_bindgen]
pub fn render_song_wav_with_presets(
    source: &str,
    sample_rate: u32,
    presets_json: &str,
) -> Result<Vec<u8>, JsValue> {
    let program = parse(source).map_err(|e| JsValue::from_str(&format!("{e}")))?;
    let event_list =
        compiler::compile(&program).map_err(|e| JsValue::from_str(&e))?;

    let mut engine = dsp::engine::AudioEngine::new(sample_rate as f64);

    // Deserialize and register presets (sampler or composite)
    let presets: Vec<WasmLoadedPreset> = serde_json::from_str(presets_json)
        .map_err(|e| JsValue::from_str(&format!("Failed to parse presets JSON: {e}")))?;
    for preset in &presets {
        let registered = build_preset(preset);
        match registered {
            dsp::engine::RegisteredPreset::Sampler(s) => 
                engine.register_preset(preset.name.clone(), s),
            dsp::engine::RegisteredPreset::Composite(c) => 
                engine.register_composite(preset.name.clone(), c),
        }
    }

    let pcm = engine.render_pcm_i16(&event_list);
    Ok(dsp::renderer::encode_wav_public(&pcm, sample_rate, 2))
}

// ── Piano Keyboard: Single Note Rendering ───────────────────

/// WASM-exposed: query the compilation state at a given cursor byte offset.
///
/// Returns a JSON object with the active instrument, BPM, tuning, note length,
/// track name, and beat position at the cursor. Used by the editor to determine
/// which instrument to preview when a piano key is pressed.
#[wasm_bindgen]
pub fn get_instrument_at_cursor(
    source: &str,
    cursor_byte_offset: usize,
) -> Result<JsValue, JsValue> {
    let ctx = compiler::cursor_context(source, cursor_byte_offset)
        .map_err(|e| JsValue::from_str(&e))?;
    serde_wasm_bindgen::to_value(&ctx).map_err(|e| JsValue::from_str(&format!("{e}")))
}

/// WASM-exposed: render a single note to mono f32 PCM samples.
///
/// Used by the piano keyboard to preview notes with the instrument active
/// at the cursor. Constructs a minimal EventList, renders through the
/// AudioEngine with `EndMode::Release`, and caps at 4 seconds.
///
/// * `pitch` — note name (e.g. "C4", "A3")
/// * `velocity` — note velocity 0–127
/// * `gate_beats` — audible note duration in beats
/// * `bpm` — tempo for beat→seconds conversion
/// * `tuning_pitch` — A4 reference frequency (e.g. 440.0)
/// * `sample_rate` — output sample rate
/// * `instrument_json` — `InstrumentConfig` serialized as JSON
/// * `presets_json` — optional JSON array of loaded preset data (pass "[]" if none)
#[wasm_bindgen]
pub fn render_single_note(
    pitch: &str,
    velocity: f64,
    gate_beats: f64,
    bpm: f64,
    tuning_pitch: f64,
    sample_rate: u32,
    instrument_json: &str,
    presets_json: &str,
) -> Result<Vec<f32>, JsValue> {
    let instrument: compiler::InstrumentConfig = serde_json::from_str(instrument_json)
        .map_err(|e| JsValue::from_str(&format!("Invalid instrument JSON: {e}")))?;

    // Build a minimal EventList with one note.
    let event_list = compiler::EventList {
        events: vec![
            // Set BPM
            compiler::Event {
                time: 0.0,
                kind: compiler::EventKind::SetProperty {
                    target: "track.beatsPerMinute".to_string(),
                    value: format!("{bpm}"),
                },
                track_name: None,
            },
            // Set tuning
            compiler::Event {
                time: 0.0,
                kind: compiler::EventKind::SetProperty {
                    target: "track.tuningPitch".to_string(),
                    value: format!("{tuning_pitch}"),
                },
                track_name: None,
            },
            // The note
            compiler::Event {
                time: 0.0,
                kind: compiler::EventKind::Note {
                    pitch: pitch.to_string(),
                    velocity,
                    gate: gate_beats,
                    instrument,
                    source_start: 0,
                    source_end: 0,
                },
                track_name: None,
            },
        ],
        total_beats: gate_beats,
        end_mode: compiler::EndMode::Release,
    };

    let mut engine = dsp::engine::AudioEngine::new(sample_rate as f64);

    // Register presets if provided.
    if presets_json != "[]" && !presets_json.is_empty() {
        let presets: Vec<WasmLoadedPreset> = serde_json::from_str(presets_json)
            .map_err(|e| JsValue::from_str(&format!("Failed to parse presets JSON: {e}")))?;
        for preset in &presets {
            let registered = build_preset(preset);
            match registered {
                dsp::engine::RegisteredPreset::Sampler(s) =>
                    engine.register_preset(preset.name.clone(), s),
                dsp::engine::RegisteredPreset::Composite(c) =>
                    engine.register_composite(preset.name.clone(), c),
            }
        }
    }

    let samples_f64 = engine.render(&event_list);

    // Cap at 4 seconds.
    let max_samples = (4.0 * sample_rate as f64) as usize;
    let capped = if samples_f64.len() > max_samples {
        &samples_f64[..max_samples]
    } else {
        &samples_f64
    };

    Ok(capped.iter().map(|&s| s as f32).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_single_note_produces_audio() {
        // Build the same minimal EventList that render_single_note does,
        // but call the engine directly (no WASM).
        let instrument = compiler::InstrumentConfig::default(); // triangle
        let event_list = compiler::EventList {
            events: vec![
                compiler::Event {
                    time: 0.0,
                    kind: compiler::EventKind::SetProperty {
                        target: "track.beatsPerMinute".to_string(),
                        value: "120".to_string(),
                    },
                    track_name: None,
                },
                compiler::Event {
                    time: 0.0,
                    kind: compiler::EventKind::Note {
                        pitch: "A4".to_string(),
                        velocity: 100.0,
                        gate: 1.0,
                        instrument,
                        source_start: 0,
                        source_end: 0,
                    },
                    track_name: None,
                },
            ],
            total_beats: 1.0,
            end_mode: compiler::EndMode::Release,
        };

        let engine = dsp::engine::AudioEngine::new(44100.0);
        let samples = engine.render(&event_list);

        // Should produce non-silent output.
        assert!(!samples.is_empty());
        assert!(samples.iter().any(|&s| s.abs() > 0.001));

        // Should be capped reasonably (1 beat at 120 BPM = 0.5s + release).
        let max_samples = (4.0 * 44100.0) as usize;
        assert!(samples.len() <= max_samples);
    }
}
