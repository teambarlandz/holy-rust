# SYSTEM BOUNDARIES & ECOSYSTEM

## Domain Separation: Host PC vs. Target Silicon

### Explicit Boundary Matrix

Defines what runs on the Host PC versus what runs inside Target Silicon in Ring 0.

| Domain | Host PC (Development) | Target Silicon (Execution) |
|---|---|---|
| **Operating System** | Linux, macOS, or Windows | Bare-metal (no OS) |
| **Runtime Environment** | `std` full Rust standard library | `#![no_std]` bare-metal |
| **Memory Model** | Virtual memory with MMU, paging | Physical memory, 1:1 identity mapping |
| **Privilege Rings** | Ring 3 (user) / Ring 0 (kernel) | Ring 0 only - no privilege boundary |
| **Compilation** | AOT compilation with rustc | Single-pass streaming JIT at runtime |
| **Safety Model** | OS-enforced memory protection | Compile-time capability tokens (O(1) verification) |
| **Hardware Access** | syscall / /dev interface | Direct memory-mapped I/O (MMIO) |
| **Linker/Loader** | ELF loader, dynamic linker | Static linking, no dynamic loading |
| **Interrupt Handling** | Kernel IRQ dispatcher | C-ABI trampoline thunks in SRAM |
| **REPL/SHELL** | Terminal emulator (picocom, minicom) | In-Ring 0 shell, direct hardware access |
| **Memory Footprint** | Megabytes to gigabytes | ~16 KB to 64 KB total |
| **Target Architecture** | x86_64, aarch64 (development host) | ARM Cortex-M, RISC-V (target silicon) |

### Host PC Side (Development Environment)

#### Toolchain

```bash
# Install Rust toolchain
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Add embedded target support
rustup target add riscv32imac-unknown-none-elf
rustup target add thumbv7em-none-eabihf

# Install debug/probe tools
cargo install probe-rs  # or: pip install openocd
picocom -b 115200 /dev/ttyACM0  # UART console

# Build Holy Rust for target
cargo build --target riscv32imac-unknown-none-elf

# Flash to target hardware
probe-rs target flash --chip rp2040

# Monitor runtime output
picocom -b 115200 /dev/ttyACM0
```

#### Host-Side Build Pipeline

```text
[ Source Code (.rs) ]
       │
       ▼
[ cargo build --target <target> ]
       │
       ├─ Compiles host-side infrastructure (JIT kernel, PAC bindings)
       │   using rustc with #![no_std] + thumb mode
       │
       ├─ Produces ELF binary
       │   - .text: JIT kernel code
       │   - .rodata: Micro-primitive flash functions
       │   - .data: Static capability registry
       │   - .bss: Zero-initialized data
       │
       └─ Produces hex image (via probe-rs or objcopy)
            │
            ▼
[ Target Silicon ]
```

#### Host-Side Crates & Dependencies

```toml
# Cargo.toml for holy-rust host-side build
[package]
name = "holy-rust"
version = "0.1.0"
edition = "2021"

[dependencies]
# No std dependencies on host build - only host tools
# embedded-hal for trait definitions (conditional)
# probe-rs for flashing
# serde for serialization (host-side only)

[targets]
# Architecture-specific target configurations
```

### Target Silicon Side (Ring 0 Execution)

#### Memory Layout (Static, Known at Link Time)

```text
+----------------------+----------------------+---------------------+
| Vector Table         | Capability Registry  | JIT Execution Buffer|
+----------------------+----------------------+---------------------+
| 0x2000_0400          | 0x2000_1000          | 0x2000_2000         |
| 1 KB (256 x 4 bytes) | 8 KB (bitmask + tokens)| 16 KB (configurable)|
+----------------------+----------------------+---------------------+
```

#### No-Standard Runtime (`#![no_std]`)

The JIT kernel is bootstrapped using `#![no_std]` Rust, leveraging existing
Peripheral Access Crates (PACs), `embedded-hal` traits, and vendor HALs for
register definitions. There is no operating system, no process isolation, and no
runtime library.

```rust
// Entry point - called after vector table is set up
#![no_std]
#![no_main]

use holy_rust_hal::prelude::*;

/// System initialization - called once at boot
#[no_mangle]
pub unsafe fn holy_rust_target_init() -> HardwareCapabilities {
    // 1. Initialize SRAM vector table (relocate from Flash to SRAM)
    init_sram_vector_table();
    
    // 2. Enable capability bitmap (clear all bits - no peripherals claimed)
    init_capability_bitmap();
    
    // 3. Register base capability tokens for all system peripherals
    HardwareCapabilities {
        gpio: Cap::new_unchecked(),
        uart: Cap::new_unchecked(),
        spi:  Cap::new_unchecked(),
        timer: Cap::new_unchecked(),
    }
}

/// Main loop - never returns (like !main)
#[no_mangle]
pub fn holy_rust_repl_loop() {
    // 1. Poll UART/USB for incoming character stream
    // 2. Accumulate characters in fixed-size ring buffer
    // 3. On line-break: tokenize, verify capabilities, emit SRAM tokens
    // 4. Jump to SRAM execution buffer
    // 5. Return to poll loop (< 1 microsecond total)
    
    loop {
        // Non-blocking UART polling
        if let Some(byte) = uart_poll_get_byte() {
            // Accumulate into input buffer
        }
        
        // Check if line complete (newline received)
        // If so: compile and execute
        // Immediately return to poll loop
    }
}
```

#### Capability Registry (SRAM Bitmask)

```text
+----------------------+-------------------------------+
| Bitmask Base: 0x2000_1000 |
+----------------------+-------------------------------+
| Word 0: Resources [0-31]  | Bit i = 1 => claimed, 0 => free |
+----------------------+-------------------------------+
| Word 1: Resources [32-63] | Bit i = 1 => claimed, 0 => free |
+----------------------+-------------------------------+
| ...                          |
+----------------------+-------------------------------+
```

#### Interrupt Vector Table (Relocated to SRAM)

```text
+----------------------+----------------------+----------------------+
| VTOR-Relocated Table | IRQ 0-255 Handlers     | Thunk Dispatch Slots |
+----------------------+----------------------+----------------------+
| SRAM Address: 0x2000_0400 | [fn() option; 256]   | [Option<fn()>; 64]   |
+----------------------+----------------------+----------------------+
```

### Compilation Model: Host-Target Separation

#### Why `rustc` is Used Once on the Host

A common point of confusion: Holy Rust uses `rustc` (the Rust compiler) on the
host PC, but the JIT kernel that runs on the target chip operates completely
standalone. The compilation model is separated into two distinct phases:

```text
+─────────────────────────────────────────────────────────────────+
|  PHASE 1: HOST COMPRODUCTION (once, at development time)       |
|  • rustc compiles the JIT kernel                               |
|  • Produces ELF binary for target architecture                   |
|  • Includes:                                                   |
|    - Capability token registry implementation                    |
|    - Micro-primitive Flash functions (hand-crafted assembly)     |
|    - SRAM vector table relocations                               |
|    - HAL/PAC bindings for target chip                            |
|    - Interrupt trampoline thunks                                 |
|  • Output: holy_rust_kernel.elf                                |
|  • Output: holy_rust_kernel.hex (flashed once)                 |
+─────────────────────────────────────────────────────────────────+
                               │
                               │ flashed to target once at setup
                               ▼
+─────────────────────────────────────────────────────────────────+
|  PHASE 2: TARGET RUNTIME (every REPL session, at execution time) |
|  • No rustc on target                                            |
|  • No OS on target                                               |
|  • Streaming JIT: text stream ──► tokens ──► SRAM execution      |
|  • Single-pass tokenizer: O(1) verification, no AST/MIR          |
|  • Micro-primitive dispatch: direct function calls to Flash .rodata|
|  • Capability bitmap: single-bit lookups, O(1) safety            |
|  • Execution: threaded micro-primitives, <100us compilation       |
+─────────────────────────────────────────────────────────────────+
```

#### Detailed Walkthrough

**Phase 1: Host Production (One-Time)**
1. Developers write Holy Rust application code and JIT kernel code on the Host PC
2. `cargo build --target <riscv32imac-unknown-none-elf>` invokes `rustc`
3. `rustc` compiles the JIT kernel with `#![no_std]` and target-specific flags
4. The compiled ELF includes:
   - The capability engine implementation (static bitmask, token structures)
   - The streaming JIT tokenizer (single-pass, zero-heap)
   - Micro-primitive functions compiled to Thumb-2/RISC-V assembly and placed in `.rodata`
   - The interrupt trampoline thunks (C-ABI compatible)
   - HAL/PAC abstractions for the target chip
5. The binary is flashed to target silicon once (via probe-rs, openOCD)
6. From this point forward, `rustc` is NOT needed on the target

**Phase 2: Target Runtime (Every REPL Session)**
1. Target powers on; vector table initialized; capability bitmap cleared
2. REPL session begins (UART/USB connected, picocom or similar terminal)
3. User types Holy Rust source code into the terminal
4. Character stream received over UART into fixed-size ring buffer
5. Upon line-break: tokenizer runs in a single pass, no heap allocation
6. Tokens pass through capability verifier (O(1) bitmask checks)
7. Validated tokens emitted directly into executable SRAM buffer
8. CPU jumps to SRAM buffer; execution begins at hardware speed
9. No `rustc` involvement, no OS syscalls, no context switches
10. When user types next command: steps 3-9 repeat

#### Why This Model Works

- **Zero Runtime Compiler on Target**: The heavy compiler (rustc, AST construction,
  MIR generation, LLVM IR emission) runs only on the Host PC, once. The target
  only contains a lean tokenizer and dispatcher.

- **O(1) Safety Verification**: The capability bitmap provides constant-time safety
  checks, unlike standard Rust's O(N^2) lifetime analysis which is infeasible in
  a resource-constrained JIT context.

- **Deterministic Real-Time**: No garbage collector, no scheduler, no page-fault
  handlers. Execution timing is transparent and microsecond-accurate.

- **Hardware Safety Without MMU**: The linear capability model provides memory safety
  guarantees that would normally require an MMU and kernel-enforced protection.
- **Single-Address-Space Simplicity**: No virtual-to-physical address translation
  means no TLB misses, no page faults, deterministic memory access latency.

#### Ecosystem Integration

Holy Rust integrates with the existing Rust ecosystem through strict `no_std`
compliance:

```toml
# Example: Using embedded-hal traits with Holy Rust capabilities

[dependencies]
embedded-hal = "1.0"
# Vendor PACs (Peripheral Access Crates)
stm32f4xx_hal = "0.30"
# Holy Rust core (no_std)
holy-rust-core = "0.1.0"
```

```rust
// Wrapping embedded-hal traits inside Holy Rust capabilities

use embedded_hal::digital::OutputPin;
use holy_rust_hal::Cap;

struct HolyRustPin {
    pin: STM32Pin,
    cap: Cap<GPIO>,
}

impl OutputPin for HolyRustPin {
    fn set_high(&mut self) -> Result<(), Self::Error> {
        // Acquire capability (if not already held)
        // Write to register via capability-protected write
        // Release capability if needed
        Ok(())
    }
    
    fn set_low(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}
```

### Domain Separation Summary

| Aspect | Host PC | Target Silicon |
|---|---|---|
| **Compiler** | rustc (full featured) | None (JIT tokenizer only) |
| **Linker** | cargo/rustc linker | None (static layout) |
| **Loader** | ELF dynamic linker | None (direct execution) |
| **Safety** | MMU + OS protection | Capability tokens (O(1)) |
| **Concurrency** | Threads, preemptive scheduler | Cooperative, no preemption |
| **Memory** | Virtual, paged, GB-scale | Physical, identity-mapped, KB-scale |
| **Interrupts** | Kernel-mediated | Direct thunk dispatch (<12 cycles) |
| **I/O** | Files, network, terminal | MMIO, direct register access |
| **Build Artifact** | ELF binary (.exe, a.out) | Hex image, flashed once |
| **Runtime Start** | OS boots, processes launch | Power-on, vector table init |
| **Development Cycle** | Edit → compile → run → debug | Edit → flash → REPL → test |