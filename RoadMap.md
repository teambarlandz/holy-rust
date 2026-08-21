# RoadMap.md: Holy Rust Implementation Plan

**Project Directory:**

holy-rust/
├── .cargo/
│   └── config.toml             # Custom target runners (QEMU & probe-rs)
├── .github/
│   └── workflows/
│       └── ci.yml              # Automated build, lint, and QEMU test workflow
├── docs/                       # Architectural documentation suite (Chapters 1-6)
├── memory.x                    # Linker script for Flash/SRAM bounds
├── Cargo.toml                  # Workspace manifest (#![no_std], PACs, HALs)
├── build.rs                    # Linker script validation & SRAM section layout
└── src/
├── main.rs                 # Kernel entry point & Ring 0 boot sequence
├── lib.rs                  # Core library exports
├── capabilities/           # O(1) Linear Capability Token Engine
│   ├── mod.rs              # Capability module exports
│   ├── registry.rs         # SRAM bitfield token state tracking
│   └── tokens.rs           # Non-copyable Cap<T> abstractions & PAC wrappers
├── compiler/               # Single-Pass Streaming JIT Engine
│   ├── mod.rs              # Compiler exports
│   ├── lexer.rs            # Zero-allocation streaming ASCII lexer
│   ├── parser.rs           # Single-pass grammar evaluator
│   ├── primitives.rs       # Threaded micro-primitive execution dispatch table
│   └── emitter.rs          # Thumb-2 (ARM) & RV32I (RISC-V) instruction emitters
├── kernel/                 # Ring 0 Core Infrastructure
│   ├── mod.rs              # Kernel subsystem exports
│   ├── exec.rs             # Executable SRAM buffer execution & pointer casting
│   ├── interrupt.rs        # Dynamic Vector Table relocation & C-ABI thunks
│   └── memory.rs           # Direct memory allocation & volatile access (peek/poke)
└── drivers/                # Hardware Abstraction Layer & REPL Interfaces
├── mod.rs              # Driver exports
├── uart.rs             # Bare-metal non-blocking UART driver (Ring Buffer)
└── repl.rs             # ASCII terminal REPL state machine & command handler


---

## Milestone 1: Scaffold & Toolchain Setup

**Objective:** Establish the foundational repository structure, toolchain configurations, and target definitions required to build and run Holy Rust on both QEMU emulators and physical hardware.


**Tasks:**
- Create `.cargo/config.toml` with custom target runners for QEMU (ARM Cortex-M and RISC-V) and probe-rs for flashing
- Write `memory.x` linker script defining Flash/SRAM memory boundaries (e.g., Flash at 0x0800_0000, 128KB; SRAM at 0x2000_0000, 64KB)
- Initialize `Cargo.toml` as workspace manifest with `#![no_std]`, include PAC and HAL crate dependencies (e.g., `stm32f4xx-hal`, `rp2040-hal`, `embedded-hal`)
- Define target triples: `thumbv7em-none-eabihf` (ARM Cortex-M4) and `riscv32imac-unknown-none-elf`
- Add build.rs for linker script validation and SRAM section layout confirmation

**Verification Checklist:**
- `[ ]` `cargo build --target thumbv7em-none-eabihf` compiles without errors
- `[ ]` `cargo build --target riscv32imac-unknown-none-elf` compiles without errors
- `[ ]` `probe-rs target flash --chip rp2040` successfully flashes a "Hello, World" UART banner
- `[ ]` QEMU emulation: `qemu-system-arm -M std -cpu cortex-m4 -nographic -semihosting` receives UART output
- `[ ]` `memory.x` correctly maps Flash and SRAM regions as confirmed by `cargo bleed`

---

## Milestone 2: Bare-Metal Kernel & SRAM Execution Unit

**Objective:** Implement the `#![no_std]` runtime, vector table initialization, and dynamic SRAM code execution capability that enables the JIT compiler to write and jump to executable code in SRAM.

**Tasks:**
- Write `src/main.rs` as the Ring 0 boot sequence:
  - Reset vector handler that initializes the VTOR (Vector Table Offset Register)
  - SRAM vector table relocation (move exception handlers from Flash to SRAM)
  - UART driver initialization (bring up 115200 baud serial console)
  - Print "Holy Rust REPL v0.1" ASCII banner via semihosting or direct MMIO
  - Jump to the REPL execution loop
- Implement `src/kernel/exec.rs`:
  - Define executable SRAM buffer: `#[link_section = ".sram_code"] static mut EXEC_BUFFER: [u8; 4096] = [0; 4096];`
  - Function `fn jump_to_sram(func: fn() -> u32) -> u32` using `core::mem::transmute` to cast `EXEC_BUFFER.as_ptr()` to a function pointer
  - Minimal `#[panic_handler]` that prints "Panic!" via UART and loops forever
- Implement vector table in SRAM:
  - `static mut VECTOR_TABLE: [Option<fn()>; 256] = [None; 256];`
  - Linker section `.sram_vectors` placed at address 0x2000_0400

**Verification Checklist:**
- `[ ]` Target resets and UART outputs the Holy Rust banner
- `[ ]` `jump_to_sram()` successfully casts and jumps to machine code stored in EXEC_BUFFER
- `[ ]` A simple "ADD immediate" Thumb-2 instruction sequence in EXEC_BUFFER returns the correct result (e.g., 42)
- `[ ]` `#[panic_handler]` prints via UART before looping (verified in QEMU semihosting)
- `[ ]` Vector table relocation: exceptions vector to SRAM handlers instead of Flash defaults

---

## Milestone 3: Linear Capability Engine & Token Registry

**Objective:** Build the O(1) constant-time SRAM bitfield capability registry and non-copyable `Cap<T>` linear abstractions that wrap Peripheral Access Crates (PACs) to enforce single-owner hardware access.

**Tasks:**
- Create `src/capabilities/mod.rs` exporting the capability module public API
- Implement `src/capabilities/registry.rs`:
  - Fixed-size bitfield struct `CapabilityRegistry` aligned in SRAM at 0x2000_1000
  - `fn acquire(resource_id: usize) -> Option<CapIndex>` - atomically set bit, return token index (O(1) bitmask operation)
  - `fn release(cap: CapIndex)` - atomically clear bit (O(1) bitmask operation)
  - `fn available(cap: CapIndex) -> bool` - single AND check (O(1))
- Implement `src/capabilities/tokens.rs`:
  - Non-copyable, non-cloneable `Cap<T: HardwareResource>` struct with `PhantomData<T>` marker
  - `unsafe fn steal() -> Cap<T>` - singleton constructor granting exclusive hardware ownership
  - `fn set_high(self) -> Cap<T>` - consume token, write to PAC register, return (potentially renewed) token
  - `fn set_low(self) -> Cap<T>` - same for clear operation
  - Implement `!Copy` and `!Clone` trait bounds explicitly
  - Write PAC wrappers for at least one peripheral (e.g., GPIOA pin 5 set/clear via BSRR register)

**Verification Checklist:**
- `[ ]` `CapabilityRegistry::acquire(0)` sets bit 0, returns valid CapIndex
- `[ ]` `CapabilityRegistry::available(0)` returns false after acquire, true after release
- `[ ]` `Cap::<GPIOA_pin5>::steal()` produces a valid non-zero token
- `[ ]` `pin_cap.set_high()` compiles to a single `STR` instruction to GPIOA BSRR register
- `[ ]` Attempting to `Cap::clone()` or `Cap::copy()` produces a compile error (`!Copy`/`!Clone` enforced)

---

## Milestone 4: Single-Pass Streaming JIT & Micro-Primitives

**Objective:** Build the zero-allocation streaming lexer/parser, Flash-backed primitive lookup table, and architecture-specific machine-code emitters (Thumb-2 for ARM, RV32I for RISC-V) that transform REPL text streams into executable SRAM token arrays.

**Tasks:**
- Create `src/compiler/mod.rs` exporting the compiler public API
- Implement `src/compiler/lexer.rs`:
  - Zero-allocation struct `Lexer<'a> { stream: &'a [u8]; cursor: usize }`
  - `fn next_token(&mut self) -> Token` single-pass scanner (no heap allocation, no dynamic buffers)
  - Token enum: `KwFn`, `KwLet`, `Identifier(&'static str)`, `CapabilityToken(u16)`, `Operator(u8)`, `Literal(u32)`, `Eof`
  - Whitespace skipping, identifier parsing, literal number parsing
- Implement `src/compiler/parser.rs`:
  - `fn parse(stream: &[u8]) -> Result<Vec<Token>, Error>` single-pass parser
  - Top-down symbol resolution using fixed-size SRAM hash table (128 entries, 2KB)
  - Left-to-right type and capability inference (no backward pass)
  - Grammar constraints: LL(1) predictability, left-to-right inference, top-down symbol resolution
- Implement `src/compiler/primitives.rs`:
  - Micro-primitive function type: `type MicroPrimitive = fn(ip: *const usize) -> *const usize;`
  - Flash-resident primitive table (`.rodata` section) with entries: `load_reg_prim`, `write_reg_prim`, `add_prim`, `sub_prim`
  - ARM Thumb-2 micro-primitive implementations (inline assembly or intrinsic-based)
  - RISC-V RV32I micro-primitive implementations
- Implement `src/compiler/emitter.rs`:
  - `pub trait TargetEmitter { fn emit_mov_imm(&mut self, reg: u8, imm: u32); fn emit_store_u32(&mut self, src_reg: u8, addr_reg: u8); fn emit_ret(&mut self); }`
  - `Thumb2Emitter` writing MOVW/MOVT, STR, BX LR instructions into `EXEC_BUFFER` (as generated in Milestone 2)
  - `Riscv32Emitter` writing LUI/ADDI, SW, JALR instructions into `EXEC_BUFFER`

**Verification Checklist:**
- `[ ]` Lexer processes 1000-char UART stream without a single heap allocation (verified via Miri / allocator fuzzer)
- `[ ]` Parser correctly tokenizes `fn main() {}` and `let x = 42;` patterns
- `[ ]` Micro-primitive dispatch: `run_threaded_stream(ip)` fetches function pointer from Flash and jumps, returning correct results
- `[ ]` Thumb2Emitter emits valid Thumb-2 instruction encoding (verified by `cargo embed` disassembly)
- `[ ]` Riscv32Emitter emits valid RV32I instruction encoding (verified by QEMU riscv32 target test)
- `[ ]` Pipeline comparison: threaded micro-primitives compile in ~100us, native emitter in ~1ms (benchmark script)

---

## Milestone 5: Interactive UART REPL & Hardware Integration

**Objective:** Combine the UART driver, streaming tokenizer, capability verification, and SRAM executor into a live interactive REPL loop that accepts Holy Rust source code over serial, compiles it in a single pass, and executes it with O(1) capability safety guarantees.

**Tasks:**
- Write `src/drivers/mod.rs` exporting the driver module public API
- Implement `src/drivers/uart.rs`:
  - Non-blocking UART driver with fixed-size ring buffer (256 bytes, static allocation)
  - `fn poll_get_byte() -> Option<u8>` - try to consume a byte without blocking
  - `fn put_byte(byte: u8)` - transmit a single character
  - `fn irq_handler()` - handle UART interrupt (RXNE), push byte to ring buffer, acknowledge flag
- Implement `src/drivers/repl.rs`:
  - VT100 REPL state machine: `Idle` -> `Reading` -> `Evaluating` -> `Printing` -> `Idle`
  - Escape sequence handling: `\r\n` (enter), `\b` (backspace), Ctrl-U (line kill), Ctrl-C (interrupt)
  - `fn eval(line: &[u8]) -> Result<(), Error>` - lexer → capability verifier → SRAM emitter → jump to execution
  - `peek_u32(addr) / poke_u32(addr, val)` built-in primitives (1-3 cycle access)
  - `cap_claim_peripheral() / cap_release_peripheral()` REPL commands
- Implement QEMU integration test harness:
  - CI workflow `.github/workflows/ci.yml` that builds, flashes (or loads via QEMU), runs REPL session script, and verifies expected output
  - Automated test: type `poke 0x4000_0000 1` → verify UART echo "OK" → type `peek 0x4000_0000` → verify echoed value

**Verification Checklist:**
- `[ ]` UART receives typed characters and echoes them back (loopback test)
- `[ ]` REPL accepts `poke 0x2000_0000 42` and echoes "OK"
- `[ ]` REPL accepts `peek 0x2000_0000` and echoes the stored value
- `[ ]` REPL accepts `cap_claim GPIOA` and responds with capability token confirmation
- `[ ]` Capability verification: REPL rejects `poke` to already-claimed peripheral without proper token
- `[ ]` Full REPL cycle: type command → UART receive → tokenizer → capability check → SRAM emit → jump → execute → print result → return to Idle state
- `[ ]` CI workflow passes on every push (build + QEMU emulation test)

---

## Cross-Milestone Requirements

**Quality Standards (enforced across all milestones):**
1. **Zero Placeholder Code:** No `// TODO`, `todo!()`, `unimplemented!()`, or truncated snippets. Every function is fully written and compile-ready.
2. **Strict `unsafe` Documentation:** Every `unsafe` block includes a `// SAFETY:` comment detailing hardware register alignment, volatile memory access semantics, and execution safety.
3. **Zero Dynamic Allocation (`no_alloc`):** All ring buffers, capability registries, and execution buffers reside in static memory or stack frames. No `alloc` crate usage.
4. **Complete Module Coverage:** Full implementations for all modules listed in the repository tree (as specified in `docs/implementation/prompt.md`).
5. **QEMU First:** All development and verification occurs in QEMU before silicon bring-up. QEMU test harness must pass before any milestone is considered complete.
6. **CI Integration:** `.github/workflows/ci.yml` automates build, lint (clippy), and QEMU emulation tests on every commit.

**Success Criteria (final verification):**
- Holy Rust boots on QEMU ARM Cortex-M3 and RISC-V RV32I targets
- UART REPL is functional: `poke`, `peek`, `cap_claim`, `cap_release` all work
- JIT compilation from REPL completes in < 100 microseconds
- Capability enforcement prevents double-allocation of same peripheral in single REPL session
- Panic handler routes through UART (no bare-metal crashes lose debug output)
- All code compiles for both `thumbv7em-none-eabihf` and `riscv32imac-unknown-none-elf` targets


## CODE GENERATION RULES & QUALITY STANDARDS

1. **Zero Placeholder Code:** Do NOT use `// TODO`, `todo!()`, `unimplemented!()`, or truncated code snippets. Every function, struct, and driver implementation must be fully written and ready to compile.
2. **Strict `unsafe` Documentation:** Every `unsafe` block must include a `// SAFETY:` explanatory comment detailing hardware register alignment, volatile memory access semantics, and execution safety.
3. **Zero Dynamic Allocation (`no_alloc`):** Do not use `alloc` or dynamic heap allocation. All ring buffers, capability registries, and execution buffers must reside in static memory or stack frames.
4. **Complete Module Coverage:** Generate full implementations for all modules listed in the repository tree.