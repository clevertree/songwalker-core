use crate::ast::*;
use crate::error::ParseError;
use crate::token::{token_to_string, Spanned, Token};

pub struct Parser {
    tokens: Vec<Spanned>,
    pos: usize,
}

impl Parser {
    pub fn new(tokens: Vec<Spanned>) -> Self {
        Parser { tokens, pos: 0 }
    }

    // ── Helpers ──────────────────────────────────────────────

    fn peek(&self) -> Token {
        self.tokens[self.pos].token.clone()
    }

    fn peek_at(&self, offset: usize) -> Token {
        let idx = self.pos + offset;
        if idx < self.tokens.len() {
            self.tokens[idx].token.clone()
        } else {
            Token::EOF
        }
    }

    fn span(&self) -> crate::token::Span {
        self.tokens[self.pos].span
    }

    fn advance(&mut self) -> Spanned {
        let s = self.tokens[self.pos].clone();
        self.pos += 1;
        s
    }

    fn is_at_end(&self) -> bool {
        matches!(self.peek(), Token::EOF)
    }

    fn check(&self, expected: &Token) -> bool {
        std::mem::discriminant(&self.tokens[self.pos].token) == std::mem::discriminant(expected)
    }

    fn eat(&mut self, expected: &Token) -> bool {
        if self.check(expected) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, expected: &Token) -> Result<Spanned, ParseError> {
        if self.check(expected) {
            Ok(self.advance())
        } else {
            Err(ParseError::UnexpectedToken {
                expected: format!("{expected:?}"),
                found: self.peek(),
                span: self.span(),
            })
        }
    }

    fn expect_ident(&mut self) -> Result<String, ParseError> {
        match self.peek() {
            Token::Ident(name) => {
                self.advance();
                Ok(name)
            }
            _ => Err(ParseError::UnexpectedToken {
                expected: "identifier".into(),
                found: self.peek(),
                span: self.span(),
            }),
        }
    }

    fn expect_number(&mut self) -> Result<f64, ParseError> {
        match self.peek() {
            Token::Number(n) => {
                self.advance();
                Ok(n)
            }
            _ => Err(ParseError::UnexpectedToken {
                expected: "number".into(),
                found: self.peek(),
                span: self.span(),
            }),
        }
    }

    /// Skip newlines and standalone comments (collecting comments into a vec).
    fn skip_newlines(&mut self) {
        while matches!(self.peek(), Token::Newline) {
            self.advance();
        }
    }

    /// Skip newlines and return any comments found.
    fn skip_newlines_collecting_comments(&mut self) -> Vec<String> {
        let mut comments = Vec::new();
        loop {
            match self.peek() {
                Token::Newline => {
                    self.advance();
                }
                Token::Comment(text) => {
                    comments.push(text);
                    self.advance();
                }
                _ => break,
            }
        }
        comments
    }

    /// Skip an optional semicolon and/or newlines.
    fn skip_terminator(&mut self) {
        self.eat(&Token::Semicolon);
        self.skip_newlines();
    }

    // ── Program ──────────────────────────────────────────────

    pub fn parse_program(&mut self) -> Result<Program, ParseError> {
        let mut statements = Vec::new();
        self.skip_newlines();

        while !self.is_at_end() {
            // Collect any comments as statements
            let comments = self.skip_newlines_collecting_comments();
            for c in comments {
                statements.push(Statement::Comment(c));
            }
            if self.is_at_end() {
                break;
            }
            statements.push(self.parse_statement()?);
            self.skip_terminator();
        }
        Ok(Program { statements })
    }

    // ── Top-Level Statement ─────────────────────────────────

    fn parse_statement(&mut self) -> Result<Statement, ParseError> {
        match self.peek() {
            Token::Comment(text) => {
                self.advance();
                Ok(Statement::Comment(text))
            }
            Token::Track => {
                // Distinguish `track name(...)` from `track.prop = ...`
                if self.peek_at(1) == Token::Dot {
                    self.parse_assignment_starting_with_track()
                } else {
                    self.parse_track_def()
                }
            }
            Token::Const => self.parse_const_decl(),
            Token::Ident(_) => self.parse_ident_statement(false),
            _ => Err(ParseError::UnexpectedToken {
                expected: "statement (track, const, identifier, or comment)".into(),
                found: self.peek(),
                span: self.span(),
            }),
        }
    }

    // ── Track Definition ────────────────────────────────────

    fn parse_track_def(&mut self) -> Result<Statement, ParseError> {
        let start_span = self.span().start;
        self.expect(&Token::Track)?;
        let name = self.expect_ident()?;
        self.expect(&Token::LParen)?;
        let params = self.parse_param_list()?;
        self.expect(&Token::RParen)?;
        self.expect(&Token::LBrace)?;
        let body = self.parse_track_body()?;
        self.expect(&Token::RBrace)?;
        let end_span = self.tokens[self.pos.saturating_sub(1)].span.end;
        Ok(Statement::TrackDef { name, params, body, span_start: start_span, span_end: end_span })
    }

    fn parse_param_list(&mut self) -> Result<Vec<String>, ParseError> {
        let mut params = Vec::new();
        if !self.check(&Token::RParen) {
            params.push(self.expect_ident()?);
            while self.eat(&Token::Comma) {
                params.push(self.expect_ident()?);
            }
        }
        Ok(params)
    }

    // ── Track Body ──────────────────────────────────────────

    fn parse_track_body(&mut self) -> Result<Vec<TrackStatement>, ParseError> {
        let mut stmts = Vec::new();
        self.skip_newlines();

        while !self.check(&Token::RBrace) && !self.is_at_end() {
            let comments = self.skip_newlines_collecting_comments();
            for c in comments {
                stmts.push(TrackStatement::Comment(c));
            }
            if self.check(&Token::RBrace) || self.is_at_end() {
                break;
            }
            stmts.push(self.parse_track_statement()?);
            // Consume optional semicolons and newlines between statements
            self.eat(&Token::Semicolon);
            self.skip_newlines();
        }
        Ok(stmts)
    }

    fn parse_track_statement(&mut self) -> Result<TrackStatement, ParseError> {
        match self.peek() {
            Token::Comment(text) => {
                self.advance();
                Ok(TrackStatement::Comment(text))
            }
            Token::LBracket => self.parse_chord(),
            Token::Number(_) => {
                // Standalone number = rest
                let start_span = self.span().start;
                let dur = self.parse_duration_expr()?;
                let end_span = self.tokens[self.pos.saturating_sub(1)].span.end;
                Ok(TrackStatement::Rest { duration: dur, span_start: start_span, span_end: end_span })
            }
            Token::Track => {
                // `track.property = value`
                self.parse_track_body_assignment()
            }
            Token::For => self.parse_for_loop(),
            Token::Ident(_) => self.parse_ident_statement_in_track(),
            Token::Dot => {
                // Dot shorthand as a rest: `.` or `..`
                let start_span = self.span().start;
                let dur = self.parse_duration_expr()?;
                let end_span = self.tokens[self.pos.saturating_sub(1)].span.end;
                Ok(TrackStatement::Rest { duration: dur, span_start: start_span, span_end: end_span })
            }
            _ => Err(ParseError::UnexpectedToken {
                expected: "track statement (note, chord, rest, assignment, or for loop)".into(),
                found: self.peek(),
                span: self.span(),
            }),
        }
    }

    // ── Ident-leading statement (note event or track call) ──

    fn parse_ident_statement(&mut self, _in_track: bool) -> Result<Statement, ParseError> {
        let start_span = self.span().start;
        let name = self.expect_ident()?;

        // Check for assignment: `name.prop = value` or `name = value`
        if self.check(&Token::Dot) {
            let target = self.parse_dotted_ident_rest(name)?;
            self.expect(&Token::Eq)?;
            let value = self.parse_expr()?;
            let end_span = self.tokens[self.pos.saturating_sub(1)].span.end;
            return Ok(Statement::Assignment { target, value, span_start: start_span, span_end: end_span });
        }
        if self.check(&Token::Eq) {
            self.advance();
            let value = self.parse_expr()?;
            let end_span = self.tokens[self.pos.saturating_sub(1)].span.end;
            return Ok(Statement::Assignment {
                target: name,
                value,
                span_start: start_span,
                span_end: end_span,
            });
        }

        // Parse optional modifiers: *vel @dur
        let (velocity, play_duration) = self.parse_modifiers()?;

        if self.check(&Token::LParen) {
            // Track call
            self.advance();
            let args = self.parse_call_args()?;
            self.expect(&Token::RParen)?;
            let step = self.try_parse_duration()?;
            let end_span = self.tokens[self.pos.saturating_sub(1)].span.end;
            Ok(Statement::TrackCall {
                name,
                velocity,
                play_duration,
                args,
                step,
                span_start: start_span,
                span_end: end_span,
            })
        } else {
            Err(ParseError::UnexpectedToken {
                expected: "( for track call, or = for assignment".into(),
                found: self.peek(),
                span: self.span(),
            })
        }
    }

    fn parse_ident_statement_in_track(&mut self) -> Result<TrackStatement, ParseError> {
        let start_span = self.span().start;
        let name = self.expect_ident()?;

        // Check for assignment: `name.prop = value` or `name = value`
        // Distinguish `name.prop` (property access) from `name .` (dot shorthand):
        // If Dot is followed by an Ident, it's property access.
        if self.check(&Token::Dot) && matches!(self.peek_at(1), Token::Ident(_)) {
            let target = self.parse_dotted_ident_rest(name)?;
            self.expect(&Token::Eq)?;
            let value = self.parse_expr()?;
            let end_span = self.tokens[self.pos.saturating_sub(1)].span.end;
            return Ok(TrackStatement::Assignment { target, value, span_start: start_span, span_end: end_span });
        }
        if self.check(&Token::Eq) {
            self.advance();
            let value = self.parse_expr()?;
            let end_span = self.tokens[self.pos.saturating_sub(1)].span.end;
            return Ok(TrackStatement::Assignment {
                target: name,
                value,
                span_start: start_span,
                span_end: end_span,
            });
        }

        // Parse optional modifiers: *vel @dur
        let (velocity, play_duration) = self.parse_modifiers()?;

        if self.check(&Token::LParen) {
            // Track call inside a track
            self.advance();
            let args = self.parse_call_args()?;
            self.expect(&Token::RParen)?;
            let step = self.try_parse_duration()?;
            let end_span = self.tokens[self.pos.saturating_sub(1)].span.end;
            Ok(TrackStatement::TrackCall {
                name,
                velocity,
                play_duration,
                args,
                step,
                span_start: start_span,
                span_end: end_span,
            })
        } else {
            // Note event: pitch was `name`, parse optional step duration
            let step = self.try_parse_duration()?;
            let end_span = self.tokens[self.pos.saturating_sub(1)].span.end;
            Ok(TrackStatement::NoteEvent {
                pitch: name,
                velocity,
                audible_duration: play_duration,
                step_duration: step,
                span_start: start_span,
                span_end: end_span,
            })
        }
    }

    fn parse_dotted_ident_rest(&mut self, first: String) -> Result<String, ParseError> {
        let mut result = first;
        while self.eat(&Token::Dot) {
            let part = self.expect_ident()?;
            result.push('.');
            result.push_str(&part);
        }
        Ok(result)
    }

    // ── Assignment starting with `track` keyword ────────────

    fn parse_assignment_starting_with_track(&mut self) -> Result<Statement, ParseError> {
        let start_span = self.span().start;
        self.expect(&Token::Track)?; // consume `track`
        let target = self.parse_dotted_ident_rest("track".to_string())?;
        self.expect(&Token::Eq)?;
        let value = self.parse_expr()?;
        let end_span = self.tokens[self.pos.saturating_sub(1)].span.end;
        Ok(Statement::Assignment { target, value, span_start: start_span, span_end: end_span })
    }

    fn parse_track_body_assignment(&mut self) -> Result<TrackStatement, ParseError> {
        let start_span = self.span().start;
        self.expect(&Token::Track)?;
        let target = self.parse_dotted_ident_rest("track".to_string())?;
        self.expect(&Token::Eq)?;
        let value = self.parse_expr()?;
        let end_span = self.tokens[self.pos.saturating_sub(1)].span.end;
        Ok(TrackStatement::Assignment { target, value, span_start: start_span, span_end: end_span })
    }

    // ── Const Declaration ───────────────────────────────────

    fn parse_const_decl(&mut self) -> Result<Statement, ParseError> {
        let start_span = self.span().start;
        self.expect(&Token::Const)?;
        let name = self.expect_ident()?;
        self.expect(&Token::Eq)?;
        let value = self.parse_expr()?;
        let end_span = self.tokens[self.pos.saturating_sub(1)].span.end;
        Ok(Statement::ConstDecl { name, value, span_start: start_span, span_end: end_span })
    }

    // ── Chord ───────────────────────────────────────────────

    fn parse_chord(&mut self) -> Result<TrackStatement, ParseError> {
        let start_span = self.span().start;
        self.expect(&Token::LBracket)?;
        let mut notes = Vec::new();
        if !self.check(&Token::RBracket) {
            notes.push(self.parse_chord_note()?);
            while self.eat(&Token::Comma) {
                self.skip_newlines();
                notes.push(self.parse_chord_note()?);
            }
        }
        self.expect(&Token::RBracket)?;

        // Parse optional modifiers on the whole chord
        let (_, audible_duration) = self.parse_modifiers()?;
        let step_duration = self.try_parse_duration()?;
        let end_span = self.tokens[self.pos.saturating_sub(1)].span.end;

        Ok(TrackStatement::Chord {
            notes,
            audible_duration,
            step_duration,
            span_start: start_span,
            span_end: end_span,
        })
    }

    fn parse_chord_note(&mut self) -> Result<ChordNote, ParseError> {
        let pitch = self.expect_ident()?;
        let audible_duration = if self.eat(&Token::At) {
            Some(self.parse_duration_expr()?)
        } else {
            None
        };
        Ok(ChordNote {
            pitch,
            audible_duration,
        })
    }

    // ── For Loop ────────────────────────────────────────────

    fn parse_for_loop(&mut self) -> Result<TrackStatement, ParseError> {
        let start_span = self.span().start;
        self.expect(&Token::For)?;
        self.expect(&Token::LParen)?;

        // Collect three parts separated by semicolons (as opaque strings)
        let init = self.collect_tokens_until(&Token::Semicolon)?;
        self.expect(&Token::Semicolon)?;
        let condition = self.collect_tokens_until(&Token::Semicolon)?;
        self.expect(&Token::Semicolon)?;
        let update = self.collect_tokens_until(&Token::RParen)?;
        self.expect(&Token::RParen)?;

        self.skip_newlines();
        self.expect(&Token::LBrace)?;
        let body = self.parse_track_body()?;
        self.expect(&Token::RBrace)?;
        let end_span = self.tokens[self.pos.saturating_sub(1)].span.end;

        Ok(TrackStatement::ForLoop {
            init,
            condition,
            update,
            body,
            span_start: start_span,
            span_end: end_span,
        })
    }

    fn collect_tokens_until(&mut self, sentinel: &Token) -> Result<String, ParseError> {
        let mut parts = Vec::new();
        while !self.check(sentinel) && !self.is_at_end() {
            let s = self.advance();
            parts.push(token_to_string(&s.token));
        }
        Ok(parts.join(" "))
    }

    // ── Modifiers ───────────────────────────────────────────

    /// Parse optional `*velocity` and `@duration` modifiers.
    fn parse_modifiers(&mut self) -> Result<(Option<f64>, Option<DurationExpr>), ParseError> {
        let velocity = if self.eat(&Token::Star) {
            Some(self.expect_number()?)
        } else {
            None
        };

        let duration = if self.eat(&Token::At) {
            // After @, parse a simple duration (no greedy fractions).
            // `@1/4` is uncommon; use `@/4` for inverse or `@4` for beats.
            Some(self.parse_simple_duration()?)
        } else {
            None
        };

        Ok((velocity, duration))
    }

    /// Parse a simple duration: `/N` or `N` (no fraction form).
    fn parse_simple_duration(&mut self) -> Result<DurationExpr, ParseError> {
        match self.peek() {
            Token::Slash => {
                self.advance();
                let n = self.expect_number()?;
                Ok(DurationExpr::Inverse(n))
            }
            Token::Number(n) => {
                self.advance();
                Ok(DurationExpr::Beats(n))
            }
            Token::Dot => {
                let mut count = 0;
                while self.eat(&Token::Dot) {
                    count += 1;
                }
                Ok(DurationExpr::Dots(count))
            }
            _ => Err(ParseError::UnexpectedToken {
                expected: "duration after @".into(),
                found: self.peek(),
                span: self.span(),
            }),
        }
    }

    // ── Duration Expressions ────────────────────────────────

    /// Try to parse an optional duration expression (step duration).
    fn try_parse_duration(&mut self) -> Result<Option<DurationExpr>, ParseError> {
        match self.peek() {
            Token::Slash | Token::Number(_) | Token::Dot => {
                Ok(Some(self.parse_duration_expr()?))
            }
            _ => Ok(None),
        }
    }

    /// Parse a duration expression: `/N`, `N/M`, `N`, or dots.
    fn parse_duration_expr(&mut self) -> Result<DurationExpr, ParseError> {
        match self.peek() {
            Token::Slash => {
                self.advance();
                let n = self.expect_number()?;
                Ok(DurationExpr::Inverse(n))
            }
            Token::Number(n) => {
                self.advance();
                // Check for fraction: N/M
                if self.check(&Token::Slash) {
                    let saved = self.pos;
                    self.advance(); // consume /
                    if let Token::Number(m) = self.peek() {
                        self.advance();
                        Ok(DurationExpr::Fraction(n, m))
                    } else {
                        // Not a fraction, backtrack. The `/` belongs to something else.
                        self.pos = saved;
                        Ok(DurationExpr::Beats(n))
                    }
                } else {
                    Ok(DurationExpr::Beats(n))
                }
            }
            Token::Dot => {
                let mut count = 0;
                while self.eat(&Token::Dot) {
                    count += 1;
                }
                Ok(DurationExpr::Dots(count))
            }
            _ => Err(ParseError::UnexpectedToken {
                expected: "duration expression (/, number, or .)".into(),
                found: self.peek(),
                span: self.span(),
            }),
        }
    }

    // ── Expressions ─────────────────────────────────────────

    fn parse_call_args(&mut self) -> Result<Vec<Expr>, ParseError> {
        let mut args = Vec::new();
        if !self.check(&Token::RParen) {
            args.push(self.parse_expr()?);
            while self.eat(&Token::Comma) {
                args.push(self.parse_expr()?);
            }
        }
        Ok(args)
    }

    fn parse_expr(&mut self) -> Result<Expr, ParseError> {
        match self.peek() {
            Token::Number(n) => {
                self.advance();
                // Check for fraction
                if self.check(&Token::Slash) {
                    let saved = self.pos;
                    self.advance();
                    if let Token::Number(m) = self.peek() {
                        self.advance();
                        Ok(Expr::DurationLit(DurationExpr::Fraction(n, m)))
                    } else {
                        self.pos = saved;
                        Ok(Expr::Number(n))
                    }
                } else {
                    Ok(Expr::Number(n))
                }
            }
            Token::StringLit(s) => {
                self.advance();
                Ok(Expr::StringLit(s))
            }
            Token::RegexLit(s) => {
                self.advance();
                Ok(Expr::RegexLit(s))
            }
            Token::Ident(name) => {
                self.advance();
                if self.check(&Token::LParen) {
                    // Function call: Name(args)
                    self.advance(); // consume (
                    let args = self.parse_call_args()?;
                    self.expect(&Token::RParen)?;
                    Ok(Expr::FunctionCall {
                        function: name,
                        args,
                    })
                } else if self.check(&Token::Dot) {
                    let target = self.parse_dotted_ident_rest(name.clone())?;
                    Ok(Expr::PropertyAccess {
                        object: name,
                        property: target,
                    })
                } else {
                    Ok(Expr::Identifier(name))
                }
            }
            Token::LBracket => self.parse_array_expr(),
            Token::LBrace => self.parse_object_expr(),
            _ => Err(ParseError::UnexpectedToken {
                expected: "expression".into(),
                found: self.peek(),
                span: self.span(),
            }),
        }
    }

    fn parse_array_expr(&mut self) -> Result<Expr, ParseError> {
        self.expect(&Token::LBracket)?;
        let mut items = Vec::new();
        if !self.check(&Token::RBracket) {
            items.push(self.parse_expr()?);
            while self.eat(&Token::Comma) {
                items.push(self.parse_expr()?);
            }
        }
        self.expect(&Token::RBracket)?;
        Ok(Expr::Array(items))
    }

    fn parse_object_expr(&mut self) -> Result<Expr, ParseError> {
        self.expect(&Token::LBrace)?;
        let mut props = Vec::new();
        if !self.check(&Token::RBrace) {
            props.push(self.parse_obj_prop()?);
            while self.eat(&Token::Comma) {
                if self.check(&Token::RBrace) {
                    break; // trailing comma
                }
                props.push(self.parse_obj_prop()?);
            }
        }
        self.expect(&Token::RBrace)?;
        Ok(Expr::ObjectLit(props))
    }

    fn parse_obj_prop(&mut self) -> Result<(String, Expr), ParseError> {
        let key = match self.peek() {
            Token::Ident(s) | Token::StringLit(s) => {
                self.advance();
                s
            }
            _ => {
                return Err(ParseError::UnexpectedToken {
                    expected: "property name (identifier or string)".into(),
                    found: self.peek(),
                    span: self.span(),
                })
            }
        };
        self.expect(&Token::Colon)?;
        let value = self.parse_expr()?;
        Ok((key, value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;

    fn parse(input: &str) -> Result<Program, Box<dyn std::error::Error>> {
        let tokens = Lexer::new(input).tokenize()?;
        let mut parser = Parser::new(tokens);
        Ok(parser.parse_program()?)
    }

    #[test]
    fn test_parse_simple_track_def() {
        let program = parse(
            r#"
track riff(inst) {
    C3 /2
    Eb3 /4
}
"#,
        )
        .unwrap();

        assert_eq!(program.statements.len(), 1);
        match &program.statements[0] {
            Statement::TrackDef { name, params, body, .. } => {
                assert_eq!(name, "riff");
                assert_eq!(params, &["inst"]);
                // Filter out comments
                let notes: Vec<_> = body
                    .iter()
                    .filter(|s| matches!(s, TrackStatement::NoteEvent { .. }))
                    .collect();
                assert_eq!(notes.len(), 2);
            }
            other => panic!("Expected TrackDef, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_note_with_modifiers() {
        let program = parse(
            r#"
track t() {
    C2*90@/4 /2
}
"#,
        )
        .unwrap();

        match &program.statements[0] {
            Statement::TrackDef { body, .. } => match &body[0] {
                TrackStatement::NoteEvent {
                    pitch,
                    velocity,
                    audible_duration,
                    step_duration,
                    ..
                } => {
                    assert_eq!(pitch, "C2");
                    assert_eq!(*velocity, Some(90.0));
                    assert_eq!(*audible_duration, Some(DurationExpr::Inverse(4.0)));
                    assert_eq!(*step_duration, Some(DurationExpr::Inverse(2.0)));
                }
                other => panic!("Expected NoteEvent, got {other:?}"),
            },
            other => panic!("Expected TrackDef, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_track_call() {
        let program = parse("riff(lead);").unwrap();
        match &program.statements[0] {
            Statement::TrackCall { name, args, .. } => {
                assert_eq!(name, "riff");
                assert_eq!(args.len(), 1);
            }
            other => panic!("Expected TrackCall, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_track_call_with_modifiers_and_step() {
        let program = parse("drums*96@4(osc) 8;").unwrap();
        match &program.statements[0] {
            Statement::TrackCall {
                name,
                velocity,
                play_duration,
                args,
                step,
                ..
            } => {
                assert_eq!(name, "drums");
                assert_eq!(*velocity, Some(96.0));
                assert_eq!(*play_duration, Some(DurationExpr::Beats(4.0)));
                assert_eq!(args.len(), 1);
                assert_eq!(*step, Some(DurationExpr::Beats(8.0)));
            }
            other => panic!("Expected TrackCall, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_const_decl() {
        let program = parse(r#"const lead = loadPreset("Guitar");"#).unwrap();
        match &program.statements[0] {
            Statement::ConstDecl { name, value, .. } => {
                assert_eq!(name, "lead");
                match value {
                    Expr::FunctionCall { function, args } => {
                        assert_eq!(function, "loadPreset");
                        assert_eq!(args.len(), 1);
                    }
                    other => panic!("Expected FunctionCall, got {other:?}"),
                }
            }
            other => panic!("Expected ConstDecl, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_assignment() {
        let program = parse("track.beatsPerMinute = 160;").unwrap();
        match &program.statements[0] {
            Statement::Assignment { target, value, .. } => {
                assert_eq!(target, "track.beatsPerMinute");
                match value {
                    Expr::Number(n) => assert_eq!(*n, 160.0),
                    other => panic!("Expected Number, got {other:?}"),
                }
            }
            other => panic!("Expected Assignment, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_chord() {
        let program = parse(
            r#"
track t() {
    [C3@2, E3, G3]@1 /2
}
"#,
        )
        .unwrap();

        match &program.statements[0] {
            Statement::TrackDef { body, .. } => match &body[0] {
                TrackStatement::Chord {
                    notes,
                    audible_duration,
                    step_duration,
                    ..
                } => {
                    assert_eq!(notes.len(), 3);
                    assert_eq!(notes[0].pitch, "C3");
                    assert_eq!(notes[0].audible_duration, Some(DurationExpr::Beats(2.0)));
                    assert_eq!(notes[1].pitch, "E3");
                    assert_eq!(notes[1].audible_duration, None);
                    assert_eq!(*audible_duration, Some(DurationExpr::Beats(1.0)));
                    assert_eq!(*step_duration, Some(DurationExpr::Inverse(2.0)));
                }
                other => panic!("Expected Chord, got {other:?}"),
            },
            other => panic!("Expected TrackDef, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_rest() {
        let program = parse(
            r#"
track t() {
    C3 /4
    4
    D3 /4
}
"#,
        )
        .unwrap();

        match &program.statements[0] {
            Statement::TrackDef { body, .. } => {
                assert!(matches!(&body[0], TrackStatement::NoteEvent { pitch, .. } if pitch == "C3"));
                assert!(matches!(&body[1], TrackStatement::Rest { duration: DurationExpr::Beats(n), .. } if *n == 4.0));
                assert!(matches!(&body[2], TrackStatement::NoteEvent { pitch, .. } if pitch == "D3"));
            }
            other => panic!("Expected TrackDef, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_for_loop() {
        let program = parse(
            r#"
track t() {
    for (let i = 0; i < 2; i ++) {
        Eb3 /8
        F3 /8
    }
}
"#,
        )
        .unwrap();

        match &program.statements[0] {
            Statement::TrackDef { body, .. } => match &body[0] {
                TrackStatement::ForLoop {
                    init,
                    condition,
                    update,
                    body,
                    ..
                } => {
                    assert!(init.contains("let"));
                    assert!(condition.contains("<"));
                    assert!(update.contains("++"));
                    let notes: Vec<_> = body
                        .iter()
                        .filter(|s| matches!(s, TrackStatement::NoteEvent { .. }))
                        .collect();
                    assert_eq!(notes.len(), 2);
                }
                other => panic!("Expected ForLoop, got {other:?}"),
            },
            other => panic!("Expected TrackDef, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_fraction_duration() {
        let program = parse(
            r#"
track t() {
    track.duration = 1/4;
}
"#,
        )
        .unwrap();

        match &program.statements[0] {
            Statement::TrackDef { body, .. } => match &body[0] {
                TrackStatement::Assignment { target, value, .. } => {
                    assert_eq!(target, "track.duration");
                    match value {
                        Expr::DurationLit(DurationExpr::Fraction(n, m)) => {
                            assert_eq!(*n, 1.0);
                            assert_eq!(*m, 4.0);
                        }
                        other => panic!("Expected Fraction, got {other:?}"),
                    }
                }
                other => panic!("Expected Assignment, got {other:?}"),
            },
            other => panic!("Expected TrackDef, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_dot_shorthand() {
        let program = parse(
            r#"
track t() {
    C3 .
    D3 ..
}
"#,
        )
        .unwrap();

        match &program.statements[0] {
            Statement::TrackDef { body, .. } => {
                match &body[0] {
                    TrackStatement::NoteEvent { step_duration, .. } => {
                        assert_eq!(*step_duration, Some(DurationExpr::Dots(1)));
                    }
                    other => panic!("Expected NoteEvent, got {other:?}"),
                }
                match &body[1] {
                    TrackStatement::NoteEvent { step_duration, .. } => {
                        assert_eq!(*step_duration, Some(DurationExpr::Dots(2)));
                    }
                    other => panic!("Expected NoteEvent, got {other:?}"),
                }
            }
            other => panic!("Expected TrackDef, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_full_program() {
        let input = r#"
const lead = loadPreset("Guitar");
track.beatsPerMinute = 160;

riff(lead);
drums*96@4() 8;

track riff(instrument) {
    track.instrument = instrument;
    track.duration = 1/4;
    C3 /2
    C2*90@/4 /2
    [C3@2, E3, G3]@1 /2
    for (let i = 0; i < 2; i ++) {
        Eb3 /8
    }
}
"#;
        let program = parse(input).unwrap();
        // ConstDecl, Assignment, TrackCall, TrackCall, TrackDef
        let non_comment: Vec<_> = program
            .statements
            .iter()
            .filter(|s| !matches!(s, Statement::Comment(_)))
            .collect();
        assert_eq!(non_comment.len(), 5);
    }
}
