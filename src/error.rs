use crate::token::{Span, Token};
use std::fmt;

#[derive(Debug)]
pub enum SongWalkerError {
    Lex(LexError),
    Parse(ParseError),
}

#[derive(Debug)]
pub enum LexError {
    UnexpectedChar { ch: char, pos: usize },
    UnterminatedString { pos: usize },
    UnterminatedRegex { pos: usize },
    InvalidNumber { text: String, pos: usize },
}

#[derive(Debug)]
pub enum ParseError {
    UnexpectedToken {
        expected: String,
        found: Token,
        span: Span,
    },
    UnexpectedEOF {
        expected: String,
    },
}

impl fmt::Display for SongWalkerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SongWalkerError::Lex(e) => write!(f, "Lexer error: {e:?}"),
            SongWalkerError::Parse(e) => write!(f, "Parse error: {e:?}"),
        }
    }
}

impl std::error::Error for SongWalkerError {}

impl fmt::Display for LexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LexError::UnexpectedChar { ch, pos } => write!(f, "Unexpected char '{ch}' at pos {pos}"),
            LexError::UnterminatedString { pos } => write!(f, "Unterminated string at pos {pos}"),
            LexError::UnterminatedRegex { pos } => write!(f, "Unterminated regex at pos {pos}"),
            LexError::InvalidNumber { text, pos } => write!(f, "Invalid number '{text}' at pos {pos}"),
        }
    }
}

impl std::error::Error for LexError {}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::UnexpectedToken { expected, found, span } => {
                write!(f, "Expected {expected}, found {found:?} at pos {}", span.start)
            }
            ParseError::UnexpectedEOF { expected } => {
                write!(f, "Unexpected end of file, expected {expected}")
            }
        }
    }
}

impl std::error::Error for ParseError {}

impl From<LexError> for SongWalkerError {
    fn from(e: LexError) -> Self {
        SongWalkerError::Lex(e)
    }
}

impl From<ParseError> for SongWalkerError {
    fn from(e: ParseError) -> Self {
        SongWalkerError::Parse(e)
    }
}
