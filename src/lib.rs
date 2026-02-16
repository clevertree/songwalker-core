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
#[derive(serde::Deserialize)]
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

/// A loaded preset transferred from JS → WASM.
#[derive(serde::Deserialize)]
struct WasmLoadedPreset {
    /// The preset name as it appears in loadPreset("name").
    name: String,
    /// Whether this is a drum kit (percussion mode).
    #[serde(default, rename = "isDrumKit")]
    is_drum_kit: bool,
    /// Loaded sample zones with PCM data.
    zones: Vec<WasmLoadedZone>,
}

/// Build a `Sampler` from the WASM-transferred preset data.
fn build_sampler(preset: &WasmLoadedPreset) -> dsp::sampler::Sampler {
    let zones = preset.zones.iter().map(|z| {
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
    dsp::sampler::Sampler::new(zones, preset.is_drum_kit)
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

    // Deserialize and register presets
    let presets: Vec<WasmLoadedPreset> = serde_json::from_str(presets_json)
        .map_err(|e| JsValue::from_str(&format!("Failed to parse presets JSON: {e}")))?;
    for preset in &presets {
        let sampler = build_sampler(preset);
        engine.register_preset(preset.name.clone(), sampler);
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

    let presets: Vec<WasmLoadedPreset> = serde_json::from_str(presets_json)
        .map_err(|e| JsValue::from_str(&format!("Failed to parse presets JSON: {e}")))?;
    for preset in &presets {
        let sampler = build_sampler(preset);
        engine.register_preset(preset.name.clone(), sampler);
    }

    let pcm = engine.render_pcm_i16(&event_list);
    Ok(dsp::renderer::encode_wav_public(&pcm, sample_rate, 2))
}
