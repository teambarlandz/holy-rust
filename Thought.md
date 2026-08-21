# Thought.md — Holy Rust Engineering Log

Running design log for the implementation work. Newest entries at the bottom.
This file records *why* things are built the way they are, including every
deviation from `docs/` and `RoadMap.md` and the reasoning behind it.

---

## 0. Machine capability assessment (can this box do the job?)

- Toolchain found: `rustc 1.97.1` / `cargo 1.97.1` (aarch64 host).
- Cross targets were **not** installed initially; installed via
  `rustup target add thumbv7em-none-eabihf riscv32imac-unknown-none-elf`. Both now present.
- QEMU (`qemu-system-arm`, `qemu-system-riscv32`) **not installed**. Builds are
  fully possible; runtime emulation needs a later `apt install qemu-system-*`
  attempt. Development is not blocked.
- Verdict: **yes, this machine can handle the project** — it is a pure
  cross-compilation workload; the heavy lifting is rustc + lld, both local.

## 1. Repository state on arrival

- Only scaffolding existed: `Cargo.toml` (broken), `build.rs`, `memory.x`
  (MEMORY block only, no SECTIONS), `.cargo/config.toml` (broken runners),
  docs suite, RoadMap. All four `src/` subdirectories were **empty**.
- First `cargo build` failed twice:
  1. `Cargo.toml:29` → `[replace-with]` / bare `github` is invalid TOML. Removed.
  2. "no targets specified" → no `src/lib.rs`/`src/main.rs`. This kicked off
     the full implementation.

## 2. Cargo.toml repairs

Problems found beyond `[replace-with]`:

- `core = "1.0"` as a dependency is invalid — `core` is a built-in sysroot
  crate, not on crates.io. Removed.
- `[targets]` table is not valid Cargo manifest syntax. Removed.
- Kept `embedded-hal = "1.0"` (trait-only crate, no_std-safe). We use only
  `embedded_hal::digital::{OutputPin, ErrorType, Error, ErrorKind}` — paths I
  am confident exist in 1.x. UART traits deliberately NOT used (API surface
  churn risk); our UART driver exposes its own polling API.
- Added `[profile.release]` with `opt-level="z"`, lto, codegen-units=1.

## 3. Target silicon choice (QEMU-first requirement)

The docs fix Flash at `0x0800_0000` (STM32 style) and SRAM at `0x2000_0000`.
To honor "QEMU first" without breaking the documented map:

- **ARM**: `-M netduinoplus2` (STM32F405, Cortex-M4F — exactly matches
  `thumbv7em-none-eabihf`). QEMU's stm32f4xx SoC maps flash at `0x08000000`
  *and* aliases it at `0x00000000`, so the vector table at ORIGIN(flash)
  works. UART = STM32 USART1 @ `0x40011000`: SR(0x00) TXE=bit7 RXNE=bit5,
  DR(0x04), CR1(0x0C) UE=bit13 TE=bit3 RE=bit2. QEMU's usart model only
  transmits when UE|TE are set — init must set them.
- **RISC-V**: `-machine sifive_e` (SiFive E310, RV32IMAC — matches
  `riscv32imac`). Flash XIP @ `0x20000000`; RAM = DTIM @ `0x80000000`
  (declared conservatively as 16K in the linker script); UART0 @
  `0x10013000`: txdata(0x00) write-to-send, rxdata(0x04) read with bit31 =
  empty flag. No init required in QEMU.
- Original `.cargo/config.toml` was broken: `-M std` is not a machine,
  `three-stage-sifive` is not a board, and `runner-args` / `rustc` keys don't
  exist. Replaced with proper single-string `runner` entries (cargo appends
  the ELF path automatically).

## 4. One linker script per arch, shared layout

One `memory.x` cannot serve both targets (different RAM origins). Solution:

- `memory.x` (ARM) and `memory-riscv.x` define identically-named regions
  (`flash`, `sram`) plus three base-address symbols:
  `__capreg_base`, `__sram_vectors_base`, `__sram_code_base`.
- `memory-layout.x` holds the shared `SECTIONS` + `ENTRY(Reset)` and is
  pulled in via ld `INCLUDE` from both memory files.
- `build.rs` selects the script from `CARGO_CFG_TARGET_ARCH`
  (`riscv32` → riscv script, else ARM), validates it contains
  `ORIGIN`/`LENGTH`/`INCLUDE memory-layout.x`, and emits
  `cargo:rustc-link-arg=-T<script>` (+ rerun-if-changed).

Section layout decisions:

- `.isr_vector` words are emitted **by the linker**:
  `LONG(_stack_top)` + `LONG(Reset + 1)`. Doing it in Rust would require
  const fn-pointer→int casts (fragile across rustc versions) and manual
  Thumb-bit handling; the linker does both trivially (`+1` sets the Thumb
  bit for Cortex-M).
- Carved SRAM subsections are `(NOLOAD)` explicit-address sections:
  `.sram_vectors` @ sram+0x400 (roadmap: 0x2000_0400),
  `.capability_registry` @ sram+0x1000 (doc ch.2: 0x2000_1000),
  `.sram_code` @ sram+0x2000 (memory.x original). `.data`/`.bss` start at
  ORIGIN(sram) and stay well below 0x400 used bytes, so no overlap.
- `_stack_top = ORIGIN(sram) + LENGTH(sram)`; descending full stack.
- `__global_pointer$ = __sdata + 0x800` defined for RISC-V gp relaxation;
  startup initializes `gp` inside a `.option norelax` window.
- `/DISCARD/` for `.eh_frame*` / `.comment*`.

## 5. Startup strategy (no rt crates)

Deliberately **zero external runtime deps** (no cortex-m-rt / riscv-rt) so
both targets build from one hand-rolled entry:

- ARM: CPU/QEMU loads SP from vector[0] before entering `Reset`, so Reset is
  a plain Rust fn: `init_data_bss()` then `boot()`.
- RISC-V: SP/gp are NOT set by the loader → `#[naked] extern "C" fn Reset`
  runs raw asm (`la sp, _stack_top`, `la gp, __global_pointer$` under
  norelax) then tail-calls `rust_boot_riscv`. `#[naked]` is stable on
  rustc 1.97.
- `init_data_bss()` (kernel/memory.rs): volatile copy `.data` LMA→VMA using
  `__sidata`, volatile zero `.bss`. All symbols accessed via
  `addr_of!`/raw pointers to avoid `static_mut_refs` issues.
- FPU note: eabihf ABI but we emit no float instructions, so leaving
  CPACR/FPSCR untouched is safe (documented; cortex-m-rt normally enables it).

## 6. Kernel modules

### kernel/memory.rs
- `peek_u32` / `poke_u32` volatile wrappers (REPL primitives, doc ch.5).
- `reg_set_bit` / `reg_clr_bit`: **deviation** — doc wrote plain
  `write(1<<bit)` which clobbers all other bits in the register; we do a
  volatile RMW (read-modify-write) instead. Safer and still O(1).

### kernel/exec.rs
- `EXEC_BUFFER: [u8; 4096]` in `.sram_code`, `#[repr(align(4))]`.
- `exec_buffer_entry()` transmutes the buffer base into `fn() -> u32`.
  **Critical detail**: on ARM the byte address must get the Thumb bit
  (`addr | 1`) before transmute or `BX` faults. cfg-gated.
- Threaded dispatch `run_threaded_stream(ip)`: fetch word → advance IP →
  transmute word to `MicroPrimitive` → call → loop until null IP. Defensive
  extra break if a fetched word itself is 0 (corrupt stream protection).
- VM stack: static `[usize; 64]` + index. Overflow silently drops the pushed
  value, underflow pops 0 — **no panics allowed in the hot loop** (rule:
  panic handler exists but REPL must never need it). Documented behavior.

### kernel/interrupt.rs
- `VECTOR_TABLE: [u32; 256]` in `.sram_vectors`, `#[repr(align(1024))]`
  (VTOR alignment safety margin).
- `boot_relocate_vectors()`:
  - ARM: copies the flash vector words (`__vector_start..__vector_end`) into
    the SRAM table, then writes VTOR (`0xE000ED08`) with its address.
    Roadmap M2 checklist satisfied.
  - RISC-V: points `mtvec` (direct mode) at a `_trap_hang` stub defined via
    `global_asm!` (`j _trap_hang`). Vectored mode deferred until real IRQ
    bring-up (needs per-slot asm stubs).
- `set_handler(slot, Option<fn>)` stores `fn as usize` (compiler sets Thumb
  bit automatically on ARM when casting fn→usize).
- `generic_irq_trampoline_ch16()` kept from doc ch.4 as the C-ABI thunk
  example (ack pending reg, dispatch slot 16).

## 7. Capability engine

### capabilities/registry.rs
- `REGISTRY_BITS: [AtomicU32; 8]` (256 resources) placed in
  `.capability_registry` section → lands at SRAM+0x1000 per doc ch.2.
- **Deviation**: doc uses plain RMW on raw memory; we use atomics
  (`fetch_or` test-and-set for acquire, `fetch_and` release, `load`
  available). Same O(1) cost, but interrupt-safe without critical sections.
- acquire returns bool (false = already claimed).

### capabilities/tokens.rs
- `Cap<T>` is non-Copy/non-Clone **by omission** — negative impls
  (`impl !Copy`) are unstable; the doc's snippet wouldn't compile on stable
  anyway. Not deriving Copy/Clone gives identical compile errors on misuse.
- No `Drop` impl: linear ownership means explicit `drop_cap(cap)` consumes
  the token and releases the bit. A Drop impl would risk double-release
  paths and hidden releases; linearity here is *explicit by design*.
- `HardwareResource { RESOURCE_ID, NAME }`; resources: GPIOA(0), GPIOB(1),
  UART0(2), SPI0(3), I2C0(4), TIMER0(5), DMA0(6), SUPERUSER(31).
- GPIO model: virtual port contract (SET@+0x00, CLR@+0x04, OUT@+0x08,
  DIR@+0x10, bit-per-pin) — an explicit Holy HAL contract for QEMU bring-up
  until a real PAC lands (M5+ hardware milestone). Documented as such.
- `PinGuard<'a, T, N>` lease returned by `cap.pin::<N>()` — borrow ends at
  scope close (doc's "borrow-lease token"). Inherent linear methods follow
  the roadmap signature `set_high(self) -> Self` (consume-and-return);
  embedded-hal 1.0 `OutputPin` implemented on the same guard with `&mut self`
  for generic driver interop. Both coexist; inherent wins for method-call
  syntax.
- `SuperUserCap` granted at boot path only; audit counter increments per
  grant (doc ch.2 rule 2/5, minimal viable version).

## 8. Streaming JIT compiler

### compiler/lexer.rs
- `Lexer<'a>` over `&'a [u8]`, cursor + line counter, zero alloc.
- **Deviation**: doc's `Identifier(&'static str)` is impossible for REPL
  input (nothing is 'static); tokens borrow slices of the input buffer
  instead (`Token<'a>`, `Identifier(&'a [u8])`). Still zero-copy/zero-alloc.
- Numbers: decimal + `0x` hex + `_` separators; overflow → `Token::Error`.
- Punctuation variants (LParen/RParen/LBrace/RBrace/Semicolon/Comma) added —
  the grammar needs them; doc's sketch had a stray `Token::Punctuation(b)`
  comment acknowledging the gap.

### compiler/primitives.rs
- `MicroPrimitive = fn(ip: *const usize) -> *const usize` per doc ch.3.
- Prims: `lit_prim` (push next word), `load_reg_prim` (addr arg → push
  volatile read), `write_reg_prim` (addr arg, pop → volatile write),
  `add/sub/mul/div`, `halt_prim` (returns null → dispatch stops).
- div-by-zero pushes 0 (deterministic, no traps in Ring 0 threaded mode).

### compiler/emitter.rs
Doc's sample encodings were **buggy** (wrong bit packing in both ISAs), so
correct encodings were derived:

- Thumb-2:
  - MOVW hw1 = `0xF240 | imm4 | (i<<10)`, hw2 = `imm3<<12 | Rd<<8 | imm8`;
    MOVT same with base `0xF2C0` and upper-half fields. MOVW alone zeroes
    the top half, so MOVT emitted only when `imm >> 16 != 0`.
  - Fast path `MOVS Rd, #imm8` = `0x2000 | Rd<<8 | imm8` for ≤255.
  - STR/LDR T1: `0x6000/0x6800 | (imm5<<6) | Rn<<3 | Rt` (low regs, word
    offset/4).
  - ADDS/SUBS reg: `0x1800/0x1A00 | Rm<<6 | Rn<<3 | Rd`.
  - RET = `BX LR` = `0x4770`.
- RV32I:
  - LUI+ADDI pair with standard hi20 = `(imm + 0x800) >> 12` rounding so the
    ADDI sign-extension cancels; LUI skipped when value fits signed 12-bit.
  - SW/LW offset split imm[11:5]/imm[4:0]; ADD/SUB R-type; RET =
    `JALR x0, ra, 0` = `0x00008067`.
- Emitters write through raw cursors into EXEC_BUFFER with capacity checks
  returning `Result<(), EmitError>` (**deviation**: doc's version writes
  through unchecked pointers and would silently corrupt past the buffer).
- Pure encoding helpers exposed for future host-side unit tests.
- Some emitter API is ahead of the REPL path (native-codegen verification is
  Milestone 4's checklist) → targeted `#[allow(dead_code)]` with comments,
  not placeholders.

### compiler/parser.rs
- Persistent `Compiler` struct (symbol table + fn-body storage + stream
  buffer) owned by the REPL as a `static mut` — fn bodies must survive
  across REPL lines, so a stack-local parser won't do. Single-threaded REPL
  makes this sound; SAFETY documented at use site.
- Symbol table: 64 slots, open addressing, FNV-1a hash, names inline
  `[u8; 16]`. Fn bodies: 4 fns × 64 words. Stream: 256 words. Total ~4 KB
  static RAM — fits the 16 KB RISC-V budget alongside everything else.
- Grammar (LL(1), left-to-right eval, no precedence — per manifesto):
  - `let NAME = expr ;`
  - `fn NAME ( ) { stmt* }` — body stored WITHOUT trailing halt so bodies
    can be spliced at call sites; standalone call appends halt.
  - `poke expr expr ;` / `peek expr ;`
  - `cap_claim NAME ;` / `cap_drop NAME ;`
  - `reg_set_bit expr expr ;` / `reg_clr_bit expr expr ;`
  - `NAME ( ) ;` (call) / bare `expr ;` (evaluate & print)
  - `help` / `banner`
- Two evaluation modes over shared term parsing: constant-fold (immediate
  print, catches div-by-zero at parse time) and emit-threaded-tokens
  (execution). Left-assoc sequential emission == left-to-right semantics.
- Output: `Outcome` enum consumed by the REPL (`Bound`, `FnDefined`,
  `Run(StreamProgram)`, `Claim/Drop(NameBuf)`, `SetBit/ClrBit`, `Help`,
  `Banner`). `StreamProgram::run()` resets VM, dispatches, pops result if
  `yields_value`.

## 9. Drivers

### drivers/uart.rs
- Arch-cfg MMIO constants (see §3). `init()` sets STM32 CR1 UE|TE|RE; no-op
  on SiFive.
- Blocking `put_byte` (TXE poll), non-blocking `poll_get_byte` (RXNE /
  rxdata-bit31).
- Zero-alloc formatters: `write_str`, `write_hex_u32` ("0x" + nibbles),
  `write_dec_u32` (reverse-fill `[u8;10]` on stack).
- 256-byte SPSC RX ring + `irq_handler()` stub per roadmap M5 (polling is
  the primary path; IRQ mode wired later). Plain static mut ring, SPSC
  assumption documented.

### drivers/repl.rs
- State machine `Idle → Reading → Evaluating → Printing` (doc ch.5).
- Line buffer 128 B static; echo; backspace (`\b \b`); Ctrl-U kill line;
  Ctrl-C cancel; CR/LF submit; printable range 0x20–0x7E appended+echoed.
- `evaluate()` → parser → match Outcome:
  - Run(program): prints `= 0x........ (dec)` when it yields a value, `OK`
    for poke-class programs.
  - Claim/Drop: resolve name → registry ops → `CAP CLAIMED/BUSY/RELEASED/
    NOT HELD <NAME> id=N`.
  - SetBit/ClrBit → volatile RMW → `OK`.
  - Bound/FnDefined/Help/Banner printouts.
- Prompt `\r\nholy> `.

## 10. main.rs

- `#![no_std] #![no_main]`; bin owns the `#[panic_handler]`: prints
  `PANIC:` + payload (downcast `&str`; `PanicInfo::message()` avoided —
  unstable historically), then `wfi` loop (mnemonic valid on both arches).
- Per-arch `Reset` (§5) → `boot()`: uart init → banner →
  `interrupt::boot_relocate_vectors()` → `repl::run()` (never returns).

## 11. Open risks / follow-ups

- ~~QEMU not yet installed; runner configs untested until then.~~
  Resolved §12: QEMU 8.2.2 installed, both targets boot and pass REPL
  smoke tests.
- ~~SiFive DTIM size assumed ≥16 KB; linker LENGTH set to 16 K to be safe.~~
  Resolved §12: QEMU's sifive_e DTIM is 8 K; map re-carved accordingly.
- Real PAC integration (stm32f4xx-hal / rp2040-hal) intentionally deferred —
  current GPIO contract is the Holy HAL virtual model.
- Host-side unit tests for lexer/parser/emitters planned via the `std`
  feature gate already present in lib.rs.

## 12. Bring-up debugging log (first silicon, er, simulator)

The jump from "compiles for four configs" to "boots in QEMU" surfaced five
bugs, each educational:

### 12.1 Thumb-bit double-tagging (ARM boot failure #1)

Symptom: QEMU loaded SP/PC correctly from our vector table, then took an
immediate UsageFault (UFSR.UNALIGNED) at the entry PC, escalated through a
garbage HardFault vector into lockup.

Root cause: ELF function symbols for Thumb code **already carry the Thumb
bit** in `st_value` (Reset linked at 0x08000009). My linker script emitted
`LONG(Reset + 1)` — adding 1 to an already-odd symbol CLEARED the bit,
handing the core an even address (0x0800000A). The core booted in ARM state,
where a halfword-aligned fetch is by definition unaligned.

Fix: `LONG(Reset)` verbatim. Lesson: on ARM ELF, interworking bit comes for
free; never re-apply it.

### 12.2 Primitive calling-convention split (silent data corruption)

Symptom: `peek ADDR;` returned the same value for every address — it was
dereferencing `halt_prim`'s own machine code as a data pointer.

Root cause: I had mixed two conventions. `lit_prim` pushed arguments onto
the VM operand stack (RPN style), but `load/write_reg_prim` tried to read
an inline argument word past their opcode slot — which was actually the
NEXT primitive pointer.

Fix: all primitives are now purely stack-based (`load` pops addr/pushes
value; `write` pops value/addr). Uniform RPN streams; no inline args except
literals. This also shrank every stream by one word per op.

### 12.3 Parser double-consume (leading-token bug)

Symptom: `2+3;` → ERR UNEXPECTED TOKEN while `let x = 2+3;` worked.

Root cause: `parse()` matched on `cur.next()` then called `cur.next()`
AGAIN inside the expression arm, dropping the leading literal and feeding
the operator to `resolve_term`.

Fix: bind the consumed token first, pass it as `eval_expr(Some(first))`.

### 12.4 QEMU sifive_e boot ROM ignores the ELF entry point

Symptom: RISC-V image linked at 0x2000_0000 (per FE310 manual) never ran;
trace showed the boot ROM executing `lui t0, 0x20400; jr t0`.

Root cause: QEMU's sifive_e reset stub jumps to the flash CONTROLLER base
(0x2040_0000); 0x2000_0000 is only the XIP alias window. Also, my ARM-style
`.isr_vector` sat at the flash origin, so the jump landed on vector-table
DATA executed as code.

Fixes:
- Link RISC-V code at 0x2040_0000 so ROM jump == image start.
- Split section layouts per arch: `memory-layout.x` (ARM, emits the 16-slot
  core vector table) vs `memory-layout-riscv.x` (no vector table; Reset must
  be first). build.rs selects and validates the pair.
- DTIM is 8 K in this machine: re-carved as sram 5 K @ 0x8000_0000 +
  vectors 1 K + registry 256 B; the 4 K JIT buffer moved to the ITIM
  (@ 0x0800_0000) — tightly-coupled instruction RAM, arguably the more
  honest home for generated code anyway.

### 12.5 Fault visibility (lockup → announced halt)

Symptom: `peek <unmapped>;` hard-locked QEMU with no output.

Root cause: `.isr_vector` carried only SP+Reset; slots 2..15 read as zeros,
so any fault jumped to address 0 and escalated to lockup.

Fix: linker script routes NMI/HardFault/MemManage/BusFault/UsageFault/
SVCall/DebugMon/PendSV/SysTick to a Rust `fault_hang` that announces over
UART (`**FAULT: core exception, halted**`) and sleeps in `wfi`. Ring 0 has
no safety net below it, but failures should at least be *visible*.

### 12.6 Grammar decision: peek as compile-time term

`let y = peek ADDR;` initially failed (peek wasn't an expression term).
Resolution: symbols are immutable constants per the manifesto, so peek in
expression position evaluates AT COMPILE TIME and binds the constant result.
Left-to-right precedence applies uniformly: `peek A + 1` reads address A+1
(byte offset), NOT value+1 — verified consistent between statement and
expression forms. Value arithmetic chains through bindings
(`let t = peek A; t + 1;`). Parenthesized grouping remains unsupported by
design (manifesto rejects precedence; parens would smuggle it back in).

### 12.7 Verification status

All four build configs clean (zero clippy warnings, rustfmt-clean).
QEMU smoke tests pass on both targets: banner, arithmetic, let/fn/call,
poke/peek roundtrip, reg_set_bit/clr_bit, cap_claim→BUSY→cap_drop,
div-by-zero and unknown-symbol errors, help, graceful fault on wild access.

