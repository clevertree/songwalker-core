# SongWalker

> A music programming language — write songs as code.

**[songwalker.net](https://songwalker.net)**

## What is SongWalker?

SongWalker is a minimalist music programming language with a built-in synthesizer. Write melodies using note names, durations, tracks, and loops — then play them instantly in your browser or export to WAV.

The compiler and audio engine are written in Rust, compiled to WebAssembly for the browser and available as a native CLI for offline rendering.

## Quick Example

```
const synth = Oscillator({type: 'triangle'});
track.beatsPerMinute = 140;

melody(synth);

track melody(inst) {
    track.instrument = inst;
    track.noteLength = 1/4;

    C4 /4
    E4 /4
    G4 /4
    C5 /2

    B4 /4
    G4 /4
    E4 /4
    C4 /2

    4
}
```

## Features

- **Minimalist notation** — `C4 /4` plays middle C for a quarter beat
- **Instruments** — built-in oscillator presets with ADSR envelopes
- **Independent tracks** — reusable phrases that receive instruments via parameters
- **Control flow** — `for` loops, variables, nested track calls
- **Modifiers** — velocity (`*90`), audible duration (`@1/4`), rests (standalone numbers)
- **Chords** — simultaneous notes in one step
- **Pure Rust DSP** — deterministic audio across all platforms (anti-aliased PolyBLEP oscillators, ADSR envelopes, biquad filters)
- **WAV export** — from the browser or the CLI
- **Monaco editor** — syntax highlighting, autocomplete, keyboard shortcuts

## Project Structure

```
songwalker_core/    Rust library — parser, compiler, DSP engine
songwalker_cli/     CLI binary — offline rendering to WAV
songwalker_web/     Web editor — Vite + Monaco + WASM
docs/               Language docs and plans
archive/            Legacy codebase
```

## Getting Started

### Web Editor

Visit **[songwalker.net](https://songwalker.net)** — no install needed.

### CLI

```bash
# Render a song to WAV
cargo run --manifest-path songwalker_cli/Cargo.toml -- song.sw output.wav

# Check syntax
cargo run --manifest-path songwalker_cli/Cargo.toml -- --check song.sw

# Print AST
cargo run --manifest-path songwalker_cli/Cargo.toml -- --ast song.sw
```

### Development

```bash
# Run Rust tests (68 tests)
cd songwalker_core && cargo test

# Build WASM
cd songwalker_core && wasm-pack build --target web --out-dir ../songwalker_web/src/wasm

# Dev server
cd songwalker_web && npm run dev

# Production build
cd songwalker_web && npm run build
```

## Language Reference

### Notes
```
C4          Play C4 with default duration
C4 /4       Play C4, step 1/4 beat
Eb3 /2      Play E-flat 3, step 1/2 beat
F#5 2       Play F-sharp 5, step 2 beats
```

### Modifiers
```
C4*90 /4    Velocity 90 (out of 127)
C4@1/8 /4   Audible duration 1/8 beat, step 1/4 beat
```

### Rests
```
4           Rest for 4 beats
1/2         Rest for half a beat
```

### Tracks
```
track name(params) {
    // notes, rests, loops, track calls
}
name();     // call the track
```

### Variables
```
track.beatsPerMinute = 140;
track.noteLength = 1/4;
const synth = Oscillator({type: 'square', attack: 0.01, release: 0.2});
```

### Instruments

Instruments are created with `Oscillator({...})` and passed to tracks via parameters.
Tracks are **independent** — they inherit parent state (BPM, instrument, noteLength) and can override it locally. Best practice is to pass instruments as parameters for reusable, portable tracks.

```
const lead = Oscillator({type: 'sawtooth', attack: 0.01, release: 0.4});
const bass = Oscillator({type: 'square', sustain: 0.8});

melody(lead);
bassline(bass);

track melody(inst) {
    track.instrument = inst;
    C4 /4  E4 /4  G4 /4
}

track bassline(inst) {
    track.instrument = inst;
    C2 /2  G2 /2
}
```

**Available waveforms:** `sine`, `square`, `sawtooth`, `triangle` (default)

**ADSR envelope options:** `attack`, `decay`, `sustain`, `release` (in seconds/level)

**Other options:** `detune` (cents), `mixer` (gain level)

String shorthand is also supported: `track.instrument = 'square';`

## Architecture

The entire audio pipeline runs in Rust:

1. **Lexer** — hand-rolled tokenizer with note/modifier context
2. **Parser** — recursive descent, produces typed AST
3. **Compiler** — AST → flat EventList with resolved timings
4. **DSP Engine** — PolyBLEP oscillators → ADSR envelopes → biquad filters → soft-clip mixer
5. **Renderer** — EventList → PCM samples → WAV

The same Rust code powers both the WASM web player and the native CLI renderer, guaranteeing identical output.

## License

MIT
