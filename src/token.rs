#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    // Literals
    Number(f64),
    StringLit(String),
    RegexLit(String),
    Ident(String),

    // Keywords
    Track,
    Const,
    Let,
    For,

    // Punctuation
    Star,       // *
    At,         // @
    Slash,      // /
    Dot,        // .
    Semicolon,  // ;
    Comma,      // ,
    Eq,         // =
    LParen,     // (
    RParen,     // )
    LBracket,   // [
    RBracket,   // ]
    LBrace,     // {
    RBrace,     // }
    Lt,         // <
    Gt,         // >
    Plus,       // +
    Minus,      // -
    PlusPlus,   // ++
    MinusMinus, // --
    Colon,      // :

    // Structural
    Newline,
    Comment(String),
    EOF,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone)]
pub struct Spanned {
    pub token: Token,
    pub span: Span,
}

/// Convert a token back to its approximate source representation.
pub fn token_to_string(token: &Token) -> String {
    match token {
        Token::Number(n) => {
            if *n == (*n as i64) as f64 {
                format!("{}", *n as i64)
            } else {
                format!("{n}")
            }
        }
        Token::StringLit(s) => format!("\"{s}\""),
        Token::RegexLit(s) => s.clone(),
        Token::Ident(s) => s.clone(),
        Token::Track => "track".into(),
        Token::Const => "const".into(),
        Token::Let => "let".into(),
        Token::For => "for".into(),
        Token::Star => "*".into(),
        Token::At => "@".into(),
        Token::Slash => "/".into(),
        Token::Dot => ".".into(),
        Token::Semicolon => ";".into(),
        Token::Comma => ",".into(),
        Token::Eq => "=".into(),
        Token::LParen => "(".into(),
        Token::RParen => ")".into(),
        Token::LBracket => "[".into(),
        Token::RBracket => "]".into(),
        Token::LBrace => "{".into(),
        Token::RBrace => "}".into(),
        Token::Lt => "<".into(),
        Token::Gt => ">".into(),
        Token::Plus => "+".into(),
        Token::Minus => "-".into(),
        Token::PlusPlus => "++".into(),
        Token::MinusMinus => "--".into(),
        Token::Colon => ":".into(),
        Token::Newline => "\n".into(),
        Token::Comment(s) => format!("// {s}"),
        Token::EOF => "".into(),
    }
}
