use serde::{Deserialize, Serialize};

/// A complete SongWalker program.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Program {
    pub statements: Vec<Statement>,
}

/// A top-level statement.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Statement {
    /// `track name(params) { body }`
    TrackDef {
        name: String,
        params: Vec<String>,
        body: Vec<TrackStatement>,
        span_start: usize,
        span_end: usize,
    },
    /// `name*vel@dur(args) step;`
    TrackCall {
        name: String,
        velocity: Option<f64>,
        play_duration: Option<DurationExpr>,
        args: Vec<Expr>,
        step: Option<DurationExpr>,
        span_start: usize,
        span_end: usize,
    },
    /// `const name = expr;`
    ConstDecl {
        name: String,
        value: Expr,
        span_start: usize,
        span_end: usize,
    },
    /// `target = value;`
    Assignment {
        target: String,
        value: Expr,
        span_start: usize,
        span_end: usize,
    },
    /// `// text`
    Comment(String),
}

/// A statement inside a track body.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TrackStatement {
    /// `C3*vel@audible /step`
    NoteEvent {
        pitch: String,
        velocity: Option<f64>,
        audible_duration: Option<DurationExpr>,
        step_duration: Option<DurationExpr>,
        /// Source byte offset (start).
        span_start: usize,
        /// Source byte offset (end).
        span_end: usize,
    },
    /// `[C3@2, E3, G3]@dur /step`
    Chord {
        notes: Vec<ChordNote>,
        audible_duration: Option<DurationExpr>,
        step_duration: Option<DurationExpr>,
        /// Source byte offset (start).
        span_start: usize,
        /// Source byte offset (end).
        span_end: usize,
    },
    /// Standalone number = rest for N beats.
    Rest {
        duration: DurationExpr,
        span_start: usize,
        span_end: usize,
    },
    /// `target = value;`
    Assignment {
        target: String,
        value: Expr,
        span_start: usize,
        span_end: usize,
    },
    /// `for (init; cond; update) { body }`
    ForLoop {
        init: String,
        condition: String,
        update: String,
        body: Vec<TrackStatement>,
        span_start: usize,
        span_end: usize,
    },
    /// A track call inside another track.
    TrackCall {
        name: String,
        velocity: Option<f64>,
        play_duration: Option<DurationExpr>,
        args: Vec<Expr>,
        step: Option<DurationExpr>,
        span_start: usize,
        span_end: usize,
    },
    /// `// text`
    Comment(String),
}

/// A note within a chord.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChordNote {
    pub pitch: String,
    pub audible_duration: Option<DurationExpr>,
}

/// A duration expression.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum DurationExpr {
    /// `/N` shorthand for 1/N (e.g., `/4` = quarter note).
    Inverse(f64),
    /// `N/M` fraction (e.g., `1/4`, `3/8`).
    Fraction(f64, f64),
    /// Plain beat count (e.g., `2`, `8`).
    Beats(f64),
    /// Dot shorthand: `.` = 1x default, `..` = 2x, etc.
    Dots(usize),
}

/// A general expression (simplified for Phase 1).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Expr {
    Number(f64),
    StringLit(String),
    RegexLit(String),
    Identifier(String),
    Array(Vec<Expr>),
    ObjectLit(Vec<(String, Expr)>),
    /// `Oscillator({type: 'square'})` or `loadPreset("name")` — preset/instrument call.
    FunctionCall {
        function: String,
        args: Vec<Expr>,
    },
    PropertyAccess {
        object: String,
        property: String,
    },
    DurationLit(DurationExpr),
}

// ── Span accessors ──────────────────────────────────────────

impl Statement {
    /// Returns the source byte range `(span_start, span_end)` for this statement.
    /// Comments have no span information and return `(usize::MAX, usize::MAX)`.
    pub fn span(&self) -> (usize, usize) {
        match self {
            Statement::TrackDef { span_start, span_end, .. }
            | Statement::TrackCall { span_start, span_end, .. }
            | Statement::ConstDecl { span_start, span_end, .. }
            | Statement::Assignment { span_start, span_end, .. } => (*span_start, *span_end),
            Statement::Comment(_) => (usize::MAX, usize::MAX),
        }
    }
}

impl TrackStatement {
    /// Returns the source byte range `(span_start, span_end)` for this statement.
    /// Comments have no span information and return `(usize::MAX, usize::MAX)`.
    pub fn span(&self) -> (usize, usize) {
        match self {
            TrackStatement::NoteEvent { span_start, span_end, .. }
            | TrackStatement::Chord { span_start, span_end, .. }
            | TrackStatement::Rest { span_start, span_end, .. }
            | TrackStatement::Assignment { span_start, span_end, .. }
            | TrackStatement::ForLoop { span_start, span_end, .. }
            | TrackStatement::TrackCall { span_start, span_end, .. } => (*span_start, *span_end),
            TrackStatement::Comment(_) => (usize::MAX, usize::MAX),
        }
    }
}
