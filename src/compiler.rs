use std::collections::HashMap;
use serde::{Deserialize, Serialize};

use crate::ast::*;

// ── Song End Mode ───────────────────────────────────────────

/// Controls how the engine determines the total output length.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum EndMode {
    /// Hard cut when the last note's gate ends (note-off).
    Gate,
    /// Wait for all envelope releases to finish.
    Release,
    /// Wait for all notes and effects to finish (default).
    Tail,
}

impl Default for EndMode {
    fn default() -> Self {
        EndMode::Tail
    }
}

// ── Instrument Configuration ────────────────────────────────

/// Built-in instrument configuration resolved at compile time.
///
/// Tracks are independent units — they receive instruments through parameters
/// or inherit the parent track's instrument. The song context is passed
/// implicitly, so `const` values at song level are accessible.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InstrumentConfig {
    /// Waveform type: "sine", "square", "sawtooth", "triangle".
    pub waveform: String,
    /// ADSR envelope attack time in seconds (None = use engine default).
    pub attack: Option<f64>,
    /// ADSR envelope decay time in seconds.
    pub decay: Option<f64>,
    /// ADSR envelope sustain level [0, 1].
    pub sustain: Option<f64>,
    /// ADSR envelope release time in seconds.
    pub release: Option<f64>,
    /// Detune in cents.
    pub detune: Option<f64>,
    /// Mix level [0, 1].
    pub mixer: Option<f64>,
    /// Preset reference name (from `loadPreset("name")`).
    /// Used for compile-time extraction and runtime preloading.
    pub preset_ref: Option<String>,
}

impl Default for InstrumentConfig {
    fn default() -> Self {
        InstrumentConfig {
            waveform: "triangle".to_string(),
            attack: None,
            decay: None,
            sustain: None,
            release: None,
            detune: None,
            mixer: None,
            preset_ref: None,
        }
    }
}

// ── Event List (Compiler Output) ────────────────────────────

/// The compiled output: a flat list of timed events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventList {
    /// All events sorted by time.
    pub events: Vec<Event>,
    /// Total duration of the song in beats (cursor position at end).
    pub total_beats: f64,
    /// How the engine should determine the end of the audio.
    pub end_mode: EndMode,
}

/// A single scheduled event.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Event {
    /// When this event fires, in beats from the start.
    pub time: f64,
    pub kind: EventKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum EventKind {
    /// Play a note.
    Note {
        pitch: String,
        velocity: f64,
        /// Audible gate time in beats (how long the note sounds).
        gate: f64,
        /// Instrument configuration for this note.
        instrument: InstrumentConfig,
        /// Source byte offset (for editor highlighting).
        source_start: usize,
        /// Source byte end offset.
        source_end: usize,
    },
    /// Start a sub-track.
    TrackStart {
        track_name: String,
        velocity: Option<f64>,
        play_duration: Option<f64>,
        args: Vec<String>,
    },
    /// Set a property.
    SetProperty { target: String, value: String },
    /// Preset reference (for compile-time extraction / preloading).
    PresetRef { name: String },
}

// ── Compiler ────────────────────────────────────────────────

/// Compile context: tracks state during compilation.
struct CompileCtx {
    /// Default note length in beats (e.g., 1/4 = 0.25).
    default_note_length: f64,
    /// Song end mode.
    end_mode: EndMode,
    /// Current instrument configuration (default = Triangle).
    current_instrument: InstrumentConfig,
    /// Current cursor position in beats.
    cursor: f64,
    /// Collected events.
    events: Vec<Event>,
    /// Track definitions available for lookup.
    track_defs: Vec<TrackDef>,
    /// Song-level const bindings: `const name = Oscillator({...})`.
    consts: HashMap<String, InstrumentConfig>,
    /// Active parameter bindings during track body compilation.
    param_bindings: HashMap<String, InstrumentConfig>,
}

struct TrackDef {
    name: String,
    params: Vec<String>,
    body: Vec<TrackStatement>,
}

impl CompileCtx {
    fn new(_strict: bool) -> Self {
        CompileCtx {
            default_note_length: 1.0, // default: 1 beat
            end_mode: EndMode::Tail,
            current_instrument: InstrumentConfig::default(),
            cursor: 0.0,
            events: Vec::new(),
            track_defs: Vec::new(),
            consts: HashMap::new(),
            param_bindings: HashMap::new(),
        }
    }

    fn emit(&mut self, kind: EventKind) {
        self.events.push(Event {
            time: self.cursor,
            kind,
        });
    }

    fn resolve_duration(&self, dur: &Option<DurationExpr>) -> f64 {
        match dur {
            Some(d) => duration_to_beats(d, self.default_note_length),
            None => self.default_note_length,
        }
    }
}

/// Convert a DurationExpr to a beat count.
fn duration_to_beats(dur: &DurationExpr, default: f64) -> f64 {
    match dur {
        DurationExpr::Beats(n) => *n,
        DurationExpr::Inverse(n) => 1.0 / n,
        DurationExpr::Fraction(n, m) => n / m,
        DurationExpr::Dots(count) => default * (*count as f64),
    }
}

fn expr_to_string(expr: &Expr) -> String {
    match expr {
        Expr::Identifier(s) => s.clone(),
        Expr::StringLit(s) => s.clone(),
        Expr::Number(n) => format!("{n}"),
        Expr::RegexLit(s) => s.clone(),
        Expr::FunctionCall { function, .. } => format!("{function}(...)"),
        _ => format!("{expr:?}"),
    }
}

// ── Public API ──────────────────────────────────────────────

/// Compile a parsed Program into a flat EventList.
///
/// Phase 1: Compiles a single-pass arrangement. Tracks are inlined,
/// for-loops are unrolled, and the output is a flat timeline.
pub fn compile(program: &Program) -> Result<EventList, String> {
    compile_inner(program, false)
}

/// Compile with strict validation (editor mode).
/// Errors if a note is played before track.instrument is set.
pub fn compile_strict(program: &Program) -> Result<EventList, String> {
    compile_inner(program, true)
}

fn compile_inner(program: &Program, strict: bool) -> Result<EventList, String> {
    let mut ctx = CompileCtx::new(strict);

    // First pass: collect track definitions.
    for stmt in &program.statements {
        if let Statement::TrackDef { name, params, body } = stmt {
            ctx.track_defs.push(TrackDef {
                name: name.clone(),
                params: params.clone(),
                body: body.clone(),
            });
        }
    }

    // Second pass: compile top-level statements.
    for stmt in &program.statements {
        compile_statement(&mut ctx, stmt)?;
    }

    ctx.events.sort_by(|a, b| a.time.partial_cmp(&b.time).unwrap());

    Ok(EventList {
        total_beats: ctx.cursor,
        events: ctx.events,
        end_mode: ctx.end_mode,
    })
}

fn compile_statement(ctx: &mut CompileCtx, stmt: &Statement) -> Result<(), String> {
    match stmt {
        Statement::TrackDef { .. } => {
            // Already collected in first pass; skip.
            Ok(())
        }
        Statement::TrackCall {
            name,
            velocity,
            play_duration,
            args,
            step,
        } => {
            inline_track_call(ctx, name, velocity, play_duration, args, step)
        }
        Statement::ConstDecl { name, value } => {
            // Resolve the expression to an InstrumentConfig and store it.
            let config = evaluate_instrument_expr(ctx, value)?;
            // Emit a PresetRef event if this references an external preset.
            if let Some(ref preset_name) = config.preset_ref {
                ctx.events.push(Event {
                    time: 0.0,
                    kind: EventKind::PresetRef {
                        name: preset_name.clone(),
                    },
                });
            }
            ctx.consts.insert(name.clone(), config);
            Ok(())
        }
        Statement::Assignment { target, value } => {
            compile_assignment(ctx, target, value)
        }
        Statement::Comment(_) => Ok(()),
    }
}

/// Evaluate an expression to an InstrumentConfig.
fn evaluate_instrument_expr(ctx: &CompileCtx, expr: &Expr) -> Result<InstrumentConfig, String> {
    match expr {
        Expr::FunctionCall { function, args } => {
            match function.as_str() {
                "Oscillator" => {
                    let mut config = InstrumentConfig::default();
                    // First arg should be an ObjectLit with config keys.
                    if let Some(Expr::ObjectLit(pairs)) = args.first() {
                        for (key, value) in pairs {
                            match key.as_str() {
                                "type" => {
                                    if let Expr::StringLit(s) = value {
                                        config.waveform = s.clone();
                                    }
                                }
                                "attack" => {
                                    if let Expr::Number(n) = value {
                                        config.attack = Some(*n);
                                    }
                                }
                                "decay" => {
                                    if let Expr::Number(n) = value {
                                        config.decay = Some(*n);
                                    }
                                }
                                "sustain" => {
                                    if let Expr::Number(n) = value {
                                        config.sustain = Some(*n);
                                    }
                                }
                                "release" => {
                                    if let Expr::Number(n) = value {
                                        config.release = Some(*n);
                                    }
                                }
                                "detune" => {
                                    if let Expr::Number(n) = value {
                                        config.detune = Some(*n);
                                    }
                                }
                                "mixer" => {
                                    if let Expr::Number(n) = value {
                                        config.mixer = Some(*n);
                                    }
                                }
                                _ => {} // ignore unknown keys
                            }
                        }
                    }
                    Ok(config)
                }
                "loadPreset" => {
                    // loadPreset("name") — resolve preset by name.
                    // Currently produces a default config; runtime preloading
                    // uses extract_preset_refs() to discover references.
                    let mut config = InstrumentConfig::default();
                    if let Some(Expr::StringLit(preset_name)) = args.first() {
                        config.preset_ref = Some(preset_name.clone());
                        // If the preset name looks like an oscillator type, use it
                        match preset_name.as_str() {
                            "Oscillator" => {
                                if let Some(Expr::ObjectLit(pairs)) = args.get(1) {
                                    for (key, value) in pairs {
                                        match key.as_str() {
                                            "type" => {
                                                if let Expr::StringLit(s) = value {
                                                    config.waveform = s.clone();
                                                }
                                            }
                                            "attack" => {
                                                if let Expr::Number(n) = value {
                                                    config.attack = Some(*n);
                                                }
                                            }
                                            "decay" => {
                                                if let Expr::Number(n) = value {
                                                    config.decay = Some(*n);
                                                }
                                            }
                                            "sustain" => {
                                                if let Expr::Number(n) = value {
                                                    config.sustain = Some(*n);
                                                }
                                            }
                                            "release" => {
                                                if let Expr::Number(n) = value {
                                                    config.release = Some(*n);
                                                }
                                            }
                                            "detune" => {
                                                if let Expr::Number(n) = value {
                                                    config.detune = Some(*n);
                                                }
                                            }
                                            "mixer" => {
                                                if let Expr::Number(n) = value {
                                                    config.mixer = Some(*n);
                                                }
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                            }
                            _ => {
                                // External preset — will be loaded at runtime
                            }
                        }
                    }
                    Ok(config)
                }
                _ => Err(format!("Unknown instrument preset '{function}'.")),
            }
        }
        Expr::Identifier(name) => {
            // Look up in param_bindings first, then consts.
            if let Some(cfg) = ctx.param_bindings.get(name) {
                Ok(cfg.clone())
            } else if let Some(cfg) = ctx.consts.get(name) {
                Ok(cfg.clone())
            } else {
                Err(format!("Unknown instrument '{name}'."))
            }
        }
        Expr::StringLit(s) => {
            // Shorthand: 'triangle', 'square', etc.
            Ok(InstrumentConfig {
                waveform: s.clone(),
                ..InstrumentConfig::default()
            })
        }
        _ => Err(format!("Cannot resolve expression as instrument: {expr:?}")),
    }
}

/// Handle an assignment statement (works for both top-level and track body).
fn compile_assignment(ctx: &mut CompileCtx, target: &str, value: &Expr) -> Result<(), String> {
    if target == "track.beatsPerMinute" {
        ctx.emit(EventKind::SetProperty {
            target: target.to_string(),
            value: expr_to_string(value),
        });
    } else if target == "track.tuningPitch" || target == "track.a4Frequency" {
        // Emit as track.tuningPitch regardless of which alias was used.
        ctx.emit(EventKind::SetProperty {
            target: "track.tuningPitch".to_string(),
            value: expr_to_string(value),
        });
    } else if target == "track.noteLength" || target == "track.duration" {
        if let Expr::DurationLit(d) = value {
            ctx.default_note_length = duration_to_beats(d, ctx.default_note_length);
        } else if let Expr::Number(n) = value {
            ctx.default_note_length = *n;
        }
    } else if target == "song.endMode" {
        let mode_str = expr_to_string(value);
        ctx.end_mode = match mode_str.as_str() {
            "gate" => EndMode::Gate,
            "release" => EndMode::Release,
            "tail" => EndMode::Tail,
            _ => {
                return Err(format!(
                    "Unknown song.endMode '{}'. Expected 'gate', 'release', or 'tail'.",
                    mode_str
                ));
            }
        };
    } else if target == "track.instrument" {
        // Resolve the value to an InstrumentConfig.
        let config = evaluate_instrument_expr(ctx, value)?;
        ctx.current_instrument = config;
        ctx.emit(EventKind::SetProperty {
            target: target.to_string(),
            value: expr_to_string(value),
        });
    } else {
        ctx.emit(EventKind::SetProperty {
            target: target.to_string(),
            value: expr_to_string(value),
        });
    }
    Ok(())
}

/// Inline a track call: resolve args → params, save/restore scope, compile body.
fn inline_track_call(
    ctx: &mut CompileCtx,
    name: &str,
    _velocity: &Option<f64>,
    play_duration: &Option<DurationExpr>,
    args: &[Expr],
    step: &Option<DurationExpr>,
) -> Result<(), String> {
    let track_body = ctx
        .track_defs
        .iter()
        .find(|td| td.name == name)
        .map(|td| (td.params.clone(), td.body.clone()));

    if let Some((params, body)) = track_body {
        // Save parent scope.
        let saved_cursor = ctx.cursor;
        let saved_note_len = ctx.default_note_length;
        let saved_instrument = ctx.current_instrument.clone();
        let saved_params = ctx.param_bindings.clone();

        // Resolve args → params: zip track def params with call args.
        let mut new_bindings = ctx.param_bindings.clone();
        for (param_name, arg_expr) in params.iter().zip(args.iter()) {
            let config = evaluate_instrument_expr(ctx, arg_expr)?;
            new_bindings.insert(param_name.clone(), config);
        }
        ctx.param_bindings = new_bindings;

        // Compile the track body inline (inherits parent state).
        compile_track_body(ctx, &body)?;

        // If play_duration is set, cap the track's extent.
        if let Some(pd) = play_duration {
            let max_dur = duration_to_beats(pd, ctx.default_note_length);
            ctx.cursor = saved_cursor + max_dur;
        }

        // Restore parent scope.
        ctx.default_note_length = saved_note_len;
        ctx.current_instrument = saved_instrument;
        ctx.param_bindings = saved_params;

        // Apply step (rest after the track call).
        if let Some(s) = step {
            let step_beats = duration_to_beats(s, ctx.default_note_length);
            ctx.cursor = saved_cursor + step_beats;
        }
    } else {
        // Unknown track: emit as a TrackStart event.
        let arg_strings: Vec<String> = args.iter().map(expr_to_string).collect();
        ctx.emit(EventKind::TrackStart {
            track_name: name.to_string(),
            velocity: *_velocity,
            play_duration: play_duration
                .as_ref()
                .map(|d| duration_to_beats(d, ctx.default_note_length)),
            args: arg_strings,
        });
        if let Some(s) = step {
            ctx.cursor += duration_to_beats(s, ctx.default_note_length);
        }
    }
    Ok(())
}

fn compile_track_body(ctx: &mut CompileCtx, body: &[TrackStatement]) -> Result<(), String> {
    for stmt in body {
        compile_track_statement(ctx, stmt)?;
    }
    Ok(())
}

fn compile_track_statement(ctx: &mut CompileCtx, stmt: &TrackStatement) -> Result<(), String> {
    match stmt {
        TrackStatement::NoteEvent {
            pitch,
            velocity,
            audible_duration,
            step_duration,
            span_start,
            span_end,
        } => {
            let vel = velocity.unwrap_or(100.0);
            let audible = ctx.resolve_duration(audible_duration);
            let step = ctx.resolve_duration(step_duration);

            ctx.emit(EventKind::Note {
                pitch: pitch.clone(),
                velocity: vel,
                gate: audible,
                instrument: ctx.current_instrument.clone(),
                source_start: *span_start,
                source_end: *span_end,
            });
            ctx.cursor += step;
            Ok(())
        }
        TrackStatement::Chord {
            notes,
            audible_duration,
            step_duration,
            span_start,
            span_end,
        } => {
            let chord_audible = audible_duration
                .as_ref()
                .map(|d| duration_to_beats(d, ctx.default_note_length));

            for note in notes {
                let note_dur = note
                    .audible_duration
                    .as_ref()
                    .map(|d| duration_to_beats(d, ctx.default_note_length))
                    .or(chord_audible)
                    .unwrap_or(ctx.default_note_length);

                ctx.emit(EventKind::Note {
                    pitch: note.pitch.clone(),
                    velocity: 100.0,
                    gate: note_dur,
                    instrument: ctx.current_instrument.clone(),
                    source_start: *span_start,
                    source_end: *span_end,
                });
            }

            let step = ctx.resolve_duration(step_duration);
            ctx.cursor += step;
            Ok(())
        }
        TrackStatement::Rest(dur) => {
            ctx.cursor += duration_to_beats(dur, ctx.default_note_length);
            Ok(())
        }
        TrackStatement::Assignment { target, value } => {
            compile_assignment(ctx, target, value)
        }
        TrackStatement::ForLoop {
            init: _,
            condition: _,
            update: _,
            body,
        } => {
            // Phase 1: hardcoded unroll — extract loop count from condition.
            // For now, just compile the body once as a placeholder.
            // TODO: properly evaluate loop bounds.
            compile_track_body(ctx, body)?;
            Ok(())
        }
        TrackStatement::TrackCall {
            name,
            velocity,
            play_duration,
            args,
            step,
        } => {
            inline_track_call(ctx, name, velocity, play_duration, args, step)
        }
        TrackStatement::Comment(_) => Ok(()),
    }
}

/// Extract all preset references from a compiled event list.
/// Used for compile-time preloading of preset assets before playback.
pub fn extract_preset_refs(event_list: &EventList) -> Vec<String> {
    let mut refs = Vec::new();
    for event in &event_list.events {
        if let EventKind::PresetRef { name } = &event.kind {
            if !refs.contains(name) {
                refs.push(name.clone());
            }
        }
    }
    refs
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;

    #[test]
    fn test_compile_simple_track() {
        let program = parse(
            r#"
track riff() {
    C3 /2
    D3 /4
    E3 /4
}
riff();
"#,
        )
        .unwrap();

        let events = compile(&program).unwrap();
        assert_eq!(events.total_beats, 1.0); // 0.5 + 0.25 + 0.25

        let notes: Vec<_> = events
            .events
            .iter()
            .filter_map(|e| match &e.kind {
                EventKind::Note { pitch, .. } => Some((e.time, pitch.as_str())),
                _ => None,
            })
            .collect();

        assert_eq!(notes.len(), 3);
        assert_eq!(notes[0], (0.0, "C3"));
        assert_eq!(notes[1], (0.5, "D3"));
        assert_eq!(notes[2], (0.75, "E3"));
    }

    #[test]
    fn test_compile_track_with_rest() {
        let program = parse(
            r#"
track t() {
    C3 /4
    4
    D3 /4
}
t();
"#,
        )
        .unwrap();

        let events = compile(&program).unwrap();
        // 0.25 (C3) + 4.0 (rest) + 0.25 (D3) = 4.5
        assert_eq!(events.total_beats, 4.5);

        let notes: Vec<_> = events
            .events
            .iter()
            .filter_map(|e| match &e.kind {
                EventKind::Note { pitch, .. } => Some((e.time, pitch.as_str())),
                _ => None,
            })
            .collect();

        assert_eq!(notes[0], (0.0, "C3"));
        assert_eq!(notes[1], (4.25, "D3"));
    }

    #[test]
    fn test_song_length_ends_at_last_rest() {
        // Per plan: song ends after the last rest ends, not when last note finishes.
        let program = parse(
            r#"
track t() {
    C3 /4
    D3 /4
}
t();
"#,
        )
        .unwrap();

        let events = compile(&program).unwrap();
        // Two notes, each stepping 0.25 beats.
        // Cursor ends at 0.5, even though the last note (D3) plays for default duration.
        assert_eq!(events.total_beats, 0.5);
    }

    #[test]
    fn test_compile_chord() {
        let program = parse(
            r#"
track t() {
    [C3, E3, G3]@1 /2
}
t();
"#,
        )
        .unwrap();

        let events = compile(&program).unwrap();

        let notes: Vec<_> = events
            .events
            .iter()
            .filter_map(|e| match &e.kind {
                EventKind::Note { pitch, gate, .. } => {
                    Some((e.time, pitch.as_str(), *gate))
                }
                _ => None,
            })
            .collect();

        // All three notes fire at time 0, each with audible gate 1 beat.
        assert_eq!(notes.len(), 3);
        for (time, _, g) in &notes {
            assert_eq!(*time, 0.0);
            assert_eq!(*g, 1.0);
        }
        // Step duration /2 = 0.5 beats.
        assert_eq!(events.total_beats, 0.5);
    }

    #[test]
    fn test_compile_velocity() {
        let program = parse(
            r#"
track t() {
    C3*80 /4
}
t();
"#,
        )
        .unwrap();

        let events = compile(&program).unwrap();
        match &events.events[0].kind {
            EventKind::Note { velocity, .. } => assert_eq!(*velocity, 80.0),
            other => panic!("Expected Note, got {other:?}"),
        }
    }

    #[test]
    fn test_compile_track_call_with_step() {
        let program = parse(
            r#"
track a() {
    C3 /4
}
a() 8;
a();
"#,
        )
        .unwrap();

        let events = compile(&program).unwrap();

        let notes: Vec<_> = events
            .events
            .iter()
            .filter_map(|e| match &e.kind {
                EventKind::Note { pitch, .. } => Some((e.time, pitch.as_str())),
                _ => None,
            })
            .collect();

        // First call: C3 at 0.0, then step 8 beats.
        // Second call: C3 at 8.0.
        assert_eq!(notes[0], (0.0, "C3"));
        assert_eq!(notes[1], (8.0, "C3"));
    }

    #[test]
    fn test_compile_default_duration_override() {
        let program = parse(
            r#"
track t() {
    track.duration = 1/4;
    C3
    D3
}
t();
"#,
        )
        .unwrap();

        let events = compile(&program).unwrap();
        // Each note uses default step = 0.25 beats.
        assert_eq!(events.total_beats, 0.5);

        let notes: Vec<_> = events
            .events
            .iter()
            .filter_map(|e| match &e.kind {
                EventKind::Note { pitch, .. } => Some((e.time, pitch.as_str())),
                _ => None,
            })
            .collect();

        assert_eq!(notes[0], (0.0, "C3"));
        assert_eq!(notes[1], (0.25, "D3"));
    }

    #[test]
    fn test_default_instrument_on_notes() {
        // Notes without explicit instrument get the default Triangle config.
        let program = parse(
            r#"
track riff() {
    C3 /4
}
riff();
"#,
        )
        .unwrap();

        let events = compile(&program).unwrap();
        let note = events.events.iter().find(|e| matches!(&e.kind, EventKind::Note { .. })).unwrap();
        if let EventKind::Note { instrument, .. } = &note.kind {
            assert_eq!(instrument.waveform, "triangle");
        }
    }

    #[test]
    fn test_const_oscillator_instrument() {
        let program = parse(
            r#"
const synth = Oscillator({type: 'square'});
track riff() {
    track.instrument = synth;
    C3 /4
}
riff();
"#,
        )
        .unwrap();

        let events = compile(&program).unwrap();
        let note = events.events.iter().find(|e| matches!(&e.kind, EventKind::Note { .. })).unwrap();
        if let EventKind::Note { instrument, .. } = &note.kind {
            assert_eq!(instrument.waveform, "square");
        }
    }

    #[test]
    fn test_track_param_instrument() {
        // Instrument passed as track parameter — track independence.
        let program = parse(
            r#"
const synth = Oscillator({type: 'sawtooth', attack: 0.05});
melody(synth);

track melody(inst) {
    track.instrument = inst;
    C4 /4
    E4 /4
}
"#,
        )
        .unwrap();

        let events = compile(&program).unwrap();
        let notes: Vec<_> = events.events.iter().filter(|e| matches!(&e.kind, EventKind::Note { .. })).collect();
        assert_eq!(notes.len(), 2);
        for note in &notes {
            if let EventKind::Note { instrument, .. } = &note.kind {
                assert_eq!(instrument.waveform, "sawtooth");
                assert_eq!(instrument.attack, Some(0.05));
            }
        }
    }

    #[test]
    fn test_track_scope_isolation() {
        // Tracks inherit parent state but don't leak changes back.
        let program = parse(
            r#"
const sq = Oscillator({type: 'square'});
const tri = Oscillator({type: 'triangle'});

bass(sq);
melody(tri);

track bass(inst) {
    track.instrument = inst;
    C2 /4
}

track melody(inst) {
    track.instrument = inst;
    C4 /4
}
"#,
        )
        .unwrap();

        let events = compile(&program).unwrap();
        let notes: Vec<_> = events.events.iter().filter_map(|e| match &e.kind {
            EventKind::Note { pitch, instrument, .. } => Some((pitch.as_str(), instrument.waveform.as_str())),
            _ => None,
        }).collect();

        // bass note should be square, melody note should be triangle
        assert!(notes.iter().any(|(p, w)| *p == "C2" && *w == "square"));
        assert!(notes.iter().any(|(p, w)| *p == "C4" && *w == "triangle"));
    }

    #[test]
    fn test_string_shorthand_instrument() {
        let program = parse(
            r#"
track riff() {
    track.instrument = 'square';
    C3 /4
}
riff();
"#,
        )
        .unwrap();

        let events = compile(&program).unwrap();
        let note = events.events.iter().find(|e| matches!(&e.kind, EventKind::Note { .. })).unwrap();
        if let EventKind::Note { instrument, .. } = &note.kind {
            assert_eq!(instrument.waveform, "square");
        }
    }

    #[test]
    fn test_inline_instrument_in_track() {
        let program = parse(
            r#"
track riff() {
    track.instrument = Oscillator({type: 'sine', release: 0.5});
    C3 /4
}
riff();
"#,
        )
        .unwrap();

        let events = compile(&program).unwrap();
        let note = events.events.iter().find(|e| matches!(&e.kind, EventKind::Note { .. })).unwrap();
        if let EventKind::Note { instrument, .. } = &note.kind {
            assert_eq!(instrument.waveform, "sine");
            assert_eq!(instrument.release, Some(0.5));
        }
    }

    #[test]
    fn test_instrument_inherits_from_parent() {
        // Track inherits parent's instrument when not overridden.
        let program = parse(
            r#"
track.instrument = Oscillator({type: 'sawtooth'});
riff();

track riff() {
    C3 /4
}
"#,
        )
        .unwrap();

        let events = compile(&program).unwrap();
        let note = events.events.iter().find(|e| matches!(&e.kind, EventKind::Note { .. })).unwrap();
        if let EventKind::Note { instrument, .. } = &note.kind {
            assert_eq!(instrument.waveform, "sawtooth");
        }
    }

    // ── loadPreset tests ────────────────────────────────────

    #[test]
    fn test_load_preset_sets_preset_ref() {
        // loadPreset("name") should set preset_ref on the instrument config.
        let program = parse(
            r#"
const piano = loadPreset("FluidR3_GM/Acoustic Grand Piano");
track riff() {
    track.instrument = piano;
    C3 /4
}
riff();
"#,
        )
        .unwrap();

        let events = compile(&program).unwrap();
        let note = events.events.iter().find(|e| matches!(&e.kind, EventKind::Note { .. })).unwrap();
        if let EventKind::Note { instrument, .. } = &note.kind {
            assert_eq!(
                instrument.preset_ref,
                Some("FluidR3_GM/Acoustic Grand Piano".to_string())
            );
        } else {
            panic!("Expected Note event");
        }
    }

    #[test]
    fn test_load_preset_emits_preset_ref_event() {
        // A const decl with loadPreset should emit a PresetRef event for preloading.
        let program = parse(
            r#"
const piano = loadPreset("FluidR3_GM/Acoustic Grand Piano");
track riff() {
    track.instrument = piano;
    C3 /4
}
riff();
"#,
        )
        .unwrap();

        let events = compile(&program).unwrap();
        let preset_refs: Vec<_> = events
            .events
            .iter()
            .filter_map(|e| match &e.kind {
                EventKind::PresetRef { name } => Some(name.as_str()),
                _ => None,
            })
            .collect();

        assert_eq!(preset_refs, vec!["FluidR3_GM/Acoustic Grand Piano"]);
    }

    #[test]
    fn test_extract_preset_refs() {
        // extract_preset_refs should collect unique preset references.
        let program = parse(
            r#"
const piano = loadPreset("FluidR3_GM/Acoustic Grand Piano");
const guitar = loadPreset("FluidR3_GM/Nylon Guitar");
track riff() {
    track.instrument = piano;
    C3 /4
}
riff();
"#,
        )
        .unwrap();

        let event_list = compile(&program).unwrap();
        let refs = extract_preset_refs(&event_list);
        assert_eq!(refs.len(), 2);
        assert!(refs.contains(&"FluidR3_GM/Acoustic Grand Piano".to_string()));
        assert!(refs.contains(&"FluidR3_GM/Nylon Guitar".to_string()));
    }

    #[test]
    fn test_extract_preset_refs_deduplicates() {
        // Same preset referenced twice should appear only once.
        let program = parse(
            r#"
const a = loadPreset("FluidR3_GM/Piano");
const b = loadPreset("FluidR3_GM/Piano");
track riff() {
    track.instrument = a;
    C3 /4
}
riff();
"#,
        )
        .unwrap();

        let event_list = compile(&program).unwrap();
        let refs = extract_preset_refs(&event_list);
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0], "FluidR3_GM/Piano");
    }

    #[test]
    fn test_load_preset_default_waveform() {
        // loadPreset for an external preset should still use default waveform.
        let program = parse(
            r#"
const p = loadPreset("SomeLibrary/SomeInstrument");
track riff() {
    track.instrument = p;
    C3 /4
}
riff();
"#,
        )
        .unwrap();

        let events = compile(&program).unwrap();
        let note = events.events.iter().find(|e| matches!(&e.kind, EventKind::Note { .. })).unwrap();
        if let EventKind::Note { instrument, .. } = &note.kind {
            // External presets keep default waveform; runtime replaces it.
            assert_eq!(instrument.waveform, "triangle");
            assert_eq!(
                instrument.preset_ref,
                Some("SomeLibrary/SomeInstrument".to_string())
            );
        }
    }

    #[test]
    fn test_load_preset_oscillator_special_case() {
        // loadPreset("Oscillator", {type: 'square'}) should configure waveform.
        let program = parse(
            r#"
const osc = loadPreset("Oscillator", {type: 'square', attack: 0.1});
track riff() {
    track.instrument = osc;
    C3 /4
}
riff();
"#,
        )
        .unwrap();

        let events = compile(&program).unwrap();
        let note = events.events.iter().find(|e| matches!(&e.kind, EventKind::Note { .. })).unwrap();
        if let EventKind::Note { instrument, .. } = &note.kind {
            assert_eq!(instrument.waveform, "square");
            assert_eq!(instrument.attack, Some(0.1));
            assert_eq!(instrument.preset_ref, Some("Oscillator".to_string()));
        }
    }

    #[test]
    fn test_unknown_instrument_function_errors() {
        // An unknown function name (not Oscillator or loadPreset) should error.
        let program = parse(
            r#"
const x = unknownFunc("foo");
track riff() {
    track.instrument = x;
    C3 /4
}
riff();
"#,
        )
        .unwrap();

        let result = compile(&program);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .contains("Unknown instrument preset 'unknownFunc'"));
    }

    #[test]
    fn test_load_preset_no_args() {
        // loadPreset() with no arguments — preset_ref should be None.
        let program = parse(
            r#"
const p = loadPreset();
track riff() {
    track.instrument = p;
    C3 /4
}
riff();
"#,
        )
        .unwrap();

        let events = compile(&program).unwrap();
        let note = events.events.iter().find(|e| matches!(&e.kind, EventKind::Note { .. })).unwrap();
        if let EventKind::Note { instrument, .. } = &note.kind {
            assert_eq!(instrument.preset_ref, None);
        }
    }

    #[test]
    fn test_load_preset_passed_as_track_param() {
        // loadPreset value passed as a track parameter should propagate correctly.
        let program = parse(
            r#"
const piano = loadPreset("FluidR3_GM/Acoustic Grand Piano");
melody(piano);

track melody(inst) {
    track.instrument = inst;
    C4 /4
    E4 /4
}
"#,
        )
        .unwrap();

        let events = compile(&program).unwrap();
        let notes: Vec<_> = events
            .events
            .iter()
            .filter(|e| matches!(&e.kind, EventKind::Note { .. }))
            .collect();
        assert_eq!(notes.len(), 2);
        for note in &notes {
            if let EventKind::Note { instrument, .. } = &note.kind {
                assert_eq!(
                    instrument.preset_ref,
                    Some("FluidR3_GM/Acoustic Grand Piano".to_string())
                );
            }
        }
    }

    #[test]
    fn test_extract_preset_refs_empty_when_no_presets() {
        // Songs without loadPreset should return empty refs.
        let program = parse(
            r#"
const synth = Oscillator({type: 'square'});
track riff() {
    track.instrument = synth;
    C3 /4
}
riff();
"#,
        )
        .unwrap();

        let event_list = compile(&program).unwrap();
        let refs = extract_preset_refs(&event_list);
        assert!(refs.is_empty());
    }
}
