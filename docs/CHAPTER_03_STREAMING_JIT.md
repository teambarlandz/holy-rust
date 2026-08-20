# CHAPTER 03: STREAMING JIT COMPILER

## 3.1 The Single-Pass Syntax Strategy
Standard AOT compilers (such as rustc) process code through a multi-pass pipeline: converting raw source text into an Abstract Syntax Tree (AST), lowering it to High-Level IR (HIR), transforming it into Mid-Level IR (MIR) to run lifetime and borrow analysis, generating LLVM IR, and finally emitting target machine code.

### Standard Rust Pipeline
[ Source Text ] ──► [ AST ] ──► [ HIR ] ──► [ MIR ] ──► [ LLVM IR ] ──► [ Native Code ]

This multi-pass architecture is fundamentally incompatible with real-time, bare-metal JIT execution on constrained hardware. Storing nested AST nodes and control-flow graphs requires significant heap allocation, while multi-pass lowerings introduce unacceptable compilation latency.

### Holy Rust Streaming Pipeline
[ Text Stream / REPL ] ──► [ Single-Pass Lexer/Parser ] ──► [ Capability Token & Opcode Emitter ] ──► [ Executable SRAM ]

### Grammar Constraints for Single-Pass Compilation
To enable single-pass compilation directly from a stream of characters into native execution tokens, the language grammar adheres to three core constraints:
- **LL(1) Predictability**: The parser requires a lookahead of at most one token to determine the exact evaluation path. Declarations, expressions, and statements are uniquely identified by their initial token.
- **Left-to-Right Type and Capability Inference**: Types and linear capability tokens must be explicitly declared or deterministically inferred from left-to-right operand ordering. No backward pass is permitted to resolve symbols.
- **Top-Down Symbol Resolution**: Functions, static memory definitions, and capabilities must be declared before use, or resolved via a fixed-size, deterministic global symbol hash table residing in SRAM.

## 3.2 Memory-Efficient Parsing & Tokenization
The Holy Rust parser operates directly on a bounded input buffer (e.g., character bytes arriving over UART, USB-CDC, or an interactive shell). It performs lexing, parsing, and code emission in a single, unbuffered loop.

```text
┌─────────────────────────────────────────┐
│            Single-Pass Loop             │
└─────────────────────────────────────────┘
                                              │
[ Character Stream ] ──► [ Lexer State ] ──►──► [ Capability Check ] ──► [ SRAM Output Buffer ]
```

### Memory Footprint Allocation
To execute safely on microcontrollers with as little as 16 KB of SRAM, the compiler avoids heap allocations (malloc/free) entirely. All compiler state is statically allocated within a fixed memory budget:

| Compiler Component | Memory Structure | Static Allocation |
|---|---|---|
| Lexer Ring Buffer | Circular Byte Array | 256 Bytes |
| Symbol Hash Table | Fixed-bucket Open Addressing | 2 KB (Up to 128 active symbols) |
| Capability Registry | Bitmask & Linear Token Array | 512 Bytes |
| Target Code Buffer | Executable SRAM Page | 4 KB to 16 KB (Configurable) |

### The Tokenizer Loop
```rust
pub struct Lexer<'a> {
    stream: &'a [u8],
    cursor: usize,
}

#[derive(Debug, PartialEq)]
pub enum Token {
    KwFn,
    KwLet,
    Identifier(&'static str),
    CapabilityToken(u16),
    Operator(u8),
    Literal(u32),
    Eof,
}

impl<'a> Lexer<'a> {
    pub fn next_token(&mut self) -> Token {
        // Single-pass scanner advancing cursor without heap allocation
        while self.cursor < self.stream.len() {
            let b = self.stream[self.cursor];
            self.cursor += 1;
            match b {
                b' ' | b'\t' | b'\r' | b'\n' => continue,
                b'a'..=b'z' | b'A'..=b'Z' => return self.parse_identifier(),
                b'0'..=b'9' => return self.parse_literal(),
                b'+' | b'-' | b'*' | b'/' | b'=' => return Token::Operator(b),
                _ => break,
            }
        }
        Token::Eof
    }
}
```

## 3.3 Threaded Micro-Primitives Execution Engine
Compiling directly to raw machine code (e.g., ARM Thumb-2 or RISC-V binary opcodes) for every construct requires target-specific register allocation algorithms. To maintain high execution speeds while keeping the compiler footprint minimal, Holy Rust uses Tokenized Direct Threading.

### Mechanism of Direct Threaded Execution
Instead of translating complex expressions into raw assembly instructions at runtime, the Holy Rust JIT engine references pre-compiled, highly optimized Micro-Primitives stored permanently in read-only Flash memory (.rodata).

The output of the Holy Rust JIT compiler is a linear array of 32-bit or 64-bit Execution Tokens (direct function pointers to these Flash micro-primitives) interleaved with direct literal arguments.

```text
                      EXECUTABLE SRAM
                    ┌──────────────────┐
                    │ Token 0: 0x08001240 ───┐ (Function Pointer)
                    │ Argument: 0x40021018   │
                    │ Token 1: 0x080012A0 ───┼──┐
                    │ Argument: 0x00000001   │  │
                    └──────────────────┘  │  │
                                          │  │
                                    FLASH MEMORY        │  │
                    ┌──────────────────┐  │  │
                    │ 0x08001240:      │◄─┘  │
                    │   load_reg_prim  │     │
                    │ 0x080012A0:      │     │
                    │   write_reg_prim │◄────┘
                    └──────────────────┘
```

### The Central Inner Interpreter Thread Loop
The execution engine steps through the token array using a single register as the instruction pointer (IP):

```rust
type MicroPrimitive = fn(ip: *const usize) -> *const usize;

#[no_mangle]
pub unsafe extern "C" fn run_threaded_stream(mut ip: *const usize) {
    while !ip.is_null() {
        // Fetch pointer to the pre-baked Flash function
        let primitive_ptr = *ip as *const MicroPrimitive;
        ip = ip.add(1);
        
        // Jump directly to micro-primitive; pass current IP for argument fetching
        ip = (*primitive_ptr)(ip);
    }
}
```

### Performance Characteristics
- **Near-Native Speed**: Branch mispredictions are minimized because execution transfers directly from primitive to primitive using CPU indirect jumps.
- **Zero Register Allocation Overhead**: Micro-primitives utilize fixed CPU register contracts (e.g., R0 holds accumulator, R1 holds target address).
- **100% Code Portability**: The engine logic is identical across ARM, RISC-V, and x86. Only the Flash-resident primitives need target-specific assembly optimizations.

## 3.4 Target Emitter Architecture
For critical hot loops where threaded execution latency is undesirable, Holy Rust provides a lightweight Target Emitter Backend capable of writing raw machine code bytes directly into SRAM.

### Architecture-Specific Machine Code Emitters
```text
                      ┌──────────────────────────────┐
                      │ Universal JIT Front Engine   │
                      └──────────────┬───────────────┘
                                     │ Emits Universal Opcodes
                                     ▼
                      ┌──────────────────────────────┐
                      │ Hardware Target Emitter Trait│
                      └──────┬────────────────┬──────┘
                             │                │
           ┌───────────────┘                └───────────────┐
           ▼                                                ▼
┌───────────────────────┐                        ┌───────────────────────┐
│   ARM Emitter Core    │                        │  RISC-V Emitter Core  │
│    (Thumb-2 ISA)      │                        │     (RV32I ISA)       │
└───────────┬───────────┘                        └───────────┬───────────┘
            │ Writes Bytes                                   │ Writes Bytes
            ▼                                                ▼
┌───────────────────────┐                        ┌───────────────────────┐
│ ARM Executable SRAM   │                        │ RISC-V Executable SRAM│
└───────────────────────┘                        └───────────────────────┘
```

The emitter translates high-level arithmetic and register operations into target machine instructions without executing heavy optimization passes.

```rust
pub trait TargetEmitter {
    fn emit_mov_imm(&mut self, reg: u8, imm: u32);
    fn emit_store_u32(&mut self, src_reg: u8, addr_reg: u8);
    fn emit_ret(&mut self);
}

// ARM Cortex-M (Thumb-2) Machine Code Emitter
pub struct Thumb2Emitter {
    pub sram_cursor: *mut u16,
}

impl TargetEmitter for Thumb2Emitter {
    fn emit_mov_imm(&mut self, reg: u8, imm: u32) {
        unsafe {
            // Emit MOVW (Move Wide) 32-bit instruction encoding
            let op_low = 0xF240 | ((imm >> 12) & 0xF) as u16 | (((imm >> 11) & 0x1) << 10) as u16;
            let op_high = ((imm & 0xFF) << 8) as u16 | ((reg as u16) << 8) | ((imm >> 8) & 0x7) as u16;
            
            *self.sram_cursor = op_low;
            self.sram_cursor = self.sram_cursor.add(1);
            *self.sram_cursor = op_high;
            self.sram_cursor = self.sram_cursor.add(1);
        }
    }

    fn emit_store_u32(&mut self, src_reg: u8, addr_reg: u8) {
        unsafe {
            // Emit STR (Register) 16-bit Thumb instruction: 0x6000 | (addr << 3) | src
            let op = 0x6000 | ((addr_reg as u16) << 3) | (src_reg as u16);
            *self.sram_cursor = op;
            self.sram_cursor = self.sram_cursor.add(1);
        }
    }

    fn emit_ret(&mut self) {
        unsafe {
            // Emit BX LR (Branch and Exchange to Link Register): 0x4770
            *self.sram_cursor = 0x4770;
            self.sram_cursor = self.sram_cursor.add(1);
        }
    }
}
```

### Pipeline Comparison

| Metric                       | Threaded Micro-Primitives     | Native Target Emitter        |
|---|---|---|
| Compilation Speed            | ~100 Microseconds             | ~1 Millisecond               |
| Executable Footprint         | Compact (1x Words)            | Larger (Full Machine Code)   |
| Code Execution Speed         | ~85% Native Bare Metal        | ~98% Native Bare Metal       |
| Hardware Portability         | 100% Platform Agnostic        | Requires Architecture Emitter|
| Memory Safety Verification   | Checked via Capability Tokens | Checked via Capability Tokens|