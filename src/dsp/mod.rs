//! DSP Engine — Pure Rust audio synthesis and processing.
//!
//! All DSP runs in Rust for deterministic, cross-platform audio output.
//! The same code powers both the WebAudio (via AudioWorklet + WASM) and
//! the CLI renderer (offline WAV export).

pub mod composite;
pub mod delay;
pub mod engine;
pub mod envelope;
pub mod filter;
pub mod mixer;
pub mod oscillator;
pub mod renderer;
pub mod reverb;
pub mod sampler;
pub mod tuner;
pub mod voice;
