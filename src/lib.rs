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
