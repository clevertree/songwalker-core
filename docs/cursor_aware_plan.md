# Cursor-Aware Compilation & Single-Note Rendering

## Motivation

When editing `.sw` files, users need to **hear what a note sounds like** at their
current cursor position without playing the entire song. If the cursor sits inside
or after a `track.instrument = loadPreset("FluidR3_GM/Acoustic Grand Piano")` line,
pressing a piano key should play that piano preset. If the cursor is at the very
beginning before any instrument is loaded, the default triangle wave plays — because
triangle *is* the default instrument. There is no special-case logic; the system
simply executes whatever is loaded at that point and the triangle is what's loaded
initially. We make it clear to the user that triangle is the default/unloaded sound.

This is critical for `.sw` editing because it lets users **try the next note out**
without having to play back the entire track or song. It's the same workflow DAW
users expect: click inside a track, press a key, hear that track's instrument.

Beyond single-note preview, **cursor-aware playback** lets the user start playback
from the cursor position. The engine skips all notes before the cursor but still
executes the song up to that point (setting BPM, instruments, effects, etc.), then
begins audio output from the cursor beat. Combined with **track isolation**
(solo/mute), this gives DAW-grade editing workflow.

---

## Architectural Decisions

### 1. Tracks Are Async Functions

**Songs execute as function calls. Tracks are async functions.**

Two tracks called one after another execute **at the same time** (parallel on the
timeline). Only normal (non-track) function calls block the caller. This means:

```sw
melody(square)    // starts at beat 0, runs async
harmony(pulse)    // ALSO starts at beat 0, runs async (not beat 8)
bass(triangle)    // ALSO starts at beat 0, runs async
```

This is a **fundamental change** from the current compiler behavior, where tracks
are inlined sequentially and the cursor advances past each one. The new model makes
track isolation musically meaningful — soloing "melody" plays just the melody from
beat 0, not silence until beat 8.

**Current behavior (sequential, broken for isolation):**
```
melody:  [====notes====]
harmony:                  [====notes====]
bass:                                      [====notes====]
         beat 0           beat 8           beat 16        beat 24
```

**New behavior (async, correct for isolation):**
```
melody:  [====notes====]
harmony: [====notes====]
bass:    [====notes====]
         beat 0         beat 8
```

#### Implementation

In `inline_track_call()`, after compiling the track body, **restore `ctx.cursor`
to the saved value** instead of leaving it advanced. The track's events are already
emitted at the correct beat offsets (relative to the saved cursor). The cursor does
not advance past the track — the next statement starts at the same beat.

```rust
// Current: cursor is NOT restored (sequential)
// New: cursor IS restored (async/parallel)
ctx.cursor = saved_cursor;  // track ran async, doesn't block the caller
```

Non-track function calls (regular `Statement::Assignment`, `ConstDecl`, etc.)
still execute synchronously and *do* advance the cursor or modify state in order.

### 2. Streaming Execution Model

**The compiler does NOT flatten all tracks into one EventList upfront.** Instead,
the song executes incrementally through an **AST interpreter** (the `SongRunner`)
that produces events into a **bounded ring buffer** (the `EventBuffer`). The
`AudioEngine` consumes events from the buffer in real time.

```
Source → Lexer → Parser → AST
                           ↓
                      SongRunner  (incremental AST interpreter)
                           ↓
                      EventBuffer (ring buffer, ~4 beats ahead of playback)
                           ↓
                      AudioEngine (streaming consumer)
                           ↓
                      PCM samples
```

#### Why Streaming?

- **Infinite songs.** `.sw` songs can contain endless `while`/`for` loops (e.g.,
  generative ambient music). A full-compile model would hang or OOM.
- **Memory efficiency.** Only the buffer (~4 beats of events) plus the song state
  (variables, instrument config, BPM, etc.) are kept in memory. There is no
  unbounded `Vec<Event>`.
- **Low latency.** Playback can start as soon as the first buffer-fill completes
  (4 beats), rather than waiting for the entire song to compile.

#### SongRunner

The `SongRunner` is an AST interpreter that maintains all song execution state:

```rust
struct SongRunner {
    /// AST of the parsed song
    program: Program,

    /// Execution fibers — one per active track (+ main)
    fibers: Vec<Fiber>,

    /// Shared song state: BPM, tuning, consts, track defs
    song_state: SongState,

    /// Output destination
    buffer: EventBuffer,
}

struct Fiber {
    /// Which track this fiber is running (None = top-level)
    track_name: Option<String>,

    /// Current beat position within this fiber
    cursor: f64,

    /// Current instrument for this fiber
    instrument: InstrumentConfig,

    /// Default note length
    default_note_length: f64,

    /// AST execution position (statement index stack for nested scopes)
    exec_stack: Vec<ExecFrame>,

    /// Parameter bindings from track call arguments
    param_bindings: HashMap<String, InstrumentConfig>,
}

struct ExecFrame {
    /// Which statement list we're executing (track body, for-loop body, etc.)
    statements: Vec<TrackStatement>,
    /// Index of the next statement to execute
    position: usize,
    /// For loops: iteration state
    loop_state: Option<LoopState>,
}
```

#### Execution Loop

```
while playback is active:
    1. SongRunner.step() — execute statements until:
       (a) the buffer is full (lead >= 4 beats ahead of playback position), OR
       (b) all fibers have finished (song ended)
    2. AudioEngine.render_block(128 samples) — consume events from the buffer
       whose time <= current_sample, render audio
    3. Advance playback position by 128 samples
```

**Fiber scheduling:** Each call to `step()` round-robins through active fibers.
A fiber executes one statement, then yields. If a statement is a NoteEvent, it
pushes an `Event` to the buffer and advances its cursor by the note's step
duration. If a statement is a TrackCall, a new fiber is spawned at the current
cursor position. If a statement is a `for`/`while` loop, a new ExecFrame is
pushed onto that fiber's exec_stack.

#### EventBuffer

```rust
struct EventBuffer {
    /// Ring buffer of events, sorted by time
    events: VecDeque<Event>,

    /// The beat position of the playback head (events at or before this
    /// have been consumed by the AudioEngine)
    playback_beat: f64,

    /// Maximum lead: SongRunner pauses when the latest event's time
    /// exceeds playback_beat + buffer_beats
    buffer_beats: f64,  // default: 4.0
}

impl EventBuffer {
    /// Push an event (called by SongRunner)
    fn push(&mut self, event: Event) { ... }

    /// Drain events up to the given beat (called by AudioEngine)
    fn drain_up_to(&mut self, beat: f64) -> Vec<Event> { ... }

    /// Peek at upcoming events within the buffer (for pre-buffer awareness)
    fn peek_ahead(&self, from_beat: f64, window_beats: f64) -> &[Event] { ... }

    /// Is the buffer full? (lead >= buffer_beats)
    fn is_full(&self) -> bool { ... }
}
```

#### Memory Model

At any point during execution, the in-memory footprint is:

| Component | Contents | Bounded? |
|-----------|----------|----------|
| AST | Parsed song structure | Yes (proportional to source size) |
| SongState | BPM, tuning, consts, track_defs | Yes (small) |
| Fibers | One per active track + main | Yes (# of concurrent tracks) |
| EventBuffer | ~4 beats of events | Yes (bounded by `buffer_beats`) |
| AudioEngine | Active voices, DSP state | Yes (polyphony limit) |

There is **no unbounded `Vec<Event>`** for the entire song. The full `EventList`
never exists. Events are produced, consumed, and discarded.

### 3. Pre-Buffer Instrument Awareness

Instruments can **peek ahead** in the `EventBuffer` to see upcoming events before
they execute. This enables musically intelligent behavior:

- **Sample pre-loading:** When a sampler instrument sees upcoming notes in the
  buffer, it can begin decoding/loading those samples before the note fires.
- **Legato detection:** A synth can check if the next note overlaps with the
  current note and apply legato transitions.
- **Portamento:** If the next note is for the same instrument and overlaps, the
  synth can glide between pitches.
- **Anticipatory effects:** A filter or reverb can adjust parameters before a
  new phrase begins.

**API:** When the `AudioEngine` activates a voice for a note, it passes a
reference to the `EventBuffer` (or a pre-computed look-ahead slice). The voice's
instrument can query:

```rust
/// Get upcoming notes for this instrument within the look-ahead window
fn upcoming_notes(&self, buffer: &EventBuffer, window_beats: f64) -> Vec<&Event>
```

This is an **optimization and expressiveness feature**, not a correctness
requirement. The engine works without it — instruments simply don't anticipate.
It can be implemented incrementally after the core streaming model is working.

### 4. Byte Offsets, Not Char Indices

The lexer will be updated to track byte positions into the original `&str`, not
indices into a `Vec<char>`. This gives precision that maps directly to Rust's
`&source[..offset]` and to JavaScript's `TextEncoder`/`TextDecoder`.

### 5. No Conditional Preset Logic

The system executes whatever instrument is loaded at the cursor. If nothing is
loaded, that's the default triangle oscillator — which plays. If a preset is
referenced but not yet decoded, the engine's built-in fallback provides a
triangle. No error messages, no special cases. Triangle *is* the "no instrument
loaded" sound.

---

## Current Architecture

### Current Compile Pipeline

```
Source → Lexer → Parser → AST (Program) → Compiler → EventList → AudioEngine → Samples
```

The compiler runs to completion, producing a flat `Vec<Event>` (the `EventList`).
The `AudioEngine` then takes this complete list and renders all audio in one pass.
This is **incompatible with infinite songs** and wastes memory for long songs.

### Key Structures (Current)

```rust
struct CompileCtx {
    default_note_length: f64,
    end_mode: EndMode,
    current_instrument: InstrumentConfig,   // ← what we need to query at cursor
    cursor: f64,                            // beat position
    events: Vec<Event>,                     // flat, no track grouping
    track_defs: Vec<TrackDef>,
    consts: HashMap<String, InstrumentConfig>,
    param_bindings: HashMap<String, InstrumentConfig>,
    // MISSING: current_track_name, span tracking for assignments
}

struct Event {
    time: f64,
    kind: EventKind,
    // MISSING: track_name
}

enum EventKind {
    Note { pitch, velocity, gate, instrument: InstrumentConfig, source_start, source_end },
    TrackStart { track_name, ... },   // only for unresolved tracks
    SetProperty { target, value },
    PresetRef { name },
}
```

### Source Spans — Current State

The lexer converts source text into `Vec<char>` and tracks position as a **char
index** (not byte offset). The `Span { start, end }` on each `Spanned` token is
an index into this `Vec<char>`. For ASCII-only `.sw` files, char index == byte
offset. For files with non-ASCII characters (e.g., UTF-8 note names, comments),
they diverge.

**Which AST nodes have spans:**
- `TrackStatement::NoteEvent` — has `span_start`, `span_end` (char indices)
- `TrackStatement::Chord` — has `span_start`, `span_end`

**Which AST nodes LACK spans (need to be added):**
- `TrackStatement::Assignment` — **no span info at all**
- `TrackStatement::Rest` — no span
- `TrackStatement::ForLoop` — no span
- `TrackStatement::TrackCall` (in-track) — no span
- All `Statement` variants (top-level) — no spans
- `Expr`, `DurationExpr`, `ConstDecl` — no spans

To be precise about cursor position, we need at minimum:
- Spans on `TrackStatement::Assignment` (for `track.instrument = ...`)
- Spans on `Statement::TrackCall` (to know which track the cursor is in)
- Spans on `Statement::TrackDef` (to map cursor → track body)

### What's Missing

| Need | Current State | Gap |
|------|---------------|-----|
| Streaming execution | Compiler runs to completion, produces full EventList | Need SongRunner + EventBuffer |
| Instrument at cursor | `InstrumentConfig` on every `Note` with spans, but `Assignment` lacks spans | Add spans to Assignment, or infer from nearby notes |
| Track identifier on events | Events have no `track_name` field | Add to `Event` struct |
| Render a single note | No API | Need `render_single_note()` |
| Cursor-aware playback | Engine renders all events from beat 0 | Need "seek to cursor beat" mode in SongRunner |
| Track isolation at render time | Engine renders all events unconditionally | Need filter on EventBuffer consumer |
| Parallel track execution | Tracks compile sequentially (cursor advances) | Change to async fibers in SongRunner |
| Pre-buffer instrument awareness | No lookahead API | Need `peek_ahead()` on EventBuffer |

---

## Plan

### Phase 1: Async Track Execution (Parallel Tracks)

**Goal:** Track calls execute asynchronously — they don't advance the caller's
cursor. Two consecutive track calls start at the same beat.

This is implemented first in the **existing compiler** (flat EventList model)
because it's a simple cursor-restore change that immediately fixes track timing.
When the SongRunner replaces the compiler later, async execution is modeled as
fiber spawning instead of cursor restore.

**Change in `inline_track_call()`:**
```rust
// BEFORE (sequential — cursor left advanced):
// ctx.cursor is at wherever the body ended

// AFTER (async — cursor restored):
ctx.cursor = saved_cursor;
```

- The track body still compiles relative to `saved_cursor` and emits events at
  the correct beat offsets. The only change is that the parent cursor resets.
- Non-track statements (assignments, const decl) remain synchronous.
- `step` on a track call still works: `melody() 4;` advances cursor by 4 beats
  *after* the async call, creating a stagger.

**Impact on existing songs:** All current songs (chiptune-march, ambient-drift,
etc.) use sequential track calls and would now **overlap**. This is the desired
behavior. Songs that *intentionally* want sequential layout would use explicit
step durations: `melody() 8; harmony() 8;`.

**Test updates:** Existing compiler tests that assert sequential timing will need
to be updated to expect parallel timing.

### Phase 2: Add Track Names to Events

**Goal:** Every `Event` carries the name of the track it came from (or `None` for
top-level events). This enables track isolation (solo/mute).

1. **Add field to `Event`:**
   ```rust
   struct Event {
       time: f64,
       kind: EventKind,
       track_name: Option<String>,  // NEW
   }
   ```

2. **Add `current_track_name` to `CompileCtx`:**
   ```rust
   current_track_name: Option<String>,  // None at top level
   ```

3. **Set in `inline_track_call()`:**
   ```rust
   let saved_track = ctx.current_track_name.clone();
   ctx.current_track_name = Some(track_name.clone());
   // ... compile body ...
   ctx.current_track_name = saved_track;
   ```

4. **Stamp on every event:**
   ```rust
   ctx.events.push(Event {
       time: ctx.cursor,
       kind: ...,
       track_name: ctx.current_track_name.clone(),
   });
   ```

### Phase 3: Source Spans on All AST Nodes + Byte Offset Switch

**Goal:** Every AST node carries byte-offset spans into the original source.

1. **Lexer change:** Switch from `chars: Vec<char>` + char-index `pos` to
   iterating with `char_indices()` and tracking byte position. `Span { start, end }`
   becomes byte offsets into the original `&str`.

2. **Parser change:** For every `Statement` and `TrackStatement` variant, capture
   `span_start` (byte offset before parsing) and `span_end` (byte offset after
   the last consumed token). Mechanical change — apply to each `parse_*` method.

3. **AST change:** Add `span_start: usize` and `span_end: usize` fields to:
   - `TrackStatement::Assignment`
   - `TrackStatement::Rest`
   - `TrackStatement::ForLoop`
   - `TrackStatement::TrackCall`
   - `TrackStatement::Comment`
   - `Statement::TrackDef`
   - `Statement::TrackCall`
   - `Statement::ConstDecl`
   - `Statement::Assignment`
   - `Statement::Comment`

### Phase 4: `cursor_context()` — Instrument at Cursor

**Goal:** Given a source string and a byte offset, determine what instrument,
BPM, tuning, and beat position the cursor is at.

```rust
pub struct CursorContext {
    pub instrument: InstrumentConfig,
    pub track_name: Option<String>,
    pub note_length: f64,
    pub bpm: f64,
    pub tuning_pitch: f64,
    pub cursor_beat: f64,
}

pub fn cursor_context(source: &str, cursor_byte_offset: usize) -> Result<CursorContext, CompileError>
```

**Implementation:** Run the compiler (or a lightweight `SongRunner` in dry-run
mode) over the AST, stopping when the current statement's `span_end` exceeds
`cursor_byte_offset`. Return the accumulated state at that point.

For the initial implementation, this uses the existing compiler (flat EventList)
since `cursor_context()` runs over finite song prefixes — the cursor is always at
a known position in the source. This does NOT need the streaming model.

### Phase 5: `render_single_note()` — Piano Note Audio

**Goal:** Render a single note with a given instrument config and return PCM
samples.

```rust
#[wasm_bindgen]
pub fn render_single_note(
    pitch: f64,
    velocity: f64,
    gate_beats: f64,
    bpm: f64,
    sample_rate: u32,
    instrument_json: &str,
    presets_json: &str,
) -> Result<Vec<f32>, JsValue>
```

**Implementation:** Construct a minimal `EventList` with one `Note` event, pass
to `AudioEngine::render()`. Uses `EndMode::Release` so the engine renders the
full ADSR including release. Cap at 4 seconds total output for safety.

This phase does NOT depend on the streaming model — it synthesizes a single note
using the existing `AudioEngine`.

### Phase 6: `get_instrument_at_cursor()` WASM Export

```rust
#[wasm_bindgen]
pub fn get_instrument_at_cursor(
    source: &str,
    cursor_byte_offset: usize,
) -> Result<JsValue, JsValue>
```

Returns a `CursorContext` serialized as JSON. Calls `cursor_context()` internally.

### Phase 7: SongRunner — Streaming AST Interpreter

**Goal:** Replace the batch compiler with an incremental AST interpreter that
produces events into an `EventBuffer`.

This is the **core architectural change** described in Architectural Decision #2.
The `SongRunner`:

1. **Parses** the source into an AST (same lexer/parser as before).
2. **Initializes** a main fiber at beat 0 with default state.
3. **Steps** through the AST one statement at a time per fiber:
   - `NoteEvent` → push `Event` to buffer, advance fiber cursor
   - `Rest` → advance fiber cursor
   - `Assignment` → update fiber/song state
   - `TrackCall` → spawn a new fiber at current cursor (async)
   - `ForLoop` / `while` → push new ExecFrame onto fiber stack
   - `SetProperty` (BPM, tuning) → push `Event` to buffer, update SongState
4. **Yields** when the buffer is full (lead >= `buffer_beats` ahead of playback).
5. **Resumes** when the `AudioEngine` consumes events and makes buffer room.

**Fiber lifecycle:**
- Spawned by TrackCall or at init (main fiber).
- Runs until its statement list is exhausted (or the song is stopped).
- On completion, the fiber is removed from the active list.
- If all fibers complete, the song ends naturally.
- If a fiber enters an infinite loop, it runs forever — the buffer mechanism
  prevents runaway memory (it just pauses when the buffer is full).

**No full EventList.** The `CompileCtx` and `compile()` function remain available
for `cursor_context()` and other tooling queries, but **playback** uses the
SongRunner exclusively.

### Phase 8: Streaming AudioEngine

**Goal:** Modify the `AudioEngine` to consume events from an `EventBuffer`
instead of a pre-computed `EventList`.

**Current engine flow:**
```
1. Pre-scan all events for BPM/tuning
2. Convert all Note events to ScheduledNote (sample offsets)
3. Render in 128-sample blocks, activating voices when their sample offset arrives
```

**New engine flow:**
```
1. No pre-scan (BPM/tuning arrive as streaming events)
2. Each render_block():
   a. Drain events from buffer where event.time <= current_beat
   b. Process SetProperty events (BPM, tuning) immediately
   c. Convert Note events to active voices
   d. Render 128 samples of audio from active voices
   e. Advance current_beat by (128 / sample_rate) * (bpm / 60)
3. Expose peek_ahead() so active voices can query upcoming events
```

**BPM handling:** Without pre-scan, the engine starts with a default BPM (120)
and updates when it encounters a `SetProperty { target: "bpm", .. }` event.
This is correct — the SongRunner emits BPM changes at the beat they occur, and
the engine picks them up in order.

**Pre-buffer awareness (Architectural Decision #3):** When activating or updating
a voice, the engine passes a lookahead slice from `buffer.peek_ahead()`. The
voice's instrument can inspect upcoming notes:

```rust
impl Voice {
    fn activate(
        &mut self,
        note: &ScheduledNote,
        lookahead: &[Event],  // upcoming events in the buffer
    ) {
        // Instruments can inspect lookahead for:
        // - Next note for same instrument (legato/portamento)
        // - Upcoming preset changes
        // - Phrase boundaries
    }
}
```

### Phase 9: Cursor-Aware Playback (Play From Here)

**Goal:** Start playback from the cursor position. The SongRunner executes the
song up to the cursor beat (setting BPM, instruments, presets, effects) but
**skips audio output** for all notes before the cursor. Audio begins at the
cursor beat.

**Implementation with SongRunner:**
1. Use `cursor_context()` to find the `cursor_beat` for the given byte offset.
2. Create a SongRunner and execute in **silent mode** until `playback_beat >=
   cursor_beat`. In silent mode, the SongRunner processes all statements (updating
   state, spawning fibers) but the AudioEngine does not render audio for events
   before `cursor_beat`.
3. Once at the cursor beat, switch to normal streaming mode — audio output begins.

This is more efficient than the old approach (compile full EventList, then skip)
because the SongRunner only needs to process events up to the cursor, not the
entire song.

**WASM export:**
```rust
#[wasm_bindgen]
pub fn start_playback_from_cursor(
    source: &str,
    cursor_byte_offset: usize,
    sample_rate: u32,
    presets_json: &str,
) -> Result<Vec<f32>, JsValue>
```

Note: For the web/WASM target, the SongRunner runs in a single call that returns
a buffer of rendered audio. For real-time playback, the JS layer calls repeatedly
with the SongRunner as persistent state (or the SongRunner runs in a
Web Worker / AudioWorklet and streams chunks).

### Phase 10: Track-Filtered Rendering (Solo/Mute)

**Goal:** Render only events belonging to specific tracks.

With the streaming model, filtering happens at the **EventBuffer consumer** level.
The AudioEngine applies a track filter when draining events:

```rust
impl EventBuffer {
    fn drain_filtered(
        &mut self,
        up_to_beat: f64,
        solo_tracks: Option<&[String]>,
        muted_tracks: &[String],
    ) -> Vec<Event> {
        self.drain_up_to(up_to_beat)
            .into_iter()
            .filter(|e| {
                if let Some(ref name) = e.track_name {
                    if let Some(solo) = solo_tracks {
                        return solo.iter().any(|s| s == name);
                    }
                    return !muted_tracks.iter().any(|m| m == name);
                }
                true // top-level events always pass
            })
            .collect()
    }
}
```

Muted events are still **consumed** (removed from the buffer) — they're just not
sent to the AudioEngine for rendering. This keeps the buffer flowing and prevents
the SongRunner from stalling.

### Phase 11: Extract Track Names

**Goal:** Let the UI discover available track names for solo/mute controls.

```rust
#[wasm_bindgen]
pub fn get_track_names(source: &str) -> Result<JsValue, JsValue>
// Returns JSON array: ["melody", "harmony", "bass", "drums"]
```

Extracted from the AST's `TrackDef` names — no compilation or execution needed.

---

## Resolved Decisions

1. **Tracks are async functions.** Track calls don't advance the caller's cursor.
   Consecutive track calls overlap on the timeline (parallel). This makes track
   isolation musically meaningful. See Architectural Decision #1.

2. **No conditional preset logic.** The system executes whatever instrument is
   loaded at the cursor. If nothing is loaded, that's the default triangle
   oscillator — which plays. If a preset is referenced but not yet decoded, the
   engine's built-in fallback provides a triangle. No error messages, no special
   cases. Triangle *is* the "no instrument loaded" sound and users will learn this.

3. **Byte offsets, not char indices.** The lexer will be updated to track byte
   positions into the original `&str`, not indices into a `Vec<char>`. This gives
   precision that maps directly to Rust's `&source[..offset]` and to JavaScript's
   `TextEncoder`/`TextDecoder`.

4. **Streaming execution, not batch compilation.** The song executes incrementally
   through a SongRunner that produces events into a bounded EventBuffer. The full
   EventList never exists. This supports infinite songs, bounds memory, and enables
   pre-buffer instrument awareness. See Architectural Decision #2.

5. **Pre-buffer instrument awareness.** Instruments can peek ahead in the
   EventBuffer to anticipate upcoming notes. This enables legato, portamento,
   sample pre-loading, and anticipatory effects. It's an optimization layer on
   top of the streaming model. See Architectural Decision #3.

## Outstanding Questions

### Design Decisions (non-blocking, defaults chosen)

1. **Track name format:** Using flat names (`"melody"`, not `"melody/arpeggio"`).
   Nested track calls tag events with the innermost track name. Can revisit later
   if hierarchical names prove useful.

2. **`CursorContext` scope:** Including `note_length`, `bpm`, `tuning_pitch`, and
   `cursor_beat` alongside the instrument. This makes the piano more accurate.

3. **Release tail duration:** `render_single_note()` uses `EndMode::Release`. The
   engine renders until all voices finish their ADSR release. For safety, cap at
   4 seconds of total output (gate + release).

4. **Buffer size:** Default of 4 beats. This is a tunable parameter. Larger buffers
   give more lookahead for instruments but use more memory and add latency before
   the first audio frame. 4 beats is a good default (one measure at 4/4 time).

5. **SongRunner in WASM:** For the web target, the SongRunner needs to persist
   across multiple JS calls (it holds execution state). Options:
   - (a) Return the full rendered buffer each call (works for export, not real-time)
   - (b) Store SongRunner in a WASM-side global, expose `step()`/`render_block()`
   - (c) Run in AudioWorklet with SharedArrayBuffer
   Default: start with (a) for offline rendering, add (b) for real-time later.

### Implementation Concerns

6. **Existing song migration:** When tracks become async (Phase 1), all existing
   songs will sound different (tracks overlap instead of playing sequentially).
   The songs in `songwalker-site/public/songs/` and `songwalker-library/` need
   to be checked. Some may already sound correct overlapping; others may need
   explicit step durations to re-create sequential behavior.

7. **Performance of `cursor_context()`:** Full compile on every piano keypress
   could lag for large songs. Mitigation: cache the result keyed by a hash
   of the source text + cursor position, invalidate on edit. The WASM layer can
   hold this cache.

8. **Nested track instrument scoping:** When track A passes an instrument to
   track B as a parameter (`melody(piano)`), and the user's cursor is inside B's
   body after `track.instrument = inst`, the `CursorContext` should reflect the
   resolved instrument (the actual piano preset, not the parameter name). The
   current approach handles this because events carry the fully-resolved
   `InstrumentConfig` clone.

9. **SongRunner + compiler co-existence:** The SongRunner replaces the compiler
   for playback, but `cursor_context()` and `get_track_names()` still use the
   compiler/parser directly (they don't need streaming). The compiler code is
   retained, not deleted. Over time the SongRunner may subsume `cursor_context()`
   too (dry-run to cursor position), but for now both paths coexist.

## Implementation Order

Phases are ordered by dependency and progressive value delivery:

1. **Phase 1** — Async track execution. Fundamental timing change. Update songs.
2. **Phase 2** — Track names on events. Enables isolation.
3. **Phase 3** — Source spans + byte offsets. Enables cursor mapping.
4. **Phase 4** — `cursor_context()`. Core logic for cursor-aware features.
5. **Phase 5** — `render_single_note()`. Enables piano keyboard audio.
6. **Phase 6** — `get_instrument_at_cursor()` WASM export. Connects Phase 4 to web.
7. **Phase 11** — `get_track_names()`. Enables solo/mute UI.
8. **Phase 7** — SongRunner (streaming interpreter). Core architecture change.
9. **Phase 8** — Streaming AudioEngine. Consumes from EventBuffer.
10. **Phase 10** — Track-filtered rendering (solo/mute on streaming model).
11. **Phase 9** — Cursor-aware playback (play from here, using SongRunner).

**Rationale:** Phases 1–6 and 11 work with the existing compiler and deliver the
piano keyboard feature. Phases 7–8 are the big architectural shift to streaming.
Phases 9–10 build on the streaming model for full DAW-grade workflow.

## File Impact

| File | Phase | Changes |
|------|-------|---------|
| `src/compiler.rs` | 1 | Restore cursor in `inline_track_call()` |
| `src/compiler.rs` | 2 | Add `current_track_name` to ctx, stamp on events |
| `src/compiler.rs` | 4 | Add `cursor_context()` public function |
| `src/ast.rs` | 3 | Add `span_start`/`span_end` to all AST node variants |
| `src/parser.rs` | 3 | Record byte offsets when parsing all statement types |
| `src/lexer.rs` | 3 | Switch from char index to byte offset tracking |
| `src/token.rs` | 3 | Document that `Span` values are byte offsets |
| `src/lib.rs` | 5,6,11 | New WASM exports |
| `src/runner.rs` | 7 | **NEW FILE** — SongRunner, Fiber, ExecFrame |
| `src/event_buffer.rs` | 7 | **NEW FILE** — EventBuffer ring buffer |
| `src/dsp/engine.rs` | 8 | Streaming consumption from EventBuffer, BPM-on-the-fly, peek_ahead for voices |
| `src/lib.rs` | 9 | WASM exports for streaming playback (start_playback_from_cursor) |
| All test files | 1,2 | Update Event construction, update timing assertions |
| `public/songs/*.sw` | 1 | Verify/update songs for parallel track behavior |
