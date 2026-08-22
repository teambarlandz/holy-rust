# Chapter 4 — Single-Pass Streaming JIT Compiler

## 4.1 The Compiler Front End

Holy Rust's compiler is a single-pass, LL(1), streaming parser that reads
left-to-right and produces threaded micro-primitive streams — no AST, no
MIR, no precedence climbing, no intermediate representations. Every input
line is tokenized by the `Lexer`, consumed by a one-token-lookahead cursor
(`Cur`), and compiled directly into a `StreamProgram` or symbol-table
side-effect in a single linear pass.

This design is not a compromise; it is a hard constraint imposed by the
hardware. With 52 KB of usable SRAM on ARM and 5 KB of DTIM on RISC-V,
there is simply no room for a tree representation. The streaming approach
compiles the REPL line as it is parsed, emitting words into a fixed 128-word
buffer that doubles as both the compiler scratch space and the dispatch
stream.

## 4.2 The Compiler Struct

```rust
pub struct Compiler {
    symbols: [Symbol; SYMBOL_SLOTS],        // 32 slots
    fn_names: [NameBuf; MAX_FNS],           // 2 slots
    fn_bodies: [[usize; FN_BODY_WORDS]; MAX_FNS],  // 2 x 32 words
    fn_body_lens: [usize; MAX_FNS],
    stream: [usize; MAX_STREAM_WORDS],      // 128 words
    stream_len: usize,
}
```

| Field           | Capacity       | Purpose                                        |
|-----------------|----------------|-------------------------------------------------|
| `symbols`       | 32 slots       | Name-to-value bindings (open-addressed hash)    |
| `fn_names`      | 2 entries      | Function name storage (survives across lines)   |
| `fn_bodies`     | 2 x 32 words   | Compiled threaded bodies (no trailing halt)     |
| `fn_body_lens`  | 2 entries      | Actual length of each stored body               |
| `stream`        | 128 words      | Scratch compilation buffer; dispatch stream     |
| `stream_len`    | --             | Current word count in `stream`                  |

The `Compiler` is `const`-constructible so the REPL can own it as a
`static mut`:

```rust
static mut COMPILER: Compiler = Compiler::new();
```

This is sound under the single-threaded Ring 0 REPL contract: no interrupt
handler touches the compiler, and the single boot thread is the sole
consumer.

**Key constants:**

- `MAX_STREAM_WORDS = 128`
- `SYMBOL_SLOTS = 32`
- `NAME_MAX = 16`
- `MAX_FNS = 2`
- `FN_BODY_WORDS = 32`

## 4.3 Symbol Table: FNV-1a Hash with Open Addressing

The symbol table is a fixed-capacity hash map with 32 slots, using FNV-1a
hashing and open addressing (linear probing). Each slot stores:

```rust
struct Symbol {
    used: bool,
    name_len: u8,
    name: [u8; NAME_MAX],  // NAME_MAX = 16
    value: u32,
}
```

**FNV-1a hash function:**

```rust
fn hash(name: &[u8]) -> usize {
    let mut h: u32 = 0x811C_9DC5;
    for &b in name {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    (h as usize) % SYMBOL_SLOTS
}
```

**Lookup** starts at `hash(name)` and linearly probes until it finds a
matching name or hits an unused slot (chain terminator). **Insert** finds the
first unused slot or an existing entry with the same name and overwrites it.
Both operations are O(1) amortized for small tables.

The probe loop for lookup:

```rust
fn lookup_symbol(&self, name: &[u8]) -> Option<u32> {
    let start = Self::hash(name);
    for step in 0..SYMBOL_SLOTS {
        let i = (start + step) % SYMBOL_SLOTS;
        let s = &self.symbols[i];
        if !s.used { return None; }
        if s.name_len as usize == name.len() && &s.name[..name.len()] == name {
            return Some(s.value);
        }
    }
    None
}
```

## 4.4 How parse() Works Step by Step

The `parse()` method is the entry point for compiling one REPL line. It
creates a lexer cursor, reads the first token, and dispatches based on the
token type:

```rust
pub fn parse(&mut self, line: &[u8]) -> Result<Outcome, ParseError> {
    let mut cur = Cur { lx: Lexer::new(line), ahead: None };
    let first = cur.next();
    match first {
        Token::Eof              => Err(ParseError::EmptyLine),
        Token::KwLet            => self.parse_let(&mut cur),
        Token::KwFn             => self.parse_fn(&mut cur),
        Token::Identifier(id)   => self.parse_command(id, &mut cur),
        Token::Literal(_)
        | Token::Operator(_)
        | Token::LParen         => {
            let value = self.eval_expr(Some(first), &mut cur)?;
            self.expect_semicolon(&mut cur)?;
            Ok(Outcome::Run(self.build_value_program(value)?))
        }
        Token::Error(msg) => Err(ParseError::LexError(msg)),
        _ => Err(ParseError::UnexpectedToken),
    }
}
```

The first token determines the statement type:

| First Token                      | Handler                          | Output                                  |
|----------------------------------|----------------------------------|------------------------------------------|
| `let`                            | `parse_let()`                    | `Outcome::Bound { name, value }`        |
| `fn`                             | `parse_fn()`                     | `Outcome::FnDefined { name }`           |
| `Identifier`                     | `parse_command()`                | Varies by identifier                     |
| `Literal` / `Operator` / `LParen`| `eval_expr()` then `build_value_program()` | `Outcome::Run(StreamProgram)`  |

### 4.4.1 parse_let: Binding Constants

```text
let NAME = expr;
```

Steps:
1. Expect `Token::Identifier(NAME)` and build a `NameBuf`
2. Expect `Token::Operator(b'=')`
3. Call `eval_expr(None, cur)` to evaluate the right-hand side
4. Expect `Semicolon` or `Eof`
5. Call `insert_symbol(&name, value)` into the hash table
6. Return `Outcome::Bound { name, value }`

All bindings are immutable constants. There are no variables — only named
slots in the symbol table that hold compile-time-evaluated values.

### 4.4.2 parse_fn: Defining Callable Bodies

```text
fn NAME() { body }
```

Steps:
1. Expect `Identifier(NAME)` and build a `NameBuf`
2. Expect `LParen`, then `RParen` (no parameter support)
3. Expect `LBrace`
4. Compile the body into the scratch `stream` buffer:
   - Loop: `parse_body_stmt()` handles `peek`, `poke`, function calls,
     and bare expressions
   - Stop on `RBrace`
5. Reject if `stream_len > FN_BODY_WORDS` (32 words)
6. Allocate a slot via `alloc_fn_slot(&name)` — rejects duplicates
7. Copy `stream[..stream_len]` into `fn_bodies[index]`
8. Return `Outcome::FnDefined { name }`

Body statements are stored **without** a trailing halt so they can be
spliced at call sites. When `NAME()` is called later, `splice_call()`
copies the stored words into the current stream, and the caller's halt
terminates the combined stream.

### 4.4.3 parse_command: System Commands

The `parse_command()` method matches the identifier against known command
names:

| Command               | Signature                  | Output                                |
|-----------------------|----------------------------|----------------------------------------|
| `peek ADDR;`          | `eval_expr` + semicolon    | `Outcome::EnforcedPeek { addr }`      |
| `poke ADDR VAL;`      | `eval_expr` x2 + semicolon | `Outcome::EnforcedPoke { addr, val }` |
| `cap_claim NAME;`     | `expect_name` + semicolon  | `Outcome::Claim(name)`                |
| `cap_drop NAME;`      | `expect_name` + semicolon  | `Outcome::Drop(name)`                 |
| `reg_set_bit ADDR BIT;`| `eval_expr` x2 + semicolon| `Outcome::SetBit { addr, bit }`       |
| `reg_clr_bit ADDR BIT;`| `eval_expr` x2 + semicolon| `Outcome::ClrBit { addr, bit }`       |
| `help`                | optional semicolon         | `Outcome::Help`                        |
| `banner`              | optional semicolon         | `Outcome::Banner`                      |
| `sys_audit`           | optional semicolon         | `Outcome::SysAudit`                    |
| `NAME()`              | `LParen` `RParen` semicolon| `Outcome::Run(build_call_program())`  |
| Other identifier      | general expression         | `Outcome::Run(build_value_program())` |

Every `peek`, `poke`, `reg_set_bit`, and `reg_clr_bit` performs mandatory
capability enforcement via `check_access(addr)` at parse time. If the
address falls within a peripheral region and the matching capability is not
claimed, a `CapabilityViolation` error is returned immediately.

### 4.4.4 Bare Expressions

Any line that starts with a literal, operator, or `(` is treated as a bare
expression. The expression is evaluated at compile time by `eval_expr()`,
and the result is wrapped in a `StreamProgram` via `build_value_program()`:

```text
lit <value>    (push the constant)
halt           (stop dispatch)
```

## 4.5 Expression Evaluation: Left-to-Right, No Precedence

Expressions are evaluated strictly left-to-right with no operator
precedence. The grammar is:

```text
expr  -> term { op term }*
term  -> literal | identifier | "peek" term
op    -> '+' | '-' | '*' | '/' | '%'
```

The `eval_expr()` method:

```rust
fn eval_expr(&self, first: Option<Token>, cur: &mut Cur) -> Result<u32, ParseError> {
    let head = first.unwrap_or_else(|| cur.next());
    let mut acc = self.resolve_term(head, cur)?;
    while let Token::Operator(op) = cur.peek() {
        cur.next();
        let rhs = self.resolve_term(cur.next(), cur)?;
        acc = match op {
            b'+' => acc.wrapping_add(rhs),
            b'-' => acc.wrapping_sub(rhs),
            b'*' => acc.wrapping_mul(rhs),
            b'/' => {
                if rhs == 0 { return Err(ParseError::DivByZero); }
                acc / rhs
            }
            b'%' => {
                if rhs == 0 { return Err(ParseError::DivByZero); }
                acc % rhs
            }
            _ => return Err(ParseError::UnsupportedOperator(op)),
        };
    }
    Ok(acc)
}
```

Division by zero is caught at compile time and returns an error. All
arithmetic is wrapping (no overflow traps).

**Example:** `2 + 3 * 4` evaluates as `(2 + 3) * 4 = 20`, not `2 + (3 * 4) = 14`.

## 4.6 resolve_term: Literals, Identifiers, Compile-Time Peek

`resolve_term()` converts a single token into its `u32` value:

- **`Token::Literal(v)`** — return `v` directly
- **`Token::Identifier(b"peek")`** — parse the address expression, perform
  a compile-time memory read via `peek_u32(addr)`, and return the value.
  This makes `peek` a first-class compile-time term: the memory read
  happens during parsing, so the bound symbol is an immutable constant.
  A wild compile-time peek faults just as a runtime one would; Ring 0
  makes no promises.
- **`Token::Identifier(name)`** — `lookup_symbol(name)` in the FNV-1a
  hash table; return `ParseError::UnknownSymbol` if not found

## 4.7 The Threaded Micro-Primitives

The parser emits arrays of `usize` words into the stream buffer. Each word
is either a function pointer to a micro-primitive or an inline literal
argument. The micro-primitives are defined in `primitives.rs`:

```rust
pub type MicroPrimitive = unsafe fn(ip: *const usize) -> *const usize;
```

| Primitive         | Behavior                                                |
|-------------------|----------------------------------------------------------|
| `lit_prim`        | Read `*ip`, push to VM stack, return `ip + 1`           |
| `add_prim`        | Pop b, pop a, push `a + b`, return `ip`                 |
| `sub_prim`        | Pop b, pop a, push `a - b`, return `ip`                 |
| `mul_prim`        | Pop b, pop a, push `a * b`, return `ip`                 |
| `div_prim`        | Pop b, pop a, push `a / b` (0 on div-by-zero), return `ip` |
| `load_reg_prim`   | Pop addr, read `*addr`, push value, return `ip`         |
| `write_reg_prim`  | Pop value, pop addr, write `*addr = value`, return `ip` |
| `halt_prim`       | Return null pointer (stops dispatch)                     |

All primitives share the same C-ABI-ish signature. The `ip` parameter
points past the opcode word itself, so primitives that need inline arguments
(like `lit_prim`) read from `*ip` and return `ip + 1`. Primitives that
take no inline arguments (like `add_prim`) return `ip` unchanged.

## 4.8 How the Parser Emits Opcodes

When the parser encounters a statement like `poke 0x40020000 0x01`, it
emits words into the stream buffer:

```text
word_of(primitives::lit_prim)     // opcode: push literal
0x40020000                        // inline argument: address
word_of(primitives::lit_prim)     // opcode: push literal
0x01                              // inline argument: value
word_of(primitives::write_reg_prim) // opcode: store
```

The `word_of()` function casts a `MicroPrimitive` function pointer to
`usize`:

```rust
fn word_of(f: primitives::MicroPrimitive) -> usize {
    f as usize
}
```

On ARM, function pointers have the Thumb bit set (bit 0), so casting them
to `usize` produces an odd number. The threaded dispatch loop handles this
transparently via `core::mem::transmute`.

The `stream_push_lit()` helper emits a lit+value pair:

```rust
fn stream_push_lit(&mut self, value: u32) -> Result<(), ParseError> {
    self.stream_push(word_of(primitives::lit_prim))?;
    self.stream_push(value as usize)
}
```

The `stream_halt()` method appends the halt opcode:

```rust
fn stream_halt(&mut self) -> Result<(), ParseError> {
    self.stream_push(word_of(primitives::halt_prim))
}
```

## 4.9 The StreamProgram Struct

```rust
pub struct StreamProgram {
    words: [usize; MAX_STREAM_WORDS],  // 128 words
    len: usize,
    yields_value: bool,
}
```

- `words` — the compiled threaded stream (function pointers + inline arguments)
- `len` — number of valid words
- `yields_value` — true if the REPL should print the result (expressions,
  peek); false for side-effect-only streams (poke, function calls)

`StreamProgram` is a fixed-capacity inline type with no heap allocation.
It is created by `take_program()` which copies the compiler's scratch
stream into the program's own buffer:

```rust
fn take_program(&mut self, yields_value: bool) -> StreamProgram {
    let mut words = [0usize; MAX_STREAM_WORDS];
    words[..self.stream_len].copy_from_slice(&self.stream[..self.stream_len]);
    StreamProgram { words, len: self.stream_len, yields_value }
}
```

## 4.10 StreamProgram::run(): Dual Execution Paths

```rust
pub fn run(&self) -> Option<u32> {
    unsafe {
        crate::kernel::exec::vm_reset();
        if self.len > 0 {
            // Try native path first (Milestone 4 JIT).
            if let Ok(result) = crate::compiler::native::compile_and_run(
                &self.words, self.len, self.yields_value,
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
```

The execution flow:
1. Reset the VM operand stack (`vm_reset()`)
2. Attempt native codegen via `compile_and_run()`
3. If native codegen succeeds, return its result
4. If native codegen returns `Err(())`, fall back to `run_threaded_stream()`
5. If `yields_value` is true, pop and return the top of the VM stack

## 4.11 Native Codegen (ARM): Two-Register Strategy

The native codegen path compiles threaded streams into real Thumb-2 machine
code and executes them from `EXEC_BUFFER`. It uses a **two-register**
strategy:

| Register | Role                                    | ARM Mapping |
|----------|-----------------------------------------|-------------|
| ACC      | Accumulator — holds the running result  | r0          |
| SCRATCH  | Temporary — second operand or address   | r1          |
| Return   | Function return value                   | r0          |

The emitter (`Thumb2Emitter`) writes into `EXEC_BUFFER` at
`0x2000_2000` (SRAM, writable + executable). Instructions emitted:

- `MOVS Rd, #imm8` for small immediates (0-255)
- `MOVW/MOVT` for full 32-bit immediates
- `ADDS Rd, Rn, Rm` for addition
- `SUBS Rd, Rn, Rm` for subtraction
- `MULS Rdm, Rn, Rm` for multiplication
- `SDIV Rd, Rn, Rm` for signed division (Thumb-2 extension)
- `LDR Rt, [Rn, #0]` for load from address in SCRATCH
- `STR Rt, [Rn, #0]` for store to address in ACC
- `BX LR` for return

The `is_compilable()` check gates native codegen. Only streams matching the
pattern `lit [op lit]* [load|store] halt` are compiled. Nested expressions
fall back to the threaded interpreter because the two-register strategy
cannot handle intermediate register spills.

After emission, `exec_buffer_entry()` casts the buffer base address to a
callable function pointer (setting bit 0 on ARM for Thumb state), and
`jump_to_sram()` invokes it.

## 4.12 Native Codegen (RISC-V): Currently Disabled

The RISC-V codegen path (`Riscv32Emitter`) is implemented but currently
returns `Err(())` unconditionally:

```rust
#[cfg(target_arch = "riscv32")]
{
    let _ = (stream, len, yields_value);
    Err(())
}
```

The reason: LLD emits the ITIM (Instruction Tightly Integrated Memory)
section at `0x0800_0000` in a RW-only `PT_LOAD` segment. QEMU's
`sifive_e` machine enforces execute-permission on `PT_LOAD` segments, so
any attempt to execute code from this region faults.

The `Riscv32Emitter` is fully implemented with proper encodings for
RV32I instructions — LUI/ADDI for immediates, ADD/SUB/MUL/DIV R-type,
LW/SW for memory, JALR for return — but cannot run until the ELF is
patched with execute permission on the ITIM segment.

## 4.13 The Threaded Dispatch Loop

The threaded dispatch engine in `run_threaded_stream()` implements a classic
direct-threaded interpreter:

```rust
pub unsafe fn run_threaded_stream(mut ip: *const usize) {
    while !ip.is_null() {
        let word = core::ptr::read_volatile(ip);
        if word == 0 { break; }
        ip = ip.add(1);
        let prim: MicroPrimitive = core::mem::transmute(word);
        ip = prim(ip);
    }
}
```

Each step:
1. Volatile-read one word from the instruction pointer
2. Stop on zero (defensive guard against corrupt streams)
3. Advance `ip` past the current word
4. Transmute the word to a `MicroPrimitive` function pointer
5. Call it, passing `ip` (which now points at inline arguments)
6. The primitive returns the next `ip`

Dispatch ends when a primitive returns null (`halt_prim`).

**VM operand stack:** The primitives communicate through a separate operand
stack of 64 `usize` words:

```rust
pub const VM_STACK_SIZE: usize = 64;
static mut VM_STACK: [usize; VM_STACK_SIZE] = [0; VM_STACK_SIZE];
static mut VM_SP: usize = 0;
```

`vm_push()` returns `false` on overflow (silent drop, no panic). `vm_pop()`
returns 0 on underflow (deterministic, no panic). The hot loop must never
panic in Ring 0.

## 4.14 EXEC_BUFFER: 4 KB SRAM Execution Buffer

```rust
pub const EXEC_BUFFER_SIZE: usize = 4096;

#[repr(C, align(4))]
pub struct ExecBuffer(pub [u8; EXEC_BUFFER_SIZE]);

#[used]
#[link_section = ".sram_code"]
pub static mut EXEC_BUFFER: ExecBuffer = ExecBuffer([0; EXEC_BUFFER_SIZE]);
```

The buffer is placed in the `.sram_code` linker section, which maps to:

- ARM: `0x2000_2000` (4 KB, writable + executable SRAM)
- RISC-V: `0x0800_0000` (4 KB, ITIM — Instruction Tightly Integrated Memory)

`#[used]` + linker `KEEP()` guarantee the buffer survives even when no live
code path references it yet — the region must exist in the final image for
JIT execution.

The `Thumb2Emitter` writes halfwords (16-bit Thumb-2 instructions) into
this buffer. The `Riscv32Emitter` writes full 32-bit RV32I words. Both
emitters track their cursor position and return `Err(EmitError::Overflow)`
if the buffer fills.

## 4.15 flush_instruction_cache(): Pipeline Synchronization

After writing machine code into `EXEC_BUFFER`, the instruction pipeline must
be flushed before execution:

```rust
pub unsafe fn flush_instruction_cache() {
    #[cfg(target_arch = "arm")]
    unsafe {
        core::arch::asm!("dsb", "isb", options(nostack));
    }

    #[cfg(target_arch = "riscv32")]
    unsafe {
        core::arch::asm!("fence.i", options(nostack));
    }
}
```

On ARM Cortex-M, `dsb` (data synchronization barrier) ensures all previous
memory writes are visible, and `isb` (instruction synchronization barrier)
flushes the prefetch pipeline. On RISC-V, `fence.i` is the
instruction-fetch fence that ensures recently written instructions are
visible to the fetch unit.

This function is called by `execute_sram_buffer()` and indirectly by the
native codegen path before jumping to the generated code.

## 4.16 The is_compilable Check

Before attempting native codegen, `is_compilable()` scans the threaded stream
to verify it matches the compilable pattern:

```text
lit [op lit]* [load|store] halt
```

Where `op` is one of: `add`, `sub`, `mul`, `div`.

```rust
fn is_compilable(stream: &[usize; MAX_STREAM_WORDS], len: usize) -> bool {
    let lit_w = word_of(primitives::lit_prim);
    let halt_w = word_of(primitives::halt_prim);
    let add_w = word_of(primitives::add_prim);
    let sub_w = word_of(primitives::sub_prim);
    let mul_w = word_of(primitives::mul_prim);
    let div_w = word_of(primitives::div_prim);
    let load_w = word_of(primitives::load_reg_prim);
    let write_w = word_of(primitives::write_reg_prim);

    let mut ip = 0;
    let mut lit_seen = false;

    while ip < len {
        let w = stream[ip];
        if w == 0 || w == halt_w { return true; }
        if w == lit_w {
            ip += 2;
            lit_seen = true;
            continue;
        }
        if w == add_w || w == sub_w || w == mul_w
            || w == div_w || w == load_w || w == write_w
        {
            lit_seen = true;
            ip += 1;
            continue;
        }
        return false;
    }
    lit_seen
}
```

The check walks the stream word by word. A `lit` word is followed by its
inline argument (so `ip += 2`). All known opcodes advance `ip += 1`. Any
unknown word returns `false`. The stream must contain at least one `lit`
before a halt.

Streams that fail this check — for example, those containing function-call
splices or nested expressions — fall back to the threaded interpreter
automatically.
