# Refactoring Plan: SongWalker

## 1. Overview
The goal of this refactor is to transform `song-walker` into a robust, high-performance music creation platform. The core will be moved to a Rust-based architecture that handles transpilation, scheduling, and potentially DSP (Digital Signal Processing) to ensure cross-platform consistency. The existing JavaScript-based song format will be enhanced and formalized. A new web-based editor with advanced features will be developed.

## 2. Project Restructuring & Monorepo
We will transition to a clean monorepo structure. All existing code (legacy JS implementation) will be archived to avoid confusion.

### A. New Directory Structure
```
song-walker/
├── archive/               # All legacy code moved here
│   ├── app/
│   ├── songwalker/
│   ├── songwalker-editor/
│   └── ...
├── docs/                  # Documentation & Plans
├── songwalker_core/       # [New] Rust Core (Parser/Compiler/DSP)
│   ├── src/
│   └── Cargo.toml
├── songwalker_web/        # [New] Web Editor (SolidJS/Svelte + WASM)
├── songwalker_cli/        # [New] Command Line Interface (Rust wrapper)
└── README.md
```

### B. Migration Plan
1.  **Archive**: Move `app`, `cypress`, `public`, `scripts`, `songwalker`, `songwalker-editor`, `songwalker-presets` into `archive/`.
2.  **Initialize**: Create the new `songwalker_core` (Rust) and `songwalker_web` (Node/Vite) projects.
3.  **Dependencies**: Configure a Cargo workspace (if multiple Rust crates) or keep them independent.

## 3. Core Architecture (Rust)

### A. Rust-based Compiler (SW -> IR)
We will create a Rust library that parses the SongWalker DSL and compiles it into an Internal Representation (IR).

*   **Input**: `.sw` source code.
*   **Parser**: **`Chumsky` (Rust)**. We will define a grammar that allows standard JavaScript control flow (loops, variables) mixed with our custom keywords.
*   **Compiler Logic**:
    *   We traverse the AST.
    *   We identify `track` definitions (which behave like async generators).
    *   Inside tracks, we interpret standard JS expressions (`C3 / 4`) as event triggers.
    *   We allow standard loops (`for`, `while`) which the compiler unrolls into the event timeline.
*   **Output**: A detailed Event List (Rust Struct) ready for the Scheduler.

### B. Core Library & Embeddability
The Rust core will be designed as a standalone library used by:
*   **Web Editor**: Via WASM.
*   **CLI**: For offline rendering and testing.
*   **Tests**: For validating logic without a browser.

### C. Offline Rendering & CLI (Full Rust DSP)
To solve the `OfflineAudioContext` memory limit (GitHub Issue #2445) and ensure identical output:
*   **Block-based Rendering**: Instead of rendering the whole song at once, the system will render in chunks.
*   **All DSP in Rust**: To guarantee bit-exact output across all platforms (Web, CLI, various operating systems), we will implement *all* Signal Processing (Oscillators, Filters, Effects, Mixing) in Rust. We will **not** rely on native WebAudio nodes (like `OscillatorNode`, `BiquadFilterNode`) for the core sound generation.
*   **CLI Renderer**: A Rust binary that runs the compilation and then executes the Rust audio graph directly. This allows offline rendering without needing a headless browser.

### D. Trade-offs of Full Rust DSP Strategy
*   **Benefit - Exact Determinism**: Audio will sound exactly the same in Chrome, Firefox, Safari, and the CLI. We eliminate variations in browser DSP implementations.
*   **Benefit - Portability**: The engine allows for true offline rendering and can eventually be ported to native environments (e.g., VST plugins, native apps) without dependency on a browser engine.
*   **Cost - Implementation Effort**: We lose the convenience of `ctx.createOscillator()`. We must verify and implement our own DSP algorithms (or use crates like `fundsp` or `dasp`).
*   **Cost - Ecosystem Isolation**: It becomes difficult to mix-and-match with other JS WebAudio libraries that rely on connecting standard native nodes. We are effectively building a "Engine in a Box" that outputs a single stream to WebAudio.

## 4. Language Refinement (Hybrid DSL)

We will use a hybrid Javascript-like DSL. This gives us the familiarity of JavaScript control flow (loops, variables) while retaining distinct keywords for musical concepts.

### A. Core Syntax
*   **Custom Parser**: We will implement the exact grammar preferred by the user using `Chumsky`. It is a superset of JavaScript.
*   **Top Level**: Direct track invocation. This defines the arrangement.
    ```javascript
    // Call track 'riff' with instrument 'lead'
    riff(lead); 
    
    // Call 'drums' with modifiers: 
    // Velocity 96 (*96)
    // Play only first 4 beats (@4)
    // Wait for 8 beats before next line (8)
    drums*96@4() 8 
    ```
*   **Track Definitions**:
    ```javascript
    track riff(inst) {
        // Javascript logic allowed here (variables, loops)
        // Minimalist Note Syntax calls
        C3 /2
        Eb3@/4 /2  // Note C3, Audible(/4), Step(/2)
    }
    ```
*   **Execution Model**:
    *   **Tracks**: Distinct from normal functions. When a `track` is called, it is implicitly **scheduled** by the player to play at the current virtual time (calculated from the parent track) rather than executing immediately. The `schedule()` API remains the underlying mechanism.
    *   **Normal Functions**: Standard JavaScript functions that execute code immediately when called. These can be used for complex logic, helpers, or algorithmic composition.

### B. Modifiers & Symbols
We will formalize the symbols found in `test.sw` to ensure they are parsed consistently:
*   `*` (Asterisk): Velocity / Dynamics (e.g., `*90` or `*0.8`).
*   `.` (Dot): Shorthand Duration (e.g., `.` for 1x default step, `..` for 2x).
*   `@` (At): Audible Duration / Slice Length (e.g., `@1/4`).
*   `/` (Slash): Duration separator (e.g., `C3 / 1/2`).
*   Trailing Number: Step Duration (Wait time).
*   **Standalone Numbers**: Rests (e.g., `4` means wait 4 beats).
*   Standard JS expressions allowed in arguments `(inst)`.

## 5. WebAudio & Custom Components

### A. Scheduling & Buffering
To prevent jumps/delays:
*   **Scheduler Pattern**: A JavaScript "Pump" loop (driven by `requestAnimationFrame` or `setTimeout`, or strictly by audio time in a Worklet) calls the transpiled `track` iterators to fetch the next batch of events.
*   **Lookahead**: Always schedule events `0.1s` - `0.5s` into the future.

### B. Custom Audio Nodes (AudioWorklet + WASM)
*   **Complete Replacement**: We will bypass the standard WebAudio graph for sound generation. The WebAudio API will primarily serve as a verified output sink (`AudioContext.destination`) and a scheduler housing a single master `AudioWorklet`.
*   **DSP Library**: We will build our custom ecosystem of nodes in Rust. We can leverage existing Rust audio crates (like `fundsp` or `dasp`) where appropriate to speed up development of standard components (Envelopes, Oscillators, Filters).

### C. Standard Effects & Instruments
We will progressively implement a suite of core DSP modules in Rust. Where possible, we will match the property names and behavior of the standard WebAudio nodes to ease transition and familiarity.

*   **Effects**:
    *   **Echo / Delay**: Standard delay lines with feedback.
    *   **Reverb**: Algorithmic reverb (e.g., Freeverb or similar) and Convolution reverb (loading impulse responses).
    *   **Chorus / Flanger / Phaser**: Modulation effects based on delay lines and LFOs.
    *   **EQ / Filter**: Biquad filters (Lowpass, Highpass, Peaking, etc.) matching `BiquadFilterNode` coefficients.
    *   **Compression**: Dynamics processing matching various `DynamicsCompressorNode` parameters (threshold, knee, ratio, attack, release).
    *   *Strategy*: Start with essential filters and delay, adding complex effects (Reverb, Compressor) incrementally.

*   **Instruments**:
    *   **Oscillator**: Basic waveforms (Sine, Square, Sawtooth, Triangle) with anti-aliasing (e.g., PolyBLEP). Matching `OscillatorNode` frequency/detune params.
    *   **Sampler (AudioBuffer Player)**: Playback of loaded audio samples with rate control, looping, and pitch shifting (resampling).
    *   **Modular Design**: Complex instruments (e.g., a subtractive synth) will be composed of these basic building blocks (Oscillators + Envelopes + Filters) rather than being monolithic codebases.
    *   **Preset Format Refactor**: The existing preset format (currently relying on loading WebAudio nodes or fonts) will need to be refactored to describe these Rust-based DSP graphs. We will define a JSON/Structure schema that declarative links these new Rust modules.

## 6. Web-Based Editor

### A. Framework Choice
*   **Recommendation**: **SolidJS** or **Svelte** over React.
    *   **Why?** Signals-based fine-grained reactivity is significantly more performant for high-frequency updates (like a playhead moving 60fps or visualizing audio peaks) than React's Virtual DOM diffing. React can work, but SolidJS offers "snappy" performance by default for this use case.
    *   **Canvas/WebGL**: For the actual track visualization and peak meters, we should use an HTML5 `<canvas>` or WebGL (e.g., `PixiJS` or plain WebGL) layer, as DOM elements are too heavy for real-time audio visualization.

### B. Features
*   **Real-time Highlighting**: Uses the Source Maps generated by the Rust transpiler to map `audioContext.currentTime` -> `AST Node` -> `Line Number`.
*   **Monaco Editor (VS Code Editor)**: Embed `monaco-editor` for the code view.
    *   It supports "Peak View" (Code Lens).
    *   It has the command palette (Cmd+Shift+P) and keyboard shortcuts built-in.
    *   We can register custom languages (SongWalker `.sw`) for syntax highlighting.
*   **Plugins**: A plugin system where components listen to the audio stream (via `AnalyserNode`) and render to a canvas.

### C. Keyboard Shortcuts (Intellij-style)
Monaco Editor supports custom keybindings easily. We will map:
*   `Cmd+Option+L`: Trigger Prettier/Formatter (via Rust formatter).
*   `Cmd+/`: Toggle comment (Built-in to Monaco).
*   `Shift+F6`: Rename symbol (Requires generic LSP or simple regex-based rename for now).

## 7. Testing Strategy

*   **Unit Tests (Rust)**: Test the parser and transpiler logic. Input `.sw` -> Assert expected JS output structure.
*   **Integration Tests (Simulated Audio)**: Transpile a test song, run it in a headless environment with a Mock AudioContext, and assert that the correct methods (`connect`, `start`) were called at the correct `time`.
*   **Offline Rendering Verification**:
    *   **Song Length**: Verify that the rendered song length corresponds exactly to the end of the last scheduled event/rest (the "cursor" position). The song should *not* extend to wait for audio tails (reverb, release) unless explicitly programmed with a rest/wait.
*   **Snapshot Tests**: Render audio to a WAV file (using the CLI renderer) and compare its fingerprint/hash against a known good snapshot.

## 8. Implementation Roadmap

1.  **Phase 0: Project Restructuring**: Archive legacy code and initialize new Monorepo structure.
2.  **Phase 1: Rust Core & Transpiler**: Setup Rust project, define grammar, build basic transpiler (`.sw` -> JS).
3.  **Phase 2: Audio Engine**: Implement the JS Scheduler and basic WebAudio integration.
4.  **Phase 3: Editor Prototype**: Setup SolidJS/Svelte app with Monaco Editor and WASM integration.
5.  **Phase 4: DSP & Offline**: Implement custom WASM nodes and the CLI renderer.
6.  **Phase 5: Website & Hosting**: Build the public-facing website.

## 9. Phase 5: Website & Hosting

### A. Overview
Create a public website at [songwalker.net](https://songwalker.net) that serves as both the project homepage and the primary way to use the editor. The editor should be the hero element on the front page — users should be able to start writing music immediately without signing up.

### B. Editor Integration
*   **Front-page Editor**: The Monaco-based editor is embedded directly on the landing page, pre-loaded with an example song.
*   **Full-screen Mode**: A toggle (or `F11` / `Escape`) to expand the editor to fill the entire viewport, hiding all marketing/description content.
*   **Responsive**: The editor should work on tablets and desktops. Mobile can show a read-only/playback view.

### C. Persistence
*   **LocalStorage**: All editor state (source code, open tabs, playback position, settings) is automatically persisted to `localStorage`. Returning users pick up exactly where they left off.
*   **File System Access API**: When the browser supports the [File System Access API](https://developer.mozilla.org/en-US/docs/Web/API/File_System_Access_API), offer native Open/Save dialogs (`showOpenFilePicker`, `showSaveFilePicker`) for `.sw` files. This allows users to work with real files on disk.
*   **Fallback**: On browsers without File System Access API, fall back to `<input type="file">` for opening and `Blob` + `<a download>` for saving.

### D. Site Content
*   **Hero Section**: Editor with a "Play" button for the example song.
*   **About Section**: Brief description of the project and language.
*   **Documentation**: Language reference, tutorial, and examples (can link to a `/docs` subpath or separate page).
*   **GitHub Link**: Link to the repository.
