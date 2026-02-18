# Performance-First Custom Keyboard Prototype Plan

## Goal
Build a **performance-grade** music keyboard instrument running the SongWalker
engine in **Rust** on embedded hardware, prioritizing:
- **Fast boot** (sound-ready quickly)
- **Stability** (no "OS crashes" during performance)
- **Deterministic audio** (no glitching under UI load)

The instrument reuses as much of `songwalker-core` (Rust) as possible, with a
clear porting path from the existing desktop/WASM engine to `no_std` firmware.

---

## Platform: Rust on Daisy-Class Hardware (Recommended)

### Why Daisy + Rust
Embedded MCU-based audio avoids general-purpose OS instability and long boot times.
The instrument behaves like an appliance. Rust is the natural choice because
**songwalker-core is already written in Rust** — the DSP algorithms, voice system,
event scheduling, and preset format are all Rust code that can be refactored behind
feature flags for `no_std` compilation.

### Platform details
#### Electro-Smith Daisy (Primary Target)
- **MCU:** STM32H750 — ARM Cortex-M7 @ 480 MHz, hardware FPU (`f32` single + `f64` double)
- **RAM:** 64 KB SRAM + 64 MB SDRAM (Daisy Seed module)
- **Audio codec:** AK4556 — stereo 24-bit, up to 96 kHz
- **Flash:** 8 MB QSPI (samples, presets)
- **Bootloader:** ~2.5 s grace period before jumping to app
  Source: https://electro-smith.github.io/libDaisy/md_doc_2md_2__a7___getting-_started-_daisy-_bootloader.html
- **Optimization:** Run firmware directly from internal flash without the bootloader
  to reduce boot latency (sub-second cold start feasible)
- **Rust support:** `daisy` crate exists (https://crates.io/crates/daisy), plus
  `stm32h7xx-hal`, `cortex-m`, `cortex-m-rt` — mature embedded Rust ecosystem

**Why Cortex-M7 over M4:**
The Daisy Seed's STM32H750 has a Cortex-M7 core with double-precision FPU, meaning
`songwalker-core`'s `f64` DSP math works *without* rewriting to `f32` for the
initial port. This dramatically reduces porting effort — the M7 can run `f64` at
hardware speed, while an M4F (like the cheaper Daisy Patch SM) can only do `f32`
in hardware and would emulate `f64` in software (~10x slower). **Start on M7,
optimize to `f32` later only if CPU headroom demands it.**

#### Bela (Deprioritized — Linux boot penalty)
- Linux-based; typical boot time **15–30 s** (too slow for stage instrument)
- Excellent sub-1ms audio latency *once booted*
- Better suited as a dev/prototyping platform, not the production target
  Source: https://learn.bela.io/using-bela/about-bela/troubleshooting-guide/

---

## Existing Codebase: What We Have Today

### songwalker-core (v0.1.1) — The Engine
The Rust core already implements the full synthesis pipeline:

| Module | What It Does | Embedded-Ready? |
|--------|-------------|-----------------|
| `dsp/oscillator.rs` | PolyBLEP sine, saw, square, triangle (all `f64`) | Algorithms yes, data structures need work |
| `dsp/sampler.rs` | Multi-zone sample playback, linear interpolation, looping | Heap-heavy (`Vec<f64>` cloned per voice) |
| `dsp/envelope.rs` | Linear ADSR envelope | Portable as-is |
| `dsp/voice.rs` | Single-note instance (oscillator + envelope + velocity) | Portable as-is |
| `dsp/composite.rs` | Layer / Split / Chain multi-source instruments | Recursive `Vec` — needs fixed alloc |
| `dsp/filter.rs` | Biquad IIR (LP, HP, BP, Notch, Peaking) | Portable as-is |
| `dsp/mixer.rs` | Summing mixer with `tanh` soft clipper | Portable as-is |
| `dsp/delay.rs` | Stereo delay line with feedback (`f32` buffers) | Needs static buffer allocation |
| `dsp/reverb.rs` | Schroeder/Freeverb (8 comb + 4 allpass, `f32`) | Needs static buffer allocation |
| `dsp/chorus.rs` | LFO-modulated stereo delay (`f32`) | Needs static buffer allocation |
| `dsp/compressor.rs` | Feed-forward dynamics with soft knee | Portable as-is |
| `dsp/engine.rs` | Event scheduling, voice management (max 64), effects chain | Heap-heavy, needs static voice pool |
| `compiler.rs` | AST → flat `EventList` | `std`-only, not needed on firmware |
| `lexer.rs` / `parser.rs` | Source → tokens → AST | `std`-only, not needed on firmware |

**Master effects chain order:** Chorus → Delay → Reverb → Compressor

**Voice system:** Max 64 voices, event-driven scheduling, 128-sample block processing.
No voice stealing currently (new notes silently dropped at max). Gate-based release
with per-voice `release_sample` offset.

### songwalker-vsti (v0.2.0) — Desktop Reference Implementation
The VSTi plugin already demonstrates the full audio pipeline in a real-time context:
- `nih-plug` framework (audio callback, parameter system, MIDI handling)
- `crossbeam-channel` for editor → audio thread communication
- Slot-based multi-timbral architecture (multiple instruments simultaneously)
- Preset loading from remote library with local caching
- Piano keyboard plan exists (`songwalker-vsti/docs/piano_keyboard_plan.md`)
  with crossbeam channel design for note events — this pattern maps directly
  to the firmware's interrupt-driven MIDI/keybed scanning

### songwalker-site / songwalker-js — Web Reference
- WASM build of songwalker-core used in browser
- Cursor-aware instrument detection planned (`get_instrument_at_cursor()`)
- Single-note rendering API planned (`render_single_note()`)
- The cursor-aware/streaming execution model (`SongRunner` + `EventBuffer`)
  designed in `songwalker-core/docs/cursor_aware_plan.md` is directly relevant
  to firmware: it uses bounded ring buffers and incremental execution

---

## Porting Strategy: songwalker-core → Firmware

### Key Gaps in Current songwalker-core

| Barrier | Description | Effort |
|---------|-------------|--------|
| **No `no_std` support** | Uses `std` throughout, `HashMap`, `Vec`, `String` | High — needs feature-gated split |
| **All `f64` in DSP** | Oscillator, envelope, sampler use `f64` | Low on M7 (has `f64` FPU); deferred |
| **Heap-heavy** | `Vec` for buffers, voices, events; `HashMap` for preset registry | High — needs `heapless` or static pools |
| **Offline-only renderer** | `render()` produces entire song buffer at once | Medium — `SongRunner`+`EventBuffer` model solves this |
| **No fixed voice pool** | Dynamic `Vec<ActiveVoice>` with `retain()` GC | Medium — replace with static `[Option<ActiveVoice>; MAX_VOICES]` |
| **Sampler clones buffers** | `SampleBuffer` (mono `Vec<f64>`) cloned per voice | High — needs shared `&'static` sample refs |
| **String-heavy preset keys** | `HashMap<String, _>` for instrument lookup | Medium — use index-based or `heapless::String` |

### Proposed Crate Architecture

```
songwalker-core/          (existing — add feature flags)
├── src/
│   ├── dsp/              ← extract as `songwalker-dsp` or gate behind features
│   │   ├── oscillator.rs   no_std-ready (algorithms unchanged)
│   │   ├── envelope.rs     no_std-ready
│   │   ├── filter.rs       no_std-ready
│   │   ├── sampler.rs      needs static SampleBuffer refs
│   │   ├── voice.rs        no_std-ready
│   │   ├── mixer.rs        needs fixed-size accumulator
│   │   ├── engine.rs       needs static voice pool + bounded EventBuffer
│   │   ├── delay.rs        needs static delay line allocation
│   │   ├── reverb.rs       needs static comb/allpass buffers
│   │   ├── chorus.rs       needs static delay buffer
│   │   └── compressor.rs   no_std-ready
│   ├── preset.rs         ← zone descriptors, pitch math (already no-alloc-friendly)
│   ├── lexer.rs          ← std-only (not needed on firmware)
│   ├── parser.rs         ← std-only
│   └── compiler.rs       ← std-only

songwalker-firmware/      (NEW — the embedded target)
├── Cargo.toml            depends on songwalker-core with `no_std` feature
├── src/
│   ├── main.rs           #![no_std] #![no_main] entry + audio callback
│   ├── audio.rs          DMA-driven audio output, block rendering
│   ├── midi.rs           USB-MIDI class device + DIN MIDI UART
│   ├── keybed.rs         matrix scanning, velocity detection
│   ├── controls.rs       encoders, buttons, faders
│   ├── display.rs        SPI/I2C display driver (OLED or TFT)
│   ├── presets.rs        QSPI flash preset storage + loading
│   └── ui_link.rs        SPI/UART protocol to UI brain (if split-brain)

songwalker-vsti/          (existing — unchanged, uses songwalker-core with std)
songwalker-cli/           (existing — unchanged, uses songwalker-core with std)
```

### Feature Flag Design for songwalker-core

```toml
[features]
default = ["std", "wasm"]
std = []                    # enables lexer, parser, compiler, HashMap, etc.
wasm = ["std", "wasm-bindgen", "serde-wasm-bindgen"]
embedded = []               # no_std DSP-only build, heapless collections
```

**Phase 1 (minimal):** Gate `lexer.rs`, `parser.rs`, `compiler.rs`, and
`wasm-bindgen` behind `#[cfg(feature = "std")]`. The DSP modules compile with
just `core` + `alloc`. This gets a `no_std` build compiling without rewriting
any DSP algorithms.

**Phase 2 (optimize):** Replace `Vec` with `heapless::Vec` or static arrays in
DSP hot paths. Replace `HashMap` preset registry with indexed array. Add a
`StaticVoicePool<const N: usize>` to replace dynamic `Vec<ActiveVoice>`.

**Phase 3 (optional f32):** Add `f32` variants of oscillator/envelope/sampler
behind a `float32` feature flag for M4F-class targets. M7 targets keep `f64`.

### The SongRunner Advantage for Embedded

The planned `SongRunner` + `EventBuffer` streaming model
(see `songwalker-core/docs/cursor_aware_plan.md`) is **ideal for embedded**:

- **Bounded memory:** Only ~4 beats of events buffered, not the entire song
- **Incremental execution:** AST interpreter produces events on demand
- **No unbounded allocations:** Ring buffer has fixed capacity
- **Block-based consumption:** AudioEngine already renders in 128-sample blocks —
  perfect for DMA double-buffering on Daisy

The firmware audio callback would be:
```rust
// In DMA half-transfer / transfer-complete interrupt:
fn audio_callback(buffer: &mut [f32; BLOCK_SIZE * 2]) {
    // 1. Drain piano/MIDI events into SongRunner (if running a .sw file)
    //    or directly into engine voices (if in instrument mode)
    // 2. SongRunner.step() — fill EventBuffer up to current beat + 4
    // 3. AudioEngine.render_block(128) — consume events, mix voices
    // 4. Write interleaved stereo to DMA buffer
}
```

This is essentially the same architecture as the VSTi's `process()` function
in `songwalker-vsti/src/audio.rs`, but driven by DMA interrupts instead of
the DAW's audio callback.

---

## Recommended Architecture: Split-Brain

### Why Split-Brain
The Daisy Seed's STM32H750 has plenty of compute for DSP, but driving a
color TFT display with animations while running the effects chain would
compete for CPU cycles and bus bandwidth. Separating audio and UI guarantees
that **if the UI crashes or reboots, audio continues uninterrupted**.

### Audio Brain (Hard Real-Time) — Daisy Seed
- Runs: DSP engine, voice allocation, effects chain, MIDI handling, keybed scanning
- Audio I/O: AK4556 codec via SAI + DMA (128-sample blocks @ 48 kHz = 2.67 ms latency)
- MIDI: USB-MIDI device class + DIN MIDI via UART
- Preset storage: 8 MB QSPI flash (decode samples at note-on into SDRAM)
- Communication: SPI slave to UI brain, compact binary protocol

### UI Brain (Non-Real-Time) — ESP32-S3 or RP2040
- Runs: display rendering, preset browser, patch editor, meters, .sw file browser
- Display: SPI TFT (320x240 or 480x320) with LVGL or slint-ui
- Storage: microSD for .sw files and preset library index
- Communication: SPI master to audio brain
- **If UI hangs or resets, audio brain is unaffected**

### Interconnect Protocol (SPI Binary)
```
[CMD:u8] [SLOT:u8] [PAYLOAD:variable]

Commands:
  0x01 NOTE_ON    slot, note, velocity
  0x02 NOTE_OFF   slot, note
  0x10 LOAD_PRESET  slot, preset_index (audio brain loads from QSPI)
  0x20 SET_PARAM   slot, param_id, value_f32
  0x30 GET_METERS  -> returns peak_l, peak_r, voice_count, cpu_percent
  0x40 SET_BPM     bpm_f32
  0x50 PLAY_SONG   song_index (audio brain runs SongRunner)
  0x51 STOP
```

### Single-Brain Alternative (Simpler v0)
For Prototype v0, a single Daisy Seed with a small OLED (128x64 SSD1306 via I2C)
is sufficient. The OLED draws < 1% CPU and doesn't interfere with audio. This
avoids inter-processor protocol complexity for the initial proof of concept.
Upgrade to split-brain for v1 when a color TFT and richer UI are needed.

---

## Reliability & Stage-Safety Requirements (Non-Negotiables)

### Hard real-time rules (audio callback)
- No dynamic allocations (`#[deny(clippy::disallowed_methods)]` for `Vec::push`, `Box::new`, etc.)
- No locks/mutexes — use lock-free ring buffers (`heapless::spsc::Queue`)
- No file I/O in audio interrupt
- No logging or printing in audio interrupt
- Deterministic bounded-time operations only
- **Enforced by Rust's type system:** audio callback takes `&mut AudioState`
  containing only stack-allocated or `'static` data

### System safety
- Watchdog timer enabled (IWDG, ~500 ms timeout)
- Brownout/power-fail detection (BOR level 3 on STM32H7)
- "Safe mode" boot path: hold designated button at power-on -> load golden firmware
- All panics redirect to a blinking LED error code (no unwinding in `no_std`)

### Firmware update strategy
- **Dual-bank flash:** golden firmware in bank A, updatable firmware in bank B
- Updates only when explicitly invoked (hold button + connect USB)
- Daisy bootloader's grace period + media search for DFU mode
  Source: https://electro-smith.github.io/libDaisy/md_doc_2md_2__a7___getting-_started-_daisy-_bootloader.html
- UF2 bootloader option for drag-and-drop firmware updates

### Data safety (presets)
- Atomic preset writes (two-slot or journaled approach in QSPI flash)
- Wear-leveling for flash sectors
- Never corrupt last-known-good preset
- CRC32 on all stored preset data

---

## Prototype Roadmap

### Prototype v0 — Prove Rust DSP on Daisy (4-6 weeks)
**Objective:** Validate that songwalker-core's DSP runs on Daisy hardware with
acceptable polyphony, latency, and stability.

**Hardware:**
- Daisy Seed dev board (~$30) + breadboard
- USB-MIDI input (use any MIDI controller keyboard for note input)
- Built-in audio codec (line out + headphone out)
- Optional: SSD1306 OLED (128x64) for status display

**Software milestones:**
1. **Week 1:** Get a Rust `#![no_std]` binary running on Daisy Seed
   - `cortex-m-rt` entry point, GPIO LED blink
   - SAI + DMA audio output (sine wave test tone)
2. **Week 2:** Port `oscillator.rs` and `envelope.rs` to firmware
   - Single PolyBLEP voice playing from hardcoded note events
   - Measure CPU usage per voice (target: < 2% per oscillator voice @ 48 kHz)
3. **Week 3:** Port `engine.rs` voice system with static voice pool
   - USB-MIDI input -> polyphonic playback (target: 16+ voices)
   - Add `mixer.rs` with static accumulator buffer
4. **Week 4:** Port `sampler.rs` with SDRAM sample storage
   - Load one preset's samples from QSPI flash into SDRAM at boot
   - Play sample-based notes via MIDI
5. **Week 5-6:** Port effects chain (chorus, delay, reverb, compressor)
   - Static buffer allocation for delay lines
   - Measure total CPU headroom with full effects chain
   - Run 48-hour soak test

**Key metrics to validate:**
- Audio latency: < 3 ms (128 samples @ 48 kHz)
- Polyphony: >= 16 oscillator voices, >= 8 sampler voices with effects
- CPU headroom: >= 30% idle with full voice load + effects
- Boot to sound-ready: < 1 s (direct flash boot, no bootloader)
- Zero audio glitches over 48-hour soak test

### Prototype v1 — Keyboard Form Factor EVT (8-12 weeks)
**Objective:** Move from dev board to a real keyboard prototype.

**Hardware additions:**
- OEM keybed (25/37 key, Fatar TP/9S or similar) + scanning matrix PCB
- Split-brain: add ESP32-S3 or RP2040 + SPI TFT for UI
- Custom carrier PCB (Daisy Seed module socket + connectors)
- DIN MIDI in/out jacks + USB-C
- 3D-printed enclosure

**Software additions:**
- Keybed matrix scanning with velocity detection
- SPI inter-processor protocol
- Preset browser UI on TFT (LVGL or slint-ui)
- Multiple preset slots (load from QSPI flash library)
- `.sw` file playback via SongRunner (streaming execution model)

### Prototype v2 — DVT-Quality for Crowdfunding (12-16 weeks)
**Objective:** Small run "Founder Prototype" units for early contributors.
- Refined enclosure (CNC aluminum or injection-molded)
- Repeatable assembly with pick-and-place PCBs
- Test jig and automated soak-testing process
- Firmware update via USB drag-and-drop (UF2)
- Documentation and quick-start guide

---

## Relationship to Existing Software Targets

The firmware keyboard is a **new deployment target** alongside the existing ones.
The same songwalker-core crate serves all targets through feature flags:

| Target | Crate | Core Features | Audio Path |
|--------|-------|---------------|------------|
| **Firmware (Daisy)** | `songwalker-firmware` | `embedded` | DMA -> codec |
| **VSTi plugin** | `songwalker-vsti` v0.2.0 | `default` (std) | nih-plug audio callback |
| **CLI renderer** | `songwalker-cli` | `default` (std) | Offline WAV file |
| **Web editor** | `songwalker-site` via `songwalker-js` | `wasm` | WASM -> AudioContext |

The VSTi's crossbeam-channel piano keyboard design (editor -> audio thread note
events) is architecturally identical to the firmware's interrupt-driven keybed ->
audio callback flow. Both use a bounded channel/queue to decouple input from
the real-time audio path.

---

## Cost-Efficient Build Services (for EVT/DVT)

### PCBs + assembly
- JLCPCB / PCBWay: low-volume prototyping + assembly workflows
- Use for control boards, carrier boards, LED/encoder boards
- Daisy Seed module socketed (not soldered) — replaceable compute module

### Enclosures / mechanical
- Protolabs / Fictiv-style services for CNC aluminum housings
- Local 3D print for rapid iterations on v0/v1
- Consider PCB-as-panel for front panels (JLCPCB aluminum substrate)

### Keybed sourcing
- Fatar keybeds (TP/9S for synth action, TP/8S for semi-weighted) — industry standard OEM
- Doepfer DIY keyboard kits for early prototypes
- Salvaged keybeds from used MIDI controllers for v0 testing

---

## Crowdfunding Strategy (Prototype to Contributors)

### Viable platforms
- **Crowd Supply:** Best fit for open-source audio hardware; technical audience,
  manufacturing partnership options, no "all-or-nothing" pressure
- **Kickstarter:** Broader reach; requires honest representation + working prototype
  (use a limited "Founder Prototype Run" tier)

### Suggested offering structure
- "Founder / Contributor Prototype Run"
  - Limited quantity (20-50 units)
  - Numbered units with open hardware/firmware
  - Explicit EVT/DVT nature, support boundaries, replacement policy
  - Pricing includes prototype overhead and buffers (shipping/returns)
  - Contributors get firmware update access and input on feature priorities

---

## Next Steps (Ordered by Priority)

### Immediate (before any hardware)
1. **Add `no_std` feature gate to songwalker-core** — gate `lexer`, `parser`,
   `compiler`, `wasm-bindgen` behind `#[cfg(feature = "std")]`. Verify the DSP
   modules compile with just `core` + `alloc`.
2. **Create `songwalker-firmware` crate** — `#![no_std]` skeleton with
   `cortex-m-rt`, `stm32h7xx-hal`, `daisy` dependencies. Get LED blink running.
3. **Order a Daisy Seed** (~$30) and breadboard kit.

### Short-term (Prototype v0)
4. **Port DSP modules** — oscillator -> envelope -> voice -> mixer -> engine,
   in that order. Replace `Vec` with `heapless::Vec` or fixed arrays.
5. **Implement static voice pool** — `[Option<ActiveVoice>; 64]` replacing
   dynamic `Vec<ActiveVoice>`.
6. **USB-MIDI input** — `usb-device` + `usbd-midi` crates.
7. **Benchmark and profile** — measure per-voice CPU cost, total headroom,
   worst-case interrupt latency.

### Medium-term (Prototype v1)
8. **Port sampler with SDRAM** — `SampleBuffer` as `&'static [f64]` in SDRAM,
   zone lookup by MIDI note.
9. **Port effects chain** — static delay/reverb/chorus buffers in SDRAM.
10. **Implement SongRunner for firmware** — streaming `.sw` execution with
    bounded `EventBuffer` (ring buffer in SRAM).
11. **Design carrier PCB + keybed integration.**

---

## Open Decisions (Needed to Finalize v0 Spec)
- Key count (25/37/49/61?) and whether velocity/aftertouch are required
- UI style: knobs-first vs touchscreen-first (OLED for v0, TFT for v1)
- Prototype batch size goal: 20 vs 50 vs 100
- Polyphony target: 16 vs 32 vs 64 voices on Daisy M7
- Audio I/O: line out only vs. line out + headphone + mic in
- MIDI: USB-MIDI only vs. USB-MIDI + DIN MIDI (5-pin)
- Sample memory budget: how many presets simultaneously in SDRAM?
- `.sw` playback on firmware: required for v0, or deferred to v1?
