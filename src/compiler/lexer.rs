//! Zero-allocation streaming ASCII lexer.
//!
//! Tokens borrow slices of the caller's input buffer — nothing is copied,
//! nothing is heap-allocated. (The reference doc sketched
//! `Identifier(&'static str)`, which is impossible for live REPL input;
//! borrowed slices give identical zero-copy semantics with honest
//! lifetimes.)

/// A single lexical token. `Identifier` borrows from the input stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Token<'a> {
    /// `fn`
    KwFn,
    /// `let`
    KwLet,
    /// Identifier slice into the input buffer.
    Identifier(&'a [u8]),
    /// `+ - * / % = < > ! & | ^ ~ : ?`
    Operator(u8),
    /// Numeric literal (decimal or 0x hex).
    Literal(u32),
    LParen,
    RParen,
    LBrace,
    RBrace,
    Semicolon,
    Comma,
    /// Lexical error with an interned message (.rodata).
    Error(&'static str),
    /// End of input.
    Eof,
}

/// Streaming lexer over a fixed byte buffer.
pub struct Lexer<'a> {
    stream: &'a [u8],
    cursor: usize,
    line: usize,
}

impl<'a> Lexer<'a> {
    /// New lexer over `stream`.
    pub fn new(stream: &'a [u8]) -> Self {
        Lexer {
            stream,
            cursor: 0,
            line: 1,
        }
    }

    /// Current line counter (error reporting).
    pub fn line(&self) -> usize {
        self.line
    }

    /// Byte offset of the cursor (error reporting).
    pub fn pos(&self) -> usize {
        self.cursor
    }

    /// Produce the next token, advancing the cursor. Single pass, no
    /// allocation, no backtracking beyond one lookahead byte.
    pub fn next_token(&mut self) -> Token<'a> {
        while self.cursor < self.stream.len() {
            let b = self.stream[self.cursor];
            match b {
                b' ' | b'\t' | b'\r' => self.cursor += 1,
                b'\n' => {
                    self.line += 1;
                    self.cursor += 1;
                }
                b'a'..=b'z' | b'A'..=b'Z' | b'_' => return self.parse_identifier(),
                b'0'..=b'9' => return self.parse_literal(),
                b'(' => {
                    self.cursor += 1;
                    return Token::LParen;
                }
                b')' => {
                    self.cursor += 1;
                    return Token::RParen;
                }
                b'{' => {
                    self.cursor += 1;
                    return Token::LBrace;
                }
                b'}' => {
                    self.cursor += 1;
                    return Token::RBrace;
                }
                b';' => {
                    self.cursor += 1;
                    return Token::Semicolon;
                }
                b',' => {
                    self.cursor += 1;
                    return Token::Comma;
                }
                b'+' | b'-' | b'*' | b'/' | b'%' | b'=' | b'<' | b'>' | b'!' | b'&' | b'|'
                | b'^' | b'~' | b':' | b'?' => {
                    self.cursor += 1;
                    return Token::Operator(b);
                }
                _ => {
                    self.cursor += 1;
                    return Token::Error("unexpected character");
                }
            }
        }
        Token::Eof
    }

    fn parse_identifier(&mut self) -> Token<'a> {
        let start = self.cursor;
        while self.cursor < self.stream.len() {
            let b = self.stream[self.cursor];
            if b.is_ascii_alphanumeric() || b == b'_' {
                self.cursor += 1;
            } else {
                break;
            }
        }
        let ident = &self.stream[start..self.cursor];
        match ident {
            b"fn" => Token::KwFn,
            b"let" => Token::KwLet,
            _ => Token::Identifier(ident),
        }
    }

    fn parse_literal(&mut self) -> Token<'a> {
        let start = self.cursor;

        // Hex form: 0x / 0X prefix.
        if self.stream[self.cursor] == b'0'
            && self.cursor + 1 < self.stream.len()
            && (self.stream[self.cursor + 1] == b'x' || self.stream[self.cursor + 1] == b'X')
        {
            self.cursor += 2;
            let digits_start = self.cursor;
            let mut value: u32 = 0;
            while self.cursor < self.stream.len() {
                let b = self.stream[self.cursor];
                let digit = match b {
                    b'0'..=b'9' => b - b'0',
                    b'a'..=b'f' => b - b'a' + 10,
                    b'A'..=b'F' => b - b'A' + 10,
                    b'_' => {
                        self.cursor += 1;
                        continue;
                    }
                    _ => break,
                };
                // Overflow check before shifting in the new nibble.
                match value
                    .checked_mul(16)
                    .and_then(|v| v.checked_add(digit as u32))
                {
                    Some(v) => value = v,
                    None => {
                        // Consume the rest of the token so errors don't cascade.
                        while self.cursor < self.stream.len()
                            && (self.stream[self.cursor].is_ascii_hexdigit()
                                || self.stream[self.cursor] == b'_')
                        {
                            self.cursor += 1;
                        }
                        return Token::Error("hex literal overflow");
                    }
                }
                self.cursor += 1;
            }
            if self.cursor == digits_start {
                return Token::Error("malformed hex literal");
            }
            return Token::Literal(value);
        }

        // Decimal form.
        let mut value: u32 = 0;
        while self.cursor < self.stream.len() {
            let b = self.stream[self.cursor];
            if b.is_ascii_digit() {
                match value
                    .checked_mul(10)
                    .and_then(|v| v.checked_add((b - b'0') as u32))
                {
                    Some(v) => value = v,
                    None => {
                        while self.cursor < self.stream.len()
                            && (self.stream[self.cursor].is_ascii_digit()
                                || self.stream[self.cursor] == b'_')
                        {
                            self.cursor += 1;
                        }
                        return Token::Error("literal overflow");
                    }
                }
                self.cursor += 1;
            } else if b == b'_' {
                self.cursor += 1;
            } else {
                break;
            }
        }
        debug_assert!(self.cursor > start);
        Token::Literal(value)
    }
}
