//! Single-pass streaming parser / compiler front end.
//!
//! LL(1), left-to-right evaluation, no precedence levels — exactly the
//! grammar constraints from the manifesto. Two evaluation modes share the
//! same term parsing:
//!
//! - **fold**: constant-fold immediately (REPL prints the value; division
//!   by zero is caught at parse time).
//! - **emit**: append threaded micro-primitive tokens to the stream buffer
//!   for SRAM execution.
//!
//! The [`Compiler`] owns persistent state (symbols + fn bodies) because fn
//! definitions must survive across REPL lines. It lives in a `static mut`
//! owned by the REPL driver — sound under the single-threaded Ring 0 REPL
//! contract.

use crate::compiler::lexer::{Lexer, Token};
use crate::compiler::primitives;

/// Maximum words in one compiled token stream.
pub const MAX_STREAM_WORDS: usize = 128;
/// Symbol table slots (open addressing).
pub const SYMBOL_SLOTS: usize = 32;
/// Longest identifier accepted.
pub const NAME_MAX: usize = 16;
/// Maximum simultaneously defined functions.
pub const MAX_FNS: usize = 2;
/// Words per function body (excluding the appended halt).
pub const FN_BODY_WORDS: usize = 32;

/// Fixed-capacity identifier storage (zero allocation).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NameBuf {
    pub len: u8,
    pub bytes: [u8; NAME_MAX],
}

impl NameBuf {
    fn from_slice(name: &[u8]) -> Result<Self, ParseError> {
        if name.len() > NAME_MAX {
            return Err(ParseError::NameTooLong);
        }
        let mut bytes = [0u8; NAME_MAX];
        bytes[..name.len()].copy_from_slice(name);
        Ok(NameBuf {
            len: name.len() as u8,
            bytes,
        })
    }

    /// Borrow the name bytes.
    pub fn as_slice(&self) -> &[u8] {
        &self.bytes[..self.len as usize]
    }

    fn eq_bytes(&self, other: &[u8]) -> bool {
        self.as_slice() == other
    }
}

/// Parser failure modes (interned messages, no allocation).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseError {
    LexError(&'static str),
    UnexpectedToken,
    UnsupportedOperator(u8),
    UnknownSymbol,
    DuplicateFn,
    SymbolTableFull,
    FnTableFull,
    StreamFull,
    NameTooLong,
    DivByZero,
    MissingSemicolon,
    EmptyLine,
    /// Mandatory capability enforcement: peripheral access denied (doc ch.2).
    CapabilityViolation,
}

/// A compiled threaded token stream ready for SRAM dispatch.
#[derive(Debug, Clone, Copy)]
pub struct StreamProgram {
    words: [usize; MAX_STREAM_WORDS],
    len: usize,
    yields_value: bool,
}

impl StreamProgram {
    /// Execute against the kernel's threaded dispatch engine.
    ///
    /// Tries the native codegen path first (Milestone 4) — compiles the
    /// threaded stream into real machine code and executes via
    /// `exec_buffer_entry`. Falls back to the threaded interpreter for
    /// streams that exceed the two-register compiler's capacity.
    pub fn run(&self) -> Option<u32> {
        unsafe {
            crate::kernel::exec::vm_reset();
            if self.len > 0 {
                // Try native path first.
                if let Ok(result) = crate::compiler::native::compile_and_run(
                    &self.words,
                    self.len,
                    self.yields_value,
                ) {
                    return result;
                }
                // Fall back to threaded interpreter.
                crate::kernel::exec::run_threaded_stream(self.words.as_ptr());
            }
            if self.yields_value {
                Some(crate::kernel::exec::vm_pop() as u32)
            } else {
                None
            }
        }
    }

    /// Word count (diagnostics).
    pub fn len(&self) -> usize {
        self.len
    }

    /// True when the stream holds no words.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

/// What the parser decided the input line means.
///
/// `Run` carries a fixed-capacity inline token stream (no heap), so it
/// dwarfs the other variants by design; the REPL consumes outcomes
/// immediately and never stores them in bulk.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, Copy)]
pub enum Outcome {
    /// Blank input.
    Empty,
    /// `let NAME = expr;` bound.
    Bound { name: NameBuf, value: u32 },
    /// `fn NAME() { ... }` stored.
    FnDefined { name: NameBuf },
    /// Executable program (peek/poke/expression/call).
    Run(StreamProgram),
    /// `cap_claim NAME;`
    Claim(NameBuf),
    /// `cap_drop NAME;`
    Drop(NameBuf),
    /// `reg_set_bit ADDR BIT;`
    SetBit { addr: u32, bit: u32 },
    /// `reg_clr_bit ADDR BIT;`
    ClrBit { addr: u32, bit: u32 },
    /// `help`
    Help,
    /// `banner`
    Banner,
    /// `sys_audit` — dump the SuperUser audit log.
    SysAudit,
    /// Capability-enforced poke (direct, not via stream).
    EnforcedPoke { addr: u32, val: u32 },
    /// Capability-enforced peek (direct, not via stream).
    EnforcedPeek { addr: u32 },
}

struct Symbol {
    used: bool,
    name_len: u8,
    name: [u8; NAME_MAX],
    value: u32,
}

const EMPTY_SYMBOL: Symbol = Symbol {
    used: false,
    name_len: 0,
    name: [0; NAME_MAX],
    value: 0,
};

/// Persistent compiler state: symbol table, function bodies, stream buffer.
pub struct Compiler {
    symbols: [Symbol; SYMBOL_SLOTS],
    fn_names: [NameBuf; MAX_FNS],
    fn_bodies: [[usize; FN_BODY_WORDS]; MAX_FNS],
    fn_body_lens: [usize; MAX_FNS],
    stream: [usize; MAX_STREAM_WORDS],
    stream_len: usize,
}

impl Default for Compiler {
    fn default() -> Self {
        Self::new()
    }
}

impl Compiler {
    /// Const-constructible so the REPL can own it as a static.
    pub const fn new() -> Self {
        Compiler {
            symbols: [EMPTY_SYMBOL; SYMBOL_SLOTS],
            fn_names: [NameBuf {
                len: 0,
                bytes: [0; NAME_MAX],
            }; MAX_FNS],
            fn_bodies: [[0; FN_BODY_WORDS]; MAX_FNS],
            fn_body_lens: [0; MAX_FNS],
            stream: [0; MAX_STREAM_WORDS],
            stream_len: 0,
        }
    }

    /// Compile one REPL line.
    pub fn parse(&mut self, line: &[u8]) -> Result<Outcome, ParseError> {
        let mut cur = Cur {
            lx: Lexer::new(line),
            ahead: None,
        };
        let first = cur.next();
        match first {
            Token::Eof => Err(ParseError::EmptyLine),
            Token::KwLet => self.parse_let(&mut cur),
            Token::KwFn => self.parse_fn(&mut cur),
            Token::Identifier(id) => self.parse_command(id, &mut cur),
            Token::Literal(_) | Token::Operator(_) | Token::LParen => {
                // Bare expression statement: evaluate and print.
                let value = self.eval_expr(Some(first), &mut cur)?;
                self.expect_semicolon(&mut cur)?;
                Ok(Outcome::Run(self.build_value_program(value)?))
            }
            Token::Error(msg) => Err(ParseError::LexError(msg)),
            _ => Err(ParseError::UnexpectedToken),
        }
    }

    // -- statement parsers --------------------------------------------------

    fn parse_let(&mut self, cur: &mut Cur) -> Result<Outcome, ParseError> {
        let name_tok = cur.next();
        let name = match name_tok {
            Token::Identifier(id) => NameBuf::from_slice(id)?,
            _ => return Err(ParseError::UnexpectedToken),
        };
        match cur.next() {
            Token::Operator(b'=') => {}
            _ => return Err(ParseError::UnexpectedToken),
        }
        let value = self.eval_expr(None, cur)?;
        self.expect_semicolon(cur)?;
        self.insert_symbol(&name, value)?;
        Ok(Outcome::Bound { name, value })
    }

    fn parse_fn(&mut self, cur: &mut Cur) -> Result<Outcome, ParseError> {
        let name = match cur.next() {
            Token::Identifier(id) => NameBuf::from_slice(id)?,
            _ => return Err(ParseError::UnexpectedToken),
        };
        match cur.next() {
            Token::LParen => {}
            _ => return Err(ParseError::UnexpectedToken),
        }
        match cur.next() {
            Token::RParen => {}
            _ => return Err(ParseError::UnexpectedToken),
        }
        match cur.next() {
            Token::LBrace => {}
            _ => return Err(ParseError::UnexpectedToken),
        }

        // Compile the body into the scratch stream. Bodies are stored
        // WITHOUT a trailing halt so they can be spliced at call sites.
        self.stream_len = 0;
        loop {
            match cur.peek() {
                Token::RBrace => {
                    cur.next();
                    break;
                }
                Token::Eof => return Err(ParseError::UnexpectedToken),
                _ => self.parse_body_stmt(cur)?,
            }
        }

        if self.stream_len > FN_BODY_WORDS {
            return Err(ParseError::StreamFull);
        }
        let index = self.alloc_fn_slot(&name)?;
        self.fn_names[index] = name;
        self.fn_body_lens[index] = self.stream_len;
        self.fn_bodies[index][..self.stream_len].copy_from_slice(&self.stream[..self.stream_len]);
        Ok(Outcome::FnDefined { name })
    }

    /// One statement inside a fn body (no nested fns, no let).
    fn parse_body_stmt(&mut self, cur: &mut Cur) -> Result<(), ParseError> {
        match cur.next() {
            Token::Identifier(id) => match id {
                b"poke" => {
                    let addr = self.parse_atomic_term(cur)?;
                    let val = self.eval_expr(None, cur)?;
                    self.expect_semicolon(cur)?;
                    // Definition-time capability enforcement (doc ch.2 Q3).
                    crate::capabilities::registry::check_access(addr)
                        .map_err(|_| ParseError::CapabilityViolation)?;
                    self.stream_push_lit(addr)?;
                    self.stream_push_lit(val)?;
                    self.stream_push(word_of(primitives::write_reg_prim))?;
                    Ok(())
                }
                b"peek" => {
                    let addr = self.parse_atomic_term(cur)?;
                    self.expect_semicolon(cur)?;
                    // Definition-time capability enforcement (doc ch.2 Q3).
                    crate::capabilities::registry::check_access(addr)
                        .map_err(|_| ParseError::CapabilityViolation)?;
                    self.stream_push_lit(addr)?;
                    self.stream_push(word_of(primitives::load_reg_prim))?;
                    // Body peeks leave their value on the VM stack; the
                    // caller's halt bounds the stream either way.
                    Ok(())
                }
                other => {
                    // Call or bare expression.
                    if matches!(cur.peek(), Token::LParen) {
                        cur.next(); // '('
                        match cur.next() {
                            Token::RParen => {}
                            _ => return Err(ParseError::UnexpectedToken),
                        }
                        self.expect_semicolon(cur)?;
                        self.splice_call(other)?;
                        Ok(())
                    } else {
                        let _ = self.eval_expr(Some(Token::Identifier(other)), cur)?;
                        self.expect_semicolon(cur)?;
                        Ok(())
                    }
                }
            },
            _ => Err(ParseError::UnexpectedToken),
        }
    }

    fn parse_command(&mut self, id: &[u8], cur: &mut Cur) -> Result<Outcome, ParseError> {
        match id {
            b"peek" => {
                let addr = self.parse_atomic_term(cur)?;
                self.expect_semicolon(cur)?;
                // Mandatory capability enforcement (doc ch.2).
                crate::capabilities::registry::check_access(addr)
                    .map_err(|_| ParseError::CapabilityViolation)?;
                Ok(Outcome::EnforcedPeek { addr })
            }
            b"poke" => {
                let addr = self.parse_atomic_term(cur)?;
                let val = self.eval_expr(None, cur)?;
                self.expect_semicolon(cur)?;
                // Mandatory capability enforcement (doc ch.2).
                crate::capabilities::registry::check_access(addr)
                    .map_err(|_| ParseError::CapabilityViolation)?;
                Ok(Outcome::EnforcedPoke { addr, val })
            }
            b"cap_claim" => {
                let name = self.expect_name(cur)?;
                self.expect_semicolon(cur)?;
                Ok(Outcome::Claim(name))
            }
            b"cap_drop" => {
                let name = self.expect_name(cur)?;
                self.expect_semicolon(cur)?;
                Ok(Outcome::Drop(name))
            }
            b"reg_set_bit" => {
                let addr = self.parse_atomic_term(cur)?;
                let bit = self.parse_atomic_term(cur)?;
                self.expect_semicolon(cur)?;
                // Mandatory capability enforcement (doc ch.2).
                crate::capabilities::registry::check_access(addr)
                    .map_err(|_| ParseError::CapabilityViolation)?;
                Ok(Outcome::SetBit { addr, bit })
            }
            b"reg_clr_bit" => {
                let addr = self.parse_atomic_term(cur)?;
                let bit = self.parse_atomic_term(cur)?;
                self.expect_semicolon(cur)?;
                // Mandatory capability enforcement (doc ch.2).
                crate::capabilities::registry::check_access(addr)
                    .map_err(|_| ParseError::CapabilityViolation)?;
                Ok(Outcome::ClrBit { addr, bit })
            }
            b"help" => {
                self.allow_optional_semicolon(cur);
                Ok(Outcome::Help)
            }
            b"banner" => {
                self.allow_optional_semicolon(cur);
                Ok(Outcome::Banner)
            }
            b"sys_audit" => {
                self.allow_optional_semicolon(cur);
                Ok(Outcome::SysAudit)
            }
            other => {
                if matches!(cur.peek(), Token::LParen) {
                    cur.next(); // '('
                    match cur.next() {
                        Token::RParen => {}
                        _ => return Err(ParseError::UnexpectedToken),
                    }
                    self.expect_semicolon(cur)?;
                    let program = self.build_call_program(other)?;
                    Ok(Outcome::Run(program))
                } else {
                    // Variable reference / general expression.
                    let value = self.eval_expr(Some(Token::Identifier(other)), cur)?;
                    self.expect_semicolon(cur)?;
                    Ok(Outcome::Run(self.build_value_program(value)?))
                }
            }
        }
    }

    // -- expressions ---------------------------------------------------------

    /// Parse a single atomic term or a parenthesized sub-expression without greedily consuming trailing operators.
    fn parse_atomic_term(&self, cur: &mut Cur) -> Result<u32, ParseError> {
        let tok = cur.next();
        match tok {
            Token::LParen => {
                let val = self.eval_expr(None, cur)?;
                match cur.next() {
                    Token::RParen => Ok(val),
                    _ => Err(ParseError::UnexpectedToken),
                }
            }
            _ => self.resolve_term(tok, cur),
        }
    }

    /// Evaluate an expression left-to-right (no precedence).
    ///
    /// `first` carries an already-consumed leading token (None when the
    /// next token from `cur` starts the expression).
    fn eval_expr(&self, first: Option<Token>, cur: &mut Cur) -> Result<u32, ParseError> {
        let head = match first {
            Some(t) => t,
            None => cur.next(),
        };
        let mut acc = if head == Token::LParen {
            let val = self.eval_expr(None, cur)?;
            match cur.next() {
                Token::RParen => val,
                _ => return Err(ParseError::UnexpectedToken),
            }
        } else {
            self.resolve_term(head, cur)?
        };

        while let Token::Operator(op) = cur.peek() {
            if !matches!(op, b'+' | b'-' | b'*' | b'/' | b'%') {
                break;
            }
            cur.next(); // consume operator
            let rhs_tok = cur.next();
            let rhs = if rhs_tok == Token::LParen {
                let val = self.eval_expr(None, cur)?;
                match cur.next() {
                    Token::RParen => val,
                    _ => return Err(ParseError::UnexpectedToken),
                }
            } else {
                self.resolve_term(rhs_tok, cur)?
            };

            acc = match op {
                b'+' => acc.wrapping_add(rhs),
                b'-' => acc.wrapping_sub(rhs),
                b'*' => acc.wrapping_mul(rhs),
                b'/' => {
                    if rhs == 0 {
                        return Err(ParseError::DivByZero);
                    }
                    acc / rhs
                }
                _ => {
                    if rhs == 0 {
                        return Err(ParseError::DivByZero);
                    }
                    acc % rhs
                }
            };
        }
        Ok(acc)
    }

    /// Resolve one expression term to its constant value.
    ///
    /// `peek ADDR` is a first-class term: the address expression is parsed
    /// and the memory read happens at COMPILE time, so the bound symbol is
    /// an immutable constant — exactly like every other binding (manifesto:
    /// named constants, no variables). A wild compile-time peek faults just
    /// as a runtime one would; Ring 0 makes no promises.
    fn resolve_term(&self, tok: Token, cur: &mut Cur) -> Result<u32, ParseError> {
        match tok {
            Token::Literal(v) => Ok(v),
            Token::Identifier(id) => {
                if id == b"peek" {
                    let addr = self.parse_atomic_term(cur)?;
                    Ok(crate::kernel::memory::peek_u32(addr as usize))
                } else {
                    self.lookup_symbol(id).ok_or(ParseError::UnknownSymbol)
                }
            }
            Token::Error(msg) => Err(ParseError::LexError(msg)),
            _ => Err(ParseError::UnexpectedToken),
        }
    }

    // -- stream emission -----------------------------------------------------

    fn stream_reset(&mut self) {
        self.stream_len = 0;
    }

    fn stream_push(&mut self, word: usize) -> Result<(), ParseError> {
        if self.stream_len >= MAX_STREAM_WORDS {
            return Err(ParseError::StreamFull);
        }
        self.stream[self.stream_len] = word;
        self.stream_len += 1;
        Ok(())
    }

    fn stream_push_lit(&mut self, value: u32) -> Result<(), ParseError> {
        self.stream_push(word_of(primitives::lit_prim))?;
        self.stream_push(value as usize)
    }

    fn stream_halt(&mut self) -> Result<(), ParseError> {
        self.stream_push(word_of(primitives::halt_prim))
    }

    fn take_program(&mut self, yields_value: bool) -> StreamProgram {
        let mut words = [0usize; MAX_STREAM_WORDS];
        words[..self.stream_len].copy_from_slice(&self.stream[..self.stream_len]);
        StreamProgram {
            words,
            len: self.stream_len,
            yields_value,
        }
    }

    fn build_value_program(&mut self, value: u32) -> Result<StreamProgram, ParseError> {
        self.stream_reset();
        self.stream_push_lit(value)?;
        self.stream_halt()?;
        Ok(self.take_program(true))
    }

    fn find_fn(&self, name: &[u8]) -> Option<usize> {
        (0..MAX_FNS).find(|&i| self.fn_allocated(i) && self.fn_names[i].eq_bytes(name))
    }

    fn fn_allocated(&self, i: usize) -> bool {
        // Allocation marker: fn_body_lens is set at definition time; an
        // empty-but-valid fn stores len 0, so track liveness via name len.
        self.fn_names[i].len > 0
    }

    fn build_call_program(&mut self, name: &[u8]) -> Result<StreamProgram, ParseError> {
        let index = self.find_fn(name).ok_or(ParseError::UnknownSymbol)?;
        self.stream_reset();
        let len = self.fn_body_lens[index];
        if len > 0 {
            for w in 0..len {
                self.stream_push(self.fn_bodies[index][w])?;
            }
        }
        self.stream_halt()?;
        Ok(self.take_program(false))
    }

    fn splice_call(&mut self, name: &[u8]) -> Result<(), ParseError> {
        let index = self.find_fn(name).ok_or(ParseError::UnknownSymbol)?;
        let len = self.fn_body_lens[index];
        for w in 0..len {
            let word = self.fn_bodies[index][w];
            self.stream_push(word)?;
        }
        Ok(())
    }

    // -- symbol table ----------------------------------------------------------

    fn hash(name: &[u8]) -> usize {
        // FNV-1a 32-bit.
        let mut h: u32 = 0x811C_9DC5;
        for &b in name {
            h ^= b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        (h as usize) % SYMBOL_SLOTS
    }

    fn lookup_symbol(&self, name: &[u8]) -> Option<u32> {
        let start = Self::hash(name);
        for step in 0..SYMBOL_SLOTS {
            let i = (start + step) % SYMBOL_SLOTS;
            let s = &self.symbols[i];
            if !s.used {
                return None; // open addressing: empty slot ends the chain
            }
            if usize::from(s.name_len) == name.len() && &s.name[..name.len()] == name {
                return Some(s.value);
            }
        }
        None
    }

    fn insert_symbol(&mut self, name: &NameBuf, value: u32) -> Result<(), ParseError> {
        let bytes = name.as_slice();
        let start = Self::hash(bytes);
        for step in 0..SYMBOL_SLOTS {
            let i = (start + step) % SYMBOL_SLOTS;
            let s = &mut self.symbols[i];
            if !s.used
                || (usize::from(s.name_len) == bytes.len() && s.name[..bytes.len()] == *bytes)
            {
                s.used = true;
                s.name_len = bytes.len() as u8;
                s.name[..bytes.len()].copy_from_slice(bytes);
                s.value = value;
                return Ok(());
            }
        }
        Err(ParseError::SymbolTableFull)
    }

    fn alloc_fn_slot(&mut self, name: &NameBuf) -> Result<usize, ParseError> {
        // Reject redefinition of a live fn with the same name.
        if self.find_fn(name.as_slice()).is_some() {
            return Err(ParseError::DuplicateFn);
        }
        match (0..MAX_FNS).find(|&i| self.fn_names[i].len == 0) {
            Some(i) => Ok(i),
            None => Err(ParseError::FnTableFull),
        }
    }

    // -- token helpers -----------------------------------------------------------

    fn expect_name(&self, cur: &mut Cur) -> Result<NameBuf, ParseError> {
        match cur.next() {
            Token::Identifier(id) => NameBuf::from_slice(id),
            _ => Err(ParseError::UnexpectedToken),
        }
    }

    fn expect_semicolon(&self, cur: &mut Cur) -> Result<(), ParseError> {
        match cur.next() {
            Token::Semicolon | Token::Eof => Ok(()),
            _ => Err(ParseError::MissingSemicolon),
        }
    }

    fn allow_optional_semicolon(&self, cur: &mut Cur) {
        if matches!(cur.peek(), Token::Semicolon) {
            cur.next();
        }
    }
}

/// One-token-lookahead cursor over the lexer.
struct Cur<'a> {
    lx: Lexer<'a>,
    ahead: Option<Token<'a>>,
}

impl<'a> Cur<'a> {
    fn next(&mut self) -> Token<'a> {
        match self.ahead.take() {
            Some(t) => t,
            None => self.lx.next_token(),
        }
    }

    fn peek(&mut self) -> Token<'a> {
        if self.ahead.is_none() {
            self.ahead = Some(self.lx.next_token());
        }
        // SAFETY-free: just set above.
        self.ahead.unwrap()
    }
}

/// Function-pointer-to-word coercion used when building token streams.
fn word_of(f: primitives::MicroPrimitive) -> usize {
    f as usize
}
