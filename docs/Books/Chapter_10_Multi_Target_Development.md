# Chapter 10 — Multi-Target Development

Holy Rust supports two target architectures from a single REPL image: an ARM
Cortex-M4F variant and a RISC-V E310 variant. Both expose the identical
command interface, memory primitives, and REPL semantics, so the developer can
choose the hardware that suits their project without changing application code.

## 10.1 Two Targets, One REPL

The two supported target triplets are:

- `thumbv7em-none-eabihf` — ARM Cortex-M4F with hardware floating-point.
- `riscv32imac-unknown-none-elf` — RISC-V SiFive E310, integer-only core.

A cargo feature flag or `cfg` attribute selects the active target at compile
time. The remaining source code is `#[cfg]gated` behind `target_arch = "arm"`
or `target_arch = "riscv32"`, so building for one architecture excludes the
other's machine-specific data.

## 10.2 ARM Default Configuration

When building for `thumbv7em-none-eabihf`, the default QEMU machine is
`netduinoplus2`. Peripheral addresses in the ARM memory map are:

| Peripheral | Address       |
|------------|---------------|
| GPIOA      | `0x40020000`  |
| GPIOB      | `0x40020400`  |
| UART1      | `0x40011000`  |
| FLASH      | `0x08000000`  |

The default machine type in QEMU exposes a STM32-style USART1 and GPIO ports
at these fixed addresses. The memory layout is defined in `memory.x`.

## 10.3 RISC-V Default Configuration

When building for `riscv32imac-unknown-none-elf`, the default QEMU machine is
`sifive-e`. Peripheral addresses in the RISC-V memory map are:

| Peripheral | Address       |
|------------|---------------|
| GPIOA      | `0x10012000`  |
| UART0      | `0x10013000`  |
| SPI0       | `0x10014000`  |

The sifive-e QEMU model provides a SiFive E310-compatible address map. Memory
definitions live in `memory-riscv.x`.

## 10.4 Custom Memory Maps Per Architecture

Memory regions are not shared between targets. Each architecture has its own
linker script:

- `memory.x` — ARM Cortex-M4F memory map.
- `memory-riscv.x` — RISC-V E310 memory map.

These scripts place the vector table, SRAM regions, and executable code at
architecture-appropriate addresses. The linker respects the `target_arch`
configuration and picks the correct script automatically.

## 10.5 Peripheral Address Maps Differ

The physical addresses of peripherals differ between ARM and RISC-V because the
two CPU families use entirely separate address spaces:

| Peripheral | ARM Address   | RISC-V Address   |
|------------|---------------|------------------|
| GPIOA      | `0x40020000`  | `0x10012000`     |
| GPIOB      | `0x40020400`  | `0x10012400`     |
| UART0      | `0x40011000`  | `0x10013000`     |
| UART1      | `0x40011000`  | N/A              |
| SPI0       | `0x40013000`  | `0x10014000`     |
| I2C0       | `0x40015400`  | `0x10020000`     |
| Timer0     | `0x40000000`  | `0x10015000`     |
| DMA0       | `0x40002000`  | `0x10000000`     |

These addresses are hardcoded in the `poke`/`peek` primitives and in the
capability ID registry (see Appendix).

## 10.6 Target-Specific `addr_to_cap_id()`

The function `addr_to_cap_id()` has architecture-specific implementations:

- The ARM version maps an address to a `CapId` by comparing against the ARM
  peripheral base addresses (GPIOA at `0x40020000`, etc.).
- The RISC-V version performs the same mapping against the RISC-V base
  addresses (GPIOA at `0x10012000`, etc.).

Both implementations return the same `CapId` enum variant for the same
peripheral name, but the underlying address comparison is target-specific.

## 10.7 RISC-V Native Codegen Gating

The RISC-V backend currently emits ITIM (instruction‑timed memory) regions in
`RW-only` PT_LOAD program headers. QEMU enforces execute‑permission checks on
these segments, which means pure LLVM codegen can produce a binary that QEMU
refuses to execute.

## 10.8 Workaround: GNU ld or post-link patching

To add the `PF_X` (execute) flag to the ITIM segment, two approaches are
available:

1. Use GNU `ld` scripts to rewrite the program header flags after codegen.
2. Apply `llvm-objcopy` post-link to patch the PT_LOAD header and set the
   execute bit.

Both approaches are encapsulated behind a build-time feature flag so the
default cargo build succeeds on both targets.

## 10.9 Identical REPL Command Set Across Targets

Regardless of whether the active target is ARM or RISC-V, the REPL supports
the exact same command set:

- `peek ADDR;` — read a u32 from address
- `poke ADDR VAL;` — write a u32 to address
- `reg_set_bit ADDR BIT;` — set a register bit (RMW)
- `reg_clr_bit ADDR BIT;` — clear a register bit (RMW)
- `cap_claim NAME;` — claim a peripheral token
- `cap_drop NAME;` — release a peripheral token
- `let NAME = EXPR;` — bind a constant
- `fn NAME() { ... };` — define a callable body
- `EXPR;` — evaluate and print result
- `help;`, `banner;`, `sys_audit;`, `sys_info;`, `sys_caps;`, `sys_fns;`,
  `sys_reset;`, `sys_bench;`

## 10.10 Binary Sizes

Release-mode binary sizes differ slightly between targets due to architecture‑
specific code size:

| Target    | In-Memory Text | ELF Size |
|-----------|----------------|----------|
| ARM       | ~17 KB         | ~141 KB  |
| RISC-V    | ~18 KB         | ~25 KB   |

Debug builds are larger on both targets but share the same structural layout.

## 10.11 Manifestio Compliance

Both targets pass the exact same 12 manifesto compliance tests, which verify:

- Correct `peek`/`poke` behavior at all mapped addresses.
- Capability token claiming and release.
- JIT function compilation and execution.
- Interrupt vector table presence and correctness.
- Memory‑map adherence from the linker script.
- Fault handler availability.
- REPL startup and shutdown cleanliness.
- Binary size within expected bounds.
- Command syntax validation.
- Architecture‑specific `cfg` gate operation.

## 10.12 Choosing the Architecture

The code is fully `#[cfg]gated`: at compile time exactly one target is active,
and all peripheral addresses, vector table layouts, and memory regions are
selected accordingly. The developer chooses the architecture that matches their
hardware by setting the appropriate cargo target triple; the REPL command set,
JIT semantics, and capability model remain identical regardless of target.