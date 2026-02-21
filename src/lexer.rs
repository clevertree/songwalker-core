use crate::error::LexError;
use crate::token::{Span, Spanned, Token};

pub struct Lexer {
    chars: Vec<char>,
    /// Precomputed byte offset for each char index.
    /// `byte_offsets[i]` = byte offset of `chars[i]` in the original `&str`.
    /// `byte_offsets[chars.len()]` = total byte length (sentinel for EOF).
    byte_offsets: Vec<usize>,
    pos: usize,
    prev_significant: Option<Token>,
}

impl Lexer {
    pub fn new(input: &str) -> Self {
        let chars: Vec<char> = input.chars().collect();
        // Build a lookup table: char index → byte offset.
        let mut byte_offsets = Vec::with_capacity(chars.len() + 1);
        let mut offset = 0;
        for ch in &chars {
            byte_offsets.push(offset);
            offset += ch.len_utf8();
        }
        byte_offsets.push(offset); // sentinel for EOF
        Lexer {
            chars,
            byte_offsets,
            pos: 0,
            prev_significant: None,
        }
    }

    pub fn tokenize(&mut self) -> Result<Vec<Spanned>, LexError> {
        let mut tokens = Vec::new();
        loop {
            let spanned = self.next_token()?;
            let is_eof = spanned.token == Token::EOF;
            match &spanned.token {
                Token::Newline | Token::Comment(_) => {}
                _ => {
                    self.prev_significant = Some(spanned.token.clone());
                }
            }
            tokens.push(spanned);
            if is_eof {
                break;
            }
        }
        Ok(tokens)
    }

    fn peek_at(&self, offset: usize) -> Option<char> {
        self.chars.get(self.pos + offset).copied()
    }

    fn advance(&mut self) -> Option<char> {
        let ch = self.chars.get(self.pos).copied();
        if ch.is_some() {
            self.pos += 1;
        }
        ch
    }

    fn skip_whitespace(&mut self) {
        while self.pos < self.chars.len() {
            let ch = self.chars[self.pos];
            if ch == ' ' || ch == '\t' || ch == '\r' {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    fn is_regex_context(&self) -> bool {
        match &self.prev_significant {
            None => true,
            Some(t) => matches!(
                t,
                Token::Eq
                    | Token::LParen
                    | Token::Comma
                    | Token::LBracket
                    | Token::Semicolon
                    | Token::LBrace
            ),
        }
    }

    /// Convert a char index to a byte offset.
    fn byte_pos_of(&self, char_idx: usize) -> usize {
        self.byte_offsets[char_idx.min(self.chars.len())]
    }

    fn spanned(&self, token: Token, start: usize) -> Spanned {
        Spanned {
            token,
            span: Span {
                start: self.byte_pos_of(start),
                end: self.byte_pos_of(self.pos),
            },
        }
    }

    fn next_token(&mut self) -> Result<Spanned, LexError> {
        self.skip_whitespace();

        if self.pos >= self.chars.len() {
            return Ok(Spanned {
                token: Token::EOF,
                span: Span {
                    start: self.pos,
                    end: self.pos,
                },
            });
        }

        let start = self.pos;
        let ch = self.chars[self.pos];

        match ch {
            '\n' => {
                self.advance();
                Ok(self.spanned(Token::Newline, start))
            }
            '/' if self.peek_at(1) == Some('/') => self.lex_comment(start),
            '/' if self.is_regex_context() && self.peek_at(1).map_or(false, |c| c != ' ') => {
                self.lex_regex(start)
            }
            '/' => {
                self.advance();
                Ok(self.spanned(Token::Slash, start))
            }
            '*' => {
                self.advance();
                Ok(self.spanned(Token::Star, start))
            }
            '@' => {
                self.advance();
                Ok(self.spanned(Token::At, start))
            }
            '.' => {
                self.advance();
                Ok(self.spanned(Token::Dot, start))
            }
            ';' => {
                self.advance();
                Ok(self.spanned(Token::Semicolon, start))
            }
            ',' => {
                self.advance();
                Ok(self.spanned(Token::Comma, start))
            }
            '=' => {
                self.advance();
                Ok(self.spanned(Token::Eq, start))
            }
            '(' => {
                self.advance();
                Ok(self.spanned(Token::LParen, start))
            }
            ')' => {
                self.advance();
                Ok(self.spanned(Token::RParen, start))
            }
            '[' => {
                self.advance();
                Ok(self.spanned(Token::LBracket, start))
            }
            ']' => {
                self.advance();
                Ok(self.spanned(Token::RBracket, start))
            }
            '{' => {
                self.advance();
                Ok(self.spanned(Token::LBrace, start))
            }
            '}' => {
                self.advance();
                Ok(self.spanned(Token::RBrace, start))
            }
            '<' => {
                self.advance();
                Ok(self.spanned(Token::Lt, start))
            }
            '>' => {
                self.advance();
                Ok(self.spanned(Token::Gt, start))
            }
            ':' => {
                self.advance();
                Ok(self.spanned(Token::Colon, start))
            }
            '+' if self.peek_at(1) == Some('+') => {
                self.pos += 2;
                Ok(self.spanned(Token::PlusPlus, start))
            }
            '+' => {
                self.advance();
                Ok(self.spanned(Token::Plus, start))
            }
            '-' if self.peek_at(1) == Some('-') => {
                self.pos += 2;
                Ok(self.spanned(Token::MinusMinus, start))
            }
            '-' => {
                self.advance();
                Ok(self.spanned(Token::Minus, start))
            }
            '"' | '\'' => self.lex_string(start),
            c if c.is_ascii_digit() => self.lex_number(start),
            c if c.is_ascii_alphabetic() || c == '_' => self.lex_ident(start),
            _ => Err(LexError::UnexpectedChar { ch, pos: self.byte_pos_of(start) }),
        }
    }

    fn lex_comment(&mut self, start: usize) -> Result<Spanned, LexError> {
        self.pos += 2; // skip //
        let text_start = self.pos;
        while self.pos < self.chars.len() && self.chars[self.pos] != '\n' {
            self.pos += 1;
        }
        let text: String = self.chars[text_start..self.pos].iter().collect();
        Ok(self.spanned(Token::Comment(text.trim().to_string()), start))
    }

    fn lex_string(&mut self, start: usize) -> Result<Spanned, LexError> {
        let quote = self.advance().unwrap();
        let mut s = String::new();
        loop {
            match self.advance() {
                Some(c) if c == quote => break,
                Some('\\') => match self.advance() {
                    Some('n') => s.push('\n'),
                    Some('t') => s.push('\t'),
                    Some('\\') => s.push('\\'),
                    Some(c) => {
                        s.push('\\');
                        s.push(c);
                    }
                    None => return Err(LexError::UnterminatedString { pos: self.byte_pos_of(start) }),
                },
                Some(c) => s.push(c),
                None => return Err(LexError::UnterminatedString { pos: self.byte_pos_of(start) }),
            }
        }
        Ok(self.spanned(Token::StringLit(s), start))
    }

    fn lex_regex(&mut self, start: usize) -> Result<Spanned, LexError> {
        self.advance(); // consume opening /
        let mut pattern = String::new();
        loop {
            match self.advance() {
                Some('/') => break,
                Some('\\') => {
                    pattern.push('\\');
                    match self.advance() {
                        Some(c) => pattern.push(c),
                        None => return Err(LexError::UnterminatedRegex { pos: self.byte_pos_of(start) }),
                    }
                }
                Some(c) => pattern.push(c),
                None => return Err(LexError::UnterminatedRegex { pos: self.byte_pos_of(start) }),
            }
        }
        // Consume flags (e.g., /pattern/gi)
        let mut flags = String::new();
        while self.pos < self.chars.len() && self.chars[self.pos].is_ascii_alphabetic() {
            flags.push(self.advance().unwrap());
        }
        let regex = if flags.is_empty() {
            format!("/{pattern}/")
        } else {
            format!("/{pattern}/{flags}")
        };
        Ok(self.spanned(Token::RegexLit(regex), start))
    }

    fn lex_number(&mut self, start: usize) -> Result<Spanned, LexError> {
        while self.pos < self.chars.len() {
            let ch = self.chars[self.pos];
            if ch.is_ascii_digit() {
                self.pos += 1;
            } else if ch == '.' {
                // Only consume dot as decimal if followed by a digit
                if self.peek_at(1).map_or(false, |c| c.is_ascii_digit()) {
                    self.pos += 1; // consume the dot
                } else {
                    break;
                }
            } else {
                break;
            }
        }
        let text: String = self.chars[start..self.pos].iter().collect();
        let num: f64 = text.parse().map_err(|_| LexError::InvalidNumber {
            text: text.clone(),
            pos: self.byte_pos_of(start),
        })?;
        Ok(self.spanned(Token::Number(num), start))
    }

    fn lex_ident(&mut self, start: usize) -> Result<Spanned, LexError> {
        while self.pos < self.chars.len() {
            let ch = self.chars[self.pos];
            if ch.is_ascii_alphanumeric() || ch == '_' {
                self.pos += 1;
            } else {
                break;
            }
        }
        let text: String = self.chars[start..self.pos].iter().collect();
        let token = match text.as_str() {
            "track" => Token::Track,
            "const" => Token::Const,
            "let" => Token::Let,
            "for" => Token::For,
            _ => Token::Ident(text),
        };
        Ok(self.spanned(token, start))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lex(input: &str) -> Vec<Token> {
        Lexer::new(input)
            .tokenize()
            .unwrap()
            .into_iter()
            .map(|s| s.token)
            .filter(|t| !matches!(t, Token::EOF))
            .collect()
    }

    #[test]
    fn test_simple_tokens() {
        let tokens = lex("C3 /2");
        assert_eq!(tokens, vec![Token::Ident("C3".into()), Token::Slash, Token::Number(2.0)]);
    }

    #[test]
    fn test_modifiers() {
        let tokens = lex("C3*90@/4 /2");
        assert_eq!(
            tokens,
            vec![
                Token::Ident("C3".into()),
                Token::Star,
                Token::Number(90.0),
                Token::At,
                Token::Slash,
                Token::Number(4.0),
                Token::Slash,
                Token::Number(2.0),
            ]
        );
    }

    #[test]
    fn test_track_keyword() {
        let tokens = lex("track riff(inst) {");
        assert_eq!(
            tokens,
            vec![
                Token::Track,
                Token::Ident("riff".into()),
                Token::LParen,
                Token::Ident("inst".into()),
                Token::RParen,
                Token::LBrace,
            ]
        );
    }

    #[test]
    fn test_string_literal() {
        let tokens = lex(r#"const x = "hello";"#);
        assert_eq!(
            tokens,
            vec![
                Token::Const,
                Token::Ident("x".into()),
                Token::Eq,
                Token::StringLit("hello".into()),
                Token::Semicolon,
            ]
        );
    }

    #[test]
    fn test_regex_literal() {
        let tokens = lex(r#"const x = /FluidR3.*\/.*Guitar/i;"#);
        assert_eq!(
            tokens,
            vec![
                Token::Const,
                Token::Ident("x".into()),
                Token::Eq,
                Token::RegexLit(r"/FluidR3.*\/.*Guitar/i".into()),
                Token::Semicolon,
            ]
        );
    }

    #[test]
    fn test_comment() {
        let tokens = lex("// this is a comment\nC3");
        assert_eq!(
            tokens,
            vec![
                Token::Comment("this is a comment".into()),
                Token::Newline,
                Token::Ident("C3".into()),
            ]
        );
    }

    #[test]
    fn test_number_with_decimal() {
        let tokens = lex("0.4");
        assert_eq!(tokens, vec![Token::Number(0.4)]);
    }

    #[test]
    fn test_dot_not_consumed_as_decimal() {
        // `4.` should be Number(4) Dot, not Number(4.)
        let tokens = lex("4.");
        assert_eq!(tokens, vec![Token::Number(4.0), Token::Dot]);
    }

    #[test]
    fn test_for_loop_tokens() {
        let tokens = lex("for (let i=0; i<2; i++)");
        assert_eq!(
            tokens,
            vec![
                Token::For,
                Token::LParen,
                Token::Let,
                Token::Ident("i".into()),
                Token::Eq,
                Token::Number(0.0),
                Token::Semicolon,
                Token::Ident("i".into()),
                Token::Lt,
                Token::Number(2.0),
                Token::Semicolon,
                Token::Ident("i".into()),
                Token::PlusPlus,
                Token::RParen,
            ]
        );
    }
}
