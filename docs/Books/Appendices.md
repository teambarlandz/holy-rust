# Appendices

## A. Complete Command Reference (Alphabetical)

| Command | Syntax | Description |
|---------|--------|-------------|
| `peek` | `peek ADDR;` | Read a 32-bit value from the given physical address. Returns the value formatted as `= 0xHEX (decimal)`. Requires the peripheral token to be claimed (unless address falls in unrestricted SRAM/flash). |
| `poke` | `poke ADDR VAL;` | Write a 32-bit value to the given physical address. Volatile write — never elided or reordered by the optimizer. Returns `OK` on success. Requires the peripheral token to be claimed (unless address falls in unrestricted SRAM/flash). |
| `reg_set_bit` | `reg_set_bit ADDR BIT;` | Read-modify-write: set bit `BIT` at memory-mapped address `ADDR`. Reads the register, sets the bit, writes it back. Returns `OK`. Requires capability enforcement. |
| `reg_clr_bit` | `reg_clr_bit ADDR BIT;` | Read-modify-write: clear bit `BIT` at memory-mapped address `ADDR`. Returns `OK`. Requires capability enforcement. |
| `cap_claim` | `cap_claim NAME;` | Claim exclusive ownership of the peripheral token named `NAME`. Valid names: `GPIOA`, `GPIOB`, `UART0`, `SPI0`, `I2C0`, `TIMER0`, `DMA0`, `SUPERUSER`. Returns `CAP CLAIMED <NAME> id=<N>` if free, or `CAP BUSY <NAME>` if already claimed. |
| `cap_drop` | `cap_drop NAME;` | Release ownership of the peripheral token `NAME`. The token returns to the free state. Returns `CAP RELEASED <NAME>`. |
| `let` | `let NAME = EXPR;` | Bind a constant value to a name. `NAME` becomes immutable. The expression `EXPR` is evaluated left-to-right with wrapping arithmetic. Prints `NAME = 0xHEX (decimal)`. |
| `fn` | `fn NAME() { ... };` | Define a callable function body. The body is compiled into the EXEC_BUFFER as native machine code (ARM Thumb-2 or RV32I) or threaded micro-primitives. Printed as `FN NAME DEFINED`. |
| `EXPR` | `EXPR;` | Evaluate a bare expression (left-to-right, no precedence). Supported operators: `+`, `-`, `*`, `/`, `%`. Division and modulo by zero produce `DIV BY ZERO` at parse time. Prints `= 0xHEX (decimal)` or `OK` for side-effect-only streams. |
| `help` | `help;` | Print the full command reference listing all available commands with syntax. |
| `banner` | `banner;` | Reprint the Holy Rust boot banner, including the architecture name and version string. |
| `sys_audit` | `sys_audit;` | Dump the SuperUser audit log. Prints total unsafe operations count, then each entry's address, value, and cycle-count timestamp. The audit log is a 16-entry ring buffer; entries with `addr == 0` are skipped. |
| `sys_info` | `sys_info;` | Print architecture target, CPU frequency, and memory usage statistics. Shows EXEC_BUFFER allocation, active capability count, and total held/available tokens. |
| `sys_caps` | `sys_caps;` | List the current state of all capability tokens (32 total: GPIOA through SUPERUSER). For each, prints whether it is currently claimed or free. |
| `sys_fns` | `sys_fns;` | Display the compiled JIT symbol table. Lists each function's start address in EXEC_BUFFER, its name, and its byte size. |
| `sys_reset` | `sys_reset;` | Flush the instruction cache/pipeline and clear all compiled function symbols from EXEC_BUFFER. Effectively resets the JIT state. |
| `sys_bench` | `sys_bench;` | Microbenchmark: evaluate a given expression N times and report the average cycle count. Useful for measuring primitive operation cost and comparing native vs. threaded dispatch performance. |

## B. ARM Memory Map (from `memory.x`)

| Memory Region | Address Bounds | Size | Type | Purpose |
|--------------|----------------|------|------|---------|
| FLASH (.text) | `0x08000000` - `0x080047FF` | 18 KB | `rx` | Kernel, Lexer, Single-Pass JIT Engine |
| FLASH (.rodata) | `0x08004800` - `0x08004FFF` | 2 KB | `rx` | Jump tables, static strings, constant values |
| SRAM (SYSTEM_BSS) | `0x20000000` - `0x200003FF` | 1 KB | `rwx` | Capability token bitfield, UART RX/TX ring buffers |
| SRAM (EXEC_BUFFER) | `0x20000400` - `0x200013FF` | 4 KB | `rwx` | JIT-compiled native code target buffer |
| SRAM (STACK) | `0x20001400` - `0x20001FFF` | 3 KB | `rwx` | Core execution stack (grows downward) |
| MMIO PERIPHERALS | `0x40000000` - `0x50000000` | 256 MB | — | Hardware control registers |

## C. RISC-V Memory Map (from `memory-riscv.x`)

| Memory Region | Address Bounds | Size | Type | Purpose |
|--------------|----------------|------|------|---------|
| FLASH (.text) | `0x20400000` - `0x2047FFFF` | 512 KB | `rx` | Kernel, JIT engine (QEMU boot ROM target) |
| SRAM (DTIM) | `0x80000000` - `0x800013FF` | 5 KB | `rwx` | Data, BSS, stack, capability registry |
| SRAM (ITIM) | `0x08000000` - `0x08000FFF` | 4 KB | `rwx` | JIT buffer (execute-from-ITIM, tightly-coupled instruction RAM) |
| MMIO PERIPHERALS | `0x10000000` - `0x10FFFFFF` | 256 MB | — | Hardware control registers |

## D. Error Code Table

| Fault Code | Error Category | Root Cause | Resolution Strategy |
|------------|----------------|------------|---------------------|
| E001 | CAPABILITY_VIOLATION | Attempted access to an unclaimed peripheral address. | Run `cap_claim <NAME>` prior to peek or poke. |
| E002 | PERMISSION_DENIED | Unmapped MMIO access without SuperUserCap. | Claim SUPERUSER capability first. |
| LEX | LEX_ERROR | Invalid character or malformed literal encountered during lexing. | Check input syntax; verify no unexpected control characters. |
| UNEXPECTED TOKEN | Syntax Error | Token appeared in a position where it is not valid according to the grammar. | Verify statement structure against the command reference. |
| UNSUPPORTED OPERATOR | Arithmetic Error | An operator other than `+`, `-`, `*`, `/`, `%` was encountered. | Only these five operators are supported in expressions. |
| UNKNOWN SYMBOL | Symbol Error | Reference to a variable or function that has not been defined via `let` or `fn`. | Define the symbol before referencing it. |
| DUPLICATE FN | Definition Error | Attempted to redefine a function that already exists in the symbol table. | Use a different function name; use `sys_reset` to clear the symbol table. |
| SYMBOL TABLE FULL | Resource Error | The 32-slot symbol table is exhausted; no more symbols can be inserted. | Use `sys_reset` to clear the table, or reduce the number of defined symbols. |
| FN TABLE FULL | Resource Error | All 2 function slots are occupied; cannot define another function. | Use `sys_reset` to clear the function table, or reuse existing names. |
| STREAM FULL | Resource Error | The compiled token stream exceeds 128 words; the expression is too complex. | Simplify the expression or split it across multiple REPL lines. |
| NAME TOO LONG | Definition Error | An identifier exceeds the maximum length of 16 bytes. | Use a shorter name. |
| DIV BY ZERO | Arithmetic Error | Division or modulo operation with a zero divisor. Caught at parse time, returns error before execution. | Ensure the divisor is a non-zero constant or variable. |
| MISSING SEMICOLON | Syntax Error | A statement was not terminated with `;`. | Append a semicolon to the end of the statement. |

## E. Capability ID Registry (All CapId Variants)

| CapId | Name | ARM Address Range | RISC-V Address Range | Notes |
|-------|------|-------------------|----------------------|-------|
| 0 | GPIOA | `0x40020000` .. `0x400203FF` | `0x10012000` .. `0x10012FFF` | GPIO Port A, 1 KB register block |
| 1 | GPIOB | `0x40020400` .. `0x400207FF` | `0x10012400` .. `0x10012FFF` | GPIO Port B, 1 KB register block |
| 2 | UART0 | `0x40011000` .. `0x400113FF` | `0x10013000` .. `0x10013FFF` | Universal Asynchronous Receiver-Transmitter |
| 3 | SPI0 | `0x40013000` .. `0x400133FF` | `0x10014000` .. `0x10014FFF` | Serial Peripheral Interface |
| 4 | I2C0 | `0x40015400` .. `0x400157FF` | `0x10020000` .. `0x10020FFF` | Inter-Integrated Circuit |
| 5 | TIMER0 | `0x40000000` .. `0x400003FF` | `0x10015000` .. `0x10015FFF` | General-Purpose Timer |
| 6 | DMA0 | `0x40002000` .. `0x400023FF` | `0x10000000` .. `0x10000FFF` | Direct Memory Access Controller |
| 31 | SUPERUSER | N/A (bypasses all checks) | N/A (bypasses all checks) | Writes logged to audit ring buffer; every write recorded with addr/val/cycles |

## F. Architecture Comparison Table

| Feature | ARM (`thumbv7em-none-eabihf`) | RISC-V (`riscv32imac-unknown-none-elf`) |
|---------|----------------------------------|------------------------------------------|
| Core | Cortex-M4F (ARMv7E-M) | SiFive E310 (RV32IMAC) |
| FPU | Yes (hardware floating-point) | No (integer-only) |
| QEMU Machine | `netduinoplus2` | `sifive_e` |
| Default UART | USART1 @ `0x40011000` | UART0 @ `0x10013000` |
| Default GPIOA | `0x40020000` | `0x10012000` |
| Vector Table | `0x20000400` (SRAM) | `0x80001400` (SRAM) |
| Exec Buffer | `.sram_code` at `0x20002000` | ITIM at `0x08000000` |
| VTOR Register | `0xE000_ED08` | `mtvec` CSR (direct/vectored mode) |
| `fault_hang` | Prints `"**FAULT: core exception, halted"`, wfi loop | `_trap_hang` infinite loop (inline asm) |
| `flush_instruction_cache` | `dsb; isb` | `fence.i` |
| RISC-V native codegen | Enabled | Currently gated (LLD PT_LOAD permissions) |
| Binary size (release, in-memory) | ~17 KB text | ~18 KB text |
| Binary size (release, ELF) | ~141 KB | ~25 KB |
| Manifest compliance | All 12 tests pass | All 12 tests pass |
| REPL command set | Full | Full (identical) |

## G. Binary Size Report

| Target | Build Mode | In-Memory Text | ELF File Size |
|--------|------------|----------------|---------------|
| ARM | `dev` | ~9.2 KB | ~42 KB |
| ARM | `release` | ~15.5 KB | ~141 KB |
| RISC-V | `dev` | ~10.5 KB | ~28 KB |
| RISC-V | `release` | ~18.0 KB | ~25 KB |

*Notes:*
- In-memory text = `.text` + `.rodata` sections that execute from SRAM.
- ELF file size includes section headers, symbol tables, and debug info (not loaded into SRAM).
- `-C opt-level="z" -C lto=true -C codegen-units=1 -panic="abort" - strip = true` produces the smallest binaries.
- Both targets fit within the manifesto's ~16-64 KB SRAM claim when loaded in-memory.

## H. Software Licensing

Holy Rust is released under the **Apache License 2.0** with the explicit exception that the panic handler and UART driver may be relicensed under user-chosen terms for derivative works. The full license text is available in the `LICENSE` file at the repository root.

The project follows the [Rust Language Community Guidelines](https://www.rust-lang.org/conduct) and the [Embedded Rust Working Group](https://github.com/embedded-rust) code of conduct.