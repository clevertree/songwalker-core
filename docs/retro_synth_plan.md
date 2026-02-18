# Retro Game System Synthesizer Plan

> **Status:** Proposal — no code changes yet  
> **Scope:** Enhance the built-in oscillator and preset system to emulate NES (2A03/2A07) and Game Boy (DMG / CGB) sound hardware, with an extensible architecture for other retro systems.

---

## 1. Background

Classic game consoles used dedicated sound generators with fixed channel types, hard constraints, and characteristic imperfections. Two systems are the initial targets:

| System | Sound Chip | CPU | Channels |
|--------|-----------|-----|----------|
| **NES** | Ricoh 2A03 (NTSC) / 2A07 (PAL) | MOS 6502 derivative | 2× pulse, 1× triangle, 1× noise, 1× DPCM |
| **Game Boy** | Custom (DMG-CPU / CGB-CPU) | Sharp SM83 (Z80-derived) | 2× pulse, 1× wave, 1× noise |

Both are clocked from the CPU and produce digital output that passes through a DAC and analog stage before reaching the speaker. The Game Boy is especially interesting because **each hardware revision (DMG, MGB, CGB, GBA)** has measurably different analog characteristics — capacitor values, amplifier bias, DC offset, and low-pass filtering all vary. The chiptune community focuses on matching the **generator output** (the digital signal at the DAC), not the speaker/headphone output, because the analog stage is model-specific.

### Why generator-level emulation?

1. **Consistency** — the digital output is the canonical sound; analog variations are subjective.
2. **Composability** — users can layer their own filters/effects on top of a clean generator signal.
3. **Accuracy** — matching the 4-bit / 8-bit DAC quantization and update rates captures the essential character.

We can optionally offer analog-stage modeling as a post-effect preset for users who want that specific Game Boy model's color.

> **Note — ProSoundMod:** A very common hardware modification ("ProSoundMod") brings the audio output pins straight to a second audio jack, modulated only by the volume knob, bypassing the console's built-in amp and filtering entirely. This reinforces the focus on generator-level emulation — many real-world users are already listening to the raw generator output through a ProSoundMod.

---

## 2. Current State of the Oscillator

### What exists

- **Waveforms:** Sine, Square (50% duty, PolyBLEP), Sawtooth (PolyBLEP), Triangle
- **Per-voice chain:** Oscillator → ADSR Envelope → velocity scaling
- **Preset schema:** `WaveformType::Custom` exists but has no runtime implementation
- **Filter:** `BiquadFilter` module exists (LP/HP/BP/Notch/Peaking) but is **not wired into the voice chain**

### What's missing for retro emulation

| Feature | Needed for |
|---------|-----------|
| Noise generator | NES noise, GB noise |
| Pulse-width / duty cycle control | NES pulse (12.5%, 25%, 50%, 75%), GB pulse (same) |
| Wavetable playback (arbitrary 32-sample wave) | GB wave channel |
| DPCM / delta sample playback | NES DPCM channel |
| DAC bit-depth quantization | Both (4-bit GB, ~4-bit NES triangle) |
| Stepped frequency (period register emulation) | Both — pitch is quantized to 11-bit (NES) / 11-bit (GB) period values |
| Frequency sweep (hardware sweep unit) | GB pulse channel 1, NES pulse sweeps |
| Length counter / auto-stop | Both |
| Volume envelope (linear, hardware-style) | NES pulse/noise, GB pulse/noise (4-bit, fixed-rate ramp) |
| LFSR noise (15-bit and 7-bit modes) | NES noise, GB noise |
| Per-voice filter integration | Analog stage modeling |
| LFO / vibrato | Not in hardware, but useful for expressive patches |

---

## 3. Proposed Architecture

### 3.1 New oscillator modes

Extend the `Waveform` enum in `oscillator.rs`:

```
Waveform
├── Sine              (existing)
├── Square            (existing — add duty_cycle field)
├── Sawtooth          (existing)
├── Triangle          (existing)
├── Noise { mode }    (NEW — LFSR-based, 15-bit or 7-bit)
├── Wavetable { table: [u8; 32], bit_depth: u8 }   (NEW)
└── DPCM { samples: Vec<u8>, rate_index: u8 }      (NEW)
```

**Pulse-width modulation:** Add `duty_cycle: f64` field to `Oscillator`. Default 0.5. The PolyBLEP square generation already computes phase thresholds — extending to variable duty is straightforward.

**Noise (LFSR):** Implement a 15-bit linear feedback shift register matching the NES/GB algorithm:
- **Long mode (15-bit):** taps at bits 0, 1 → white-ish noise
- **Short mode (7-bit):** taps at bits 0, 1 with bit 6 forced → metallic/tonal noise
- Clocked at a configurable divider rate (NES has 16 fixed rates; GB has 8 base dividers × 2^shift)

**Wavetable (GB wave channel):** Play back a 32-nibble (4-bit) waveform at a given period. The user defines the wave shape as 32 values (0–15). This is the simplest wavetable — one cycle, no interpolation, just sample-and-hold at the right rate.

**DPCM:** 1-bit delta-encoded sample playback at one of 16 fixed rates. Lower priority — fewer use cases in a music language context.

### 3.2 Retro synthesis parameters

New fields for `InstrumentConfig` / `OscillatorConfig`:

```rust
// Pulse
duty_cycle: Option<f64>,          // 0.0–1.0, default 0.5

// Noise
noise_mode: Option<NoiseMode>,    // Long15 | Short7
noise_divider: Option<u16>,       // clock divider (period)

// Wavetable
wave_table: Option<[u8; 32]>,     // 4-bit samples (0–15)
wave_bit_depth: Option<u8>,       // playback bit depth (default 4)

// DAC quantization
dac_bits: Option<u8>,             // quantize output to N-bit (e.g. 4 for GB)

// Frequency quantization
period_bits: Option<u8>,          // quantize frequency to N-bit period register

// Hardware envelope (alternative to ADSR)
hw_envelope: Option<HwEnvelope>,  // { initial_volume, direction, step_rate }

// Length counter
length_ticks: Option<u32>,        // auto-stop after N ticks (at 256 Hz frame rate)

// Sweep
sweep: Option<SweepConfig>,      // { period, direction, shift }
```

### 3.3 Unified Voice with optional feature flags

**Design decision: single instrument, not a separate RetroVoice.**

All retro features decompose into orthogonal, composable layers that extend the existing `Voice` + `Oscillator` via `Option<T>` fields. A separate voice type would create an artificial boundary that prevents mixing retro and modern features.

```
Voice (extended)
  ├── Oscillator          — existing waveforms + new variants (noise, wavetable)
  │   ├── duty_cycle: Option<f64>         — only affects square/pulse
  │   └── dac_bits: Option<u8>            — post-sample bit-crush
  ├── EnvelopeMode        — enum { ADSR(AdsrConfig) | Hardware(HwConfig) }
  ├── sweep: Option<SweepUnit>            — skipped when None
  ├── length_counter: Option<LengthCounter> — skipped when None
  └── velocity
```

**Why unified?**

- Retro waveform + modern ADSR is a valid and useful combination
- DAC quantization on a normal triangle gives instant chiptune flavor
- Noise through a biquad filter requires the filter to be in the same voice
- All new fields are `Option<T>` — the fast path for Sine/Square/Triangle/Saw with ADSR checks `None` and skips (one predicted-not-taken branch per feature per sample)
- `Voice` grows ~40 bytes of `Option` fields — negligible vs. the 64-voice polyphony cap
- `AudioEngine` keeps a single voice pool and a single allocation path

Users can freely combine features:

```sw
# Retro waveform with modern ADSR
track.instrument = Oscillator({ type: 'pulse', duty: 0.125, dac_bits: 4, attack: 0.1, release: 0.3 })

# Normal triangle with DAC quantization for chiptune color
track.instrument = Oscillator({ type: 'triangle', dac_bits: 4 })

# Noise with biquad filter
track.instrument = Oscillator({ type: 'noise', noise_mode: 'short', filter: 'lowpass', cutoff: 2000 })

# Full hardware emulation with hw envelope + sweep
track.instrument = loadPreset("NES_Pulse1")
```

### 3.4 Preset-driven system configuration

The key design goal: **users configure retro synths entirely through presets**, not by learning hardware register semantics. The preset system already supports `PresetNode::Oscillator` with `OscillatorConfig`. We extend this to carry retro parameters.

#### Built-in presets (shipped with songwalker-core)

```
NES_Pulse1        — pulse, duty 50%, hw envelope, NES period quantization
NES_Pulse2        — same as Pulse1 (NES channels 1 & 2 are identical)
NES_Triangle      — triangle, 4-bit DAC quantization, no volume control
NES_Noise         — LFSR noise, 15-bit mode, 16-rate table
NES_Noise_Metal   — LFSR noise, 7-bit mode (metallic/tonal)

GB_Pulse1         — pulse, duty 50%, sweep unit, hw envelope, GB period table
GB_Pulse2         — pulse, duty 50%, hw envelope (no sweep)
GB_Wave           — wavetable, default sine wave table, 4-bit DAC
GB_Noise          — LFSR noise, 15-bit/7-bit selectable, 4-bit volume

GB_Pulse_25       — pulse, duty 25% (common Game Boy sound)
GB_Pulse_75       — pulse, duty 75% (inverted 25% — may produce interesting effects when superimposed with 25%)
GB_Pulse_125      — pulse, duty 12.5% (thin/nasal)
```

> **Note — GB_Pulse_75:** 75% duty is the inverse of 25%. While seemingly redundant, superimposing the two could create interesting phase-cancellation or reinforcement effects. This should be verified with an oscilloscope.

#### Composite presets for full-system emulation

```
NES_Full          — Composite(Layer) of Pulse1 + Pulse2 + Triangle + Noise
GB_Full           — Composite(Layer) of Pulse1 + Pulse2 + Wave + Noise
```

#### Usage in .sw files

```
# Single channel
track.instrument = loadPreset("NES_Pulse1")
C4 D4 E4 F4

# Override duty cycle inline
track.instrument = Oscillator({ type: 'pulse', duty: 0.25, dac_bits: 4 })

# Full system in a composition
track drums
  instrument = loadPreset("NES_Noise")
  ...

track bass
  instrument = loadPreset("NES_Triangle")
  ...

track lead
  instrument = loadPreset("NES_Pulse1")
  ...
```

---

## 4. NES (2A03) Emulation Details

### 4.1 Pulse channels (×2)

- 8 octaves of pitch (11-bit period register → frequency = CPU_CLOCK / (16 × (period + 1)))
- 4 duty cycle settings: 12.5%, 25%, 50%, 75%
- Volume envelope: 4-bit (0–15), with optional hardware decay (divider-based, linear ramp down)
- Sweep unit: period register is shifted right and added/subtracted every N half-frames
- Length counter: loads from a lookup table, counts down at 60 Hz

**Implementation approach:** `Oscillator` in square/pulse mode with configurable `duty_cycle`. The PolyBLEP technique generalizes to arbitrary duty by adjusting the transition point. Apply `dac_bits: 4` quantization to the volume level. Sweep and length counter are optional `Voice` fields, clocked by the frame sequencer.

### 4.2 Triangle channel

- Same 11-bit period register
- Outputs a 32-step quantized triangle (values 15,14,...,1,0,0,1,...,14,15 repeating)
- **No volume control** — always at full amplitude (or muted)
- Has a "linear counter" that controls note length

**Implementation approach:** Generate a standard triangle wave, then quantize to 16 levels (4-bit). The stepped output is the key tonal characteristic — it adds subtle harmonics at high frequencies.

### 4.3 Noise channel

- 15-bit LFSR, feedback from bits 0 and 1 (XOR), shifted right
- Mode flag: when set, also feeds back to bit 6 → 93-step loop (metallic tone)
- 16 fixed period rates (from very fast hiss to slow rumble)
- Same volume envelope as pulse channels

**Implementation approach:** Implement `LfsrNoise` struct with `shift_register: u16`, `short_mode: bool`, `period: u16`. Clock it at the configured rate relative to the sample rate. Output the low bit, scale by 4-bit volume.

### 4.4 DPCM channel

- Plays back 1-bit delta-encoded samples
- 7-bit output counter, incremented/decremented per delta bit
- 16 fixed sample rates
- Samples stored as raw bytes (each byte = 8 delta bits)

**Implementation approach:** Lower priority. Could be implemented as a specialized sampler mode. Most useful for drums and vocal samples in NES music. Can be deferred to a later phase.

---

## 5. Game Boy (DMG / CGB) Emulation Details

### 5.1 Sound generator architecture

The Game Boy's sound system is clocked from the CPU's master clock (4.194304 MHz on DMG). A **frame sequencer** clocked at 512 Hz drives the length counters (256 Hz), volume envelopes (64 Hz), and frequency sweep (128 Hz).

All channels output to a 4-bit DAC (0–15), then through a mixer that supports panning (left/right/both).

### 5.2 Pulse channels (1 & 2)

Nearly identical to NES pulse:
- 11-bit period register → frequency = 131072 / (2048 - period)
- 4 duty cycle settings: 12.5%, 25%, 50%, 75%
- 4-bit volume envelope: initial volume + direction (up/down) + step period (0 = disabled)
- Channel 1 only: frequency sweep (period, direction, shift)
- Length counter: 6-bit (0–63), clocked at 256 Hz

### 5.3 Wave channel

The Game Boy's most distinctive sound feature:
- 32 × 4-bit samples stored in wave RAM (16 bytes)
- Plays the wave at a frequency determined by an 11-bit period register
- Output level: 0%, 25%, 50%, 100% (2-bit volume selector, implemented as right-shift)
- Default wave table varies by Game Boy model (some models initialize to a recognizable pattern)

**Common wave tables:**

| Name | Pattern | Sound |
|------|---------|-------|
| Square 50% | `FFFFFFFF00000000` | Identical to pulse at 50% duty |
| Saw | `0123456789ABCDEF` | Sawtooth approximation |
| Triangle | `0248ACE...ECA842` | Soft triangle |
| Bass | custom | Deep bass tone |
| Strings | custom | Formant-like |

**Implementation approach:** `WavetableOscillator` that reads from a 32-sample buffer. Sample-and-hold (no interpolation) to preserve the stepped character. Volume is applied as a right-shift of the 4-bit output (matching hardware behavior).

### 5.4 Noise channel

Same LFSR concept as NES but with different clocking:
- Base divider (0–7) × clock shift (0–13) determines rate
- Divider 0 is treated as 0.5 (i.e., rate = master / (divider × 2^shift))
- 15-bit and 7-bit modes (same as NES)
- Same 4-bit volume envelope as pulse channels

### 5.5 Hardware revision differences

This is the critical nuance. The chiptune community has characterized several revisions:

| Model | DAC | Key Characteristics |
|-------|-----|-------------------|
| **DMG (original)** | Capacitor-coupled | High-pass filter from coupling caps, "pop" on channel enable, bass roll-off |
| **MGB (Game Boy Pocket)** | Capacitor-coupled | Cleaner than DMG, less bass, different cap values |
| **CGB (Game Boy Color)** | Direct-coupled | No high-pass effect, different noise floor, brighter |
| **GBA (Game Boy Advance)** | Resampled | Runs GB sound at 2× speed then resamples — slight aliasing artifacts |

**Strategy: model the generator, offer the analog stage as optional.**

The generator (LFSR, pulse, wave, triangle logic) is identical across models. The differences are all in the analog path:
1. **DC offset removal** — DMG/MGB have coupling capacitors that create a high-pass filter (~20 Hz on DMG, higher on MGB)
2. **Amplifier bias and gain** — different op-amp configurations
3. **Low-pass filtering** — anti-aliasing characteristics differ

We propose to:
- **Default:** Output the clean generator signal (digital, at the DAC output point)
- **Optional analog model presets** that apply post-processing:

```
GB_DMG_Analog     — high-pass ~20Hz, slight bass boost from cap resonance
GB_MGB_Analog     — high-pass ~40Hz, cleaner profile
GB_CGB_Analog     — no high-pass, slight brightness boost
GB_GBA_Analog     — 2× resample artifact simulation
```

These would be implemented as `PresetNode::Effect` chains applied after the generator.

---

## 6. Implementation Strategy

### Phase 1: Core oscillator extensions

**Effort:** Medium  
**Files:** `oscillator.rs`, `voice.rs`, `compiler.rs`, `preset.rs`

1. Add `duty_cycle: f64` to `Oscillator`, update PolyBLEP square generation
2. Add `Waveform::Noise` with LFSR implementation (15-bit and 7-bit modes)
3. Add `Waveform::Wavetable` with 32-sample buffer playback
4. Add `DacQuantizer` — simple bit-crush: `output = round(sample * levels) / levels`
5. Wire `BiquadFilter` into `Voice` (it already exists, just needs plumbing)
6. Add new `InstrumentConfig` fields: `duty_cycle`, `noise_mode`, `dac_bits`
7. Update `evaluate_instrument_expr()` in the compiler to parse new fields
8. Tests for each new waveform

### Phase 2: Hardware envelope and frame sequencer

**Effort:** Medium  
**Files:** new `dsp/hw_envelope.rs`, `dsp/sweep.rs`, `dsp/length_counter.rs`, modified `voice.rs`

1. `HwEnvelope` — 4-bit volume, direction, step rate, clocked at 64 Hz equivalent
2. `SweepUnit` — period, direction, shift, clocked at 128 Hz equivalent
3. `LengthCounter` — tick count, clocked at 256 Hz
4. `FrameSequencer` — shared clock divider that drives the above three modules
5. Refactor `Voice.envelope` from `Envelope` to `EnvelopeMode { ADSR(Envelope) | Hardware(HwEnvelope) }`
6. Add `Option<SweepUnit>` and `Option<LengthCounter>` fields to `Voice`
7. Update `Voice::next_sample()` to tick optional modules when present (no-op when `None`)

### Phase 3: Built-in presets and .sw syntax support

**Effort:** Low–Medium  
**Files:** `preset.rs`, `compiler.rs`, new `presets/` embedded resource

1. Define all NES/GB built-in presets as `PresetDescriptor` constants
2. Register them automatically when the engine initializes (or on first use)
3. Extend `.sw` syntax support:
   - `loadPreset("NES_Pulse1")`
   - Inline overrides: `Oscillator({ type: 'pulse', duty: 0.125, dac_bits: 4 })`
   - Named wave tables: `Oscillator({ type: 'wavetable', wave: 'saw' })`
4. Custom wave table definition in `.sw`:
   ```
   wave myWave = [0,1,2,3,4,5,6,7,8,9,A,B,C,D,E,F,F,E,D,C,B,A,9,8,7,6,5,4,3,2,1,0]
   track.instrument = Oscillator({ type: 'wavetable', wave: myWave })
   ```

### Phase 4: Analog stage modeling (optional post-effects)

**Effort:** Low  
**Files:** `dsp/filter.rs` (reuse biquad), new effect presets

1. High-pass filter presets for DMG/MGB coupling cap simulation
2. Gain/bias adjustment for different amp models
3. Optional GBA resample artifact (decimate + interpolate)
4. Package as `PresetNode::Effect` chains in composite presets

### Phase 5: Additional systems (future)

Extensible to other Z80-family and retro systems:

| System | Chip | Notes |
|--------|------|-------|
| Sega Master System | SN76489 (Texas Instruments) | 3× square (10-bit period) + 1× noise (LFSR with 3 feedback modes) |
| Sega Genesis | Yamaha YM2612 + SN76489 | 6× 4-operator FM + 3× square + noise — significant scope increase |
| ZX Spectrum | Beeper / AY-3-8910 | 1-bit beeper or 3× square + noise + envelope |
| Commodore 64 | SID 6581/8580 | 3× osc (pulse/saw/tri/noise) + filter + ring mod + sync — the "holy grail" of retro synth |

The architecture from Phases 1–2 (configurable oscillator + LFSR + DAC quantization + hardware envelope) directly supports SMS and Spectrum. FM synthesis (Genesis) and SID would require dedicated modules.

---

## 7. Preset Configuration Examples

### NES Pulse (built-in preset definition)

```rust
PresetDescriptor {
    id: "NES_Pulse1".into(),
    name: "NES Pulse 1".into(),
    category: PresetCategory::Synth,
    tags: vec!["nes", "chiptune", "pulse", "8bit"],
    graph: PresetNode::Oscillator(OscillatorConfig {
        waveform: WaveformType::Square,
        duty_cycle: Some(0.5),
        detune: 0.0,
        dac_bits: Some(4),
        period_bits: Some(11),
        envelope: ADSRConfig { attack: 0.0, decay: 0.0, sustain: 1.0, release: 0.01 },
        hw_envelope: Some(HwEnvelopeConfig {
            initial_volume: 15,
            direction: EnvelopeDirection::Down,
            step_period: 0, // disabled — use ADSR instead
        }),
        ..Default::default()
    }),
    ..Default::default()
}
```

### GB Wave with custom table

```rust
PresetDescriptor {
    id: "GB_Wave_Saw".into(),
    name: "Game Boy Wave (Sawtooth)".into(),
    category: PresetCategory::Synth,
    tags: vec!["gameboy", "chiptune", "wave", "saw"],
    graph: PresetNode::Oscillator(OscillatorConfig {
        waveform: WaveformType::Custom,
        wave_table: Some([0,0,1,1,2,2,3,3,4,4,5,5,6,6,7,7,
                          8,8,9,9,10,10,11,11,12,12,13,13,14,14,15,15]),
        wave_bit_depth: Some(4),
        dac_bits: Some(4),
        period_bits: Some(11),
        envelope: ADSRConfig { attack: 0.0, decay: 0.0, sustain: 1.0, release: 0.005 },
        ..Default::default()
    }),
    ..Default::default()
}
```

### GB DMG Analog (post-processing composite)

```rust
PresetDescriptor {
    id: "GB_DMG_Pulse_Analog".into(),
    name: "Game Boy DMG Pulse (Analog)".into(),
    category: PresetCategory::Composite,
    graph: PresetNode::Composite(CompositeConfig {
        mode: CompositeMode::Chain,
        children: vec![
            PresetNode::Oscillator(/* GB_Pulse1 config */),
            PresetNode::Effect(EffectConfig {
                effect_type: EffectType::Filter,
                config: json!({
                    "type": "highpass",
                    "frequency": 20.0,
                    "q": 0.707
                }),
            }),
        ],
        ..Default::default()
    }),
    ..Default::default()
}
```

---

## 8. Open Questions

1. **Frequency quantization accuracy vs. musicality** — Real NES/GB hardware can't hit exact pitches (e.g., A4 = 440 Hz maps to period 253, which is actually 438.45 Hz). Do we emulate this pitch inaccuracy or allow exact tuning? Proposal: offer a `period_quantize: bool` flag, default off for musicality, on for authenticity.

2. **Frame sequencer timing** — The real hardware clocks envelopes/sweeps at exact fractions of the CPU clock. At 44100 Hz sample rate, these don't align perfectly. Do we snap to sample boundaries (simpler) or maintain sub-sample accuracy (more authentic)? Proposal: snap to sample boundaries — the difference is inaudible.

3. **DPCM priority** — NES DPCM is primarily used for pre-recorded drums/voices, which overlaps with the existing sampler. Should we implement it as a retro oscillator mode or just use the sampler with a "DPCM decoder" preset? Proposal: defer, use sampler.

4. **Channel count enforcement** — Real hardware has strict channel limits (NES: 5, GB: 4). Should the preset system enforce these limits when using a full-system preset? Proposal: no enforcement — let users layer freely, but document authentic constraints.

5. **Wave RAM** — On real GB hardware, wave RAM can be written while the channel plays, creating glitch effects. Support this as a feature or ignore? Proposal: ignore for now, possibly add as a "wave_table_modulation" feature later.

   > **Context:** Writing to wave RAM during playback is common for more than just glitch effects. With careful timing, it enables streaming samples of arbitrary length through the wave channel — effectively turning the 32-sample wave buffer into a ring buffer. LSDj (the popular Game Boy tracker) uses this technique extensively. **Didrik Madheden (nitro2k01)** is a good resource on Game Boy sound hardware internals and techniques like this.

---

## 9. Non-Goals (for this plan)

- **Cycle-accurate emulation** — We are building a synthesizer inspired by retro hardware, not a hardware emulator. We match the *sound character* at the generator level.
- **Memory constraints** — Real hardware had limited RAM/ROM. We don't impose artificial limits.
- **Tracker-style sequencing** — The `.sw` language already handles sequencing; we don't need to emulate tracker UIs or effect columns.
- **FM synthesis** — Yamaha YM2612 (Genesis) is a fundamentally different synthesis model and out of scope for this plan.

---

## 10. Summary

| Phase | Deliverable | Key Changes |
|-------|------------|-------------|
| **1** | Core oscillator extensions | duty cycle, LFSR noise, wavetable, DAC quantizer, filter wiring |
| **2** | Hardware envelope / frame sequencer | `EnvelopeMode`, `HwEnvelope`, `SweepUnit`, `LengthCounter` as `Voice` extensions |
| **3** | Built-in presets + syntax | NES/GB presets, `.sw` inline config, custom wave tables |
| **4** | Analog stage modeling | High-pass filters, gain models as effect presets |
| **5** | Additional systems | SMS, Spectrum, C64 (future) |

The architecture is designed so that **Phase 1 alone delivers immediate value** — users get pulse-width control, noise, and wavetable waveforms usable for any chiptune-inspired music, not just strict NES/GB emulation. Each subsequent phase adds authenticity layers that power users can opt into.
