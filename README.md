# HOLY RUST

A single-address-space, Ring-0 bare-metal interactive operating environment and
single-pass streaming JIT compiler for ARM Cortex-M and RISC-V microcontrollers.

```
  _   _ ol_         ____ust
 | | | | ___ | |_   |  _ \ _   _ ___| |_ 
 | |_| |/ _ \| | | | | |_) | | | / __| __|
 |  _  | (_) | | |_| |  _ <| |_| \__ \ |_ 
 |_| |_|\___/|_|\__, |_| \_\\__,_|___/\__|
                |___/                     
      The Bare-Metal Interactive OS
      
```

## Project Vision

Holy Rust synthesizes the immediacy of 1980s personal computing—typing code directly
into an interactive shell that executes immediately on raw hardware—with the mathematical
rigor of modern systems engineering. All code executes in CPU Ring 0 with verified safety
guarantees, eliminating the traditional user/kernel boundary and enabling microsecond-accurate
real-time execution.

## Architecture Diagram

```text
                          HOST PC ENVIRONMENT
                               │
                               │ compile once with cargo/rustc
                               ▼
+─────────────────────────────────────────────────────────────────+
│                  HOST-SIDE BUILD ENVIRONMENT                  │
│  cargo build --target <target>                                │
│  rustc --emit=llvm-ir ...                                     │
│  probe-rs target flash                                        │
+───────────────────────┬─────────────────────────────────────────+
                       │
                       │ hex image / binary
                       ▼
+─────────────────────────────────────────────────────────────────+
│                    TARGET SILICON (Ring 0)                    │
│  +─────────────────────────────────────────────────────────+   │
│  │  SRAM: Capability Registry + JIT Execution Buffers      │   │
│  │  • Cap<T> tokens (O(1) bitmask verification)           │   │
│  │  • Threaded micro-primitive dispatch array              │   │
│  │  • Symbol/Execution hash table                        │   │
│  +─────────────────────────────────────────────────────────+   │
│  │  Flash: Permanent micro-primitives (.rodata)            │   │
│  │  • load_reg_prim, write_reg_prim, etc.                │   │
│  +─────────────────────────────────────────────────────────+   │
│  │  Peripherals: MMIO registers accessed via Cap<T>      │   │
│  │  • GPIO, UART, SPI, I2C, PWM, Timers                   │   │
│  +─────────────────────────────────────────────────────────+   │
│  │  Vector Table: Relocatable to SRAM (<12 cycle dispatch)│   │
│  +─────────────────────────────────────────────────────────+   │
│  │  CPU: ARM Cortex-M or RISC-V, executing in Ring 0      │   │
│  +─────────────────────────────────────────────────────────+   │
│  │  Input: UART/USB CDC REPL stream                       │   │
│  │  Output: Direct console write to UART/USB              │   │
│  +─────────────────────────────────────────────────────────+   │
+─────────────────────────────────────────────────────────────────+
```

## Documentation Index

- [CHAPTER_01: MANIFESTO](docs/CHAPTER_01_MANIFESTO.md) - Philosophical and system foundation
- [CHAPTER_02: CAPABILITY ENGINE](docs/CHAPTER_02_CAPABILITY_ENGINE.md) - The safety model
- [CHAPTER_03: STREAMING JIT](docs/CHAPTER_03_STREAMING_JIT.md) - Compiler architecture
- [CHAPTER_04: RING 0 KERNEL](docs/CHAPTER_04_RING0_KERNEL.md) - Kernel internals
- [CHAPTER_05: BARE-METAL REPL](docs/CHAPTER_05_BARE_METAL_REPL.md) - Interactive shell and HAL
- [SYSTEM_BOUNDARIES_AND_ECOSYSTEM.md] - Domain separation and compilation model

## Quick-Start Guide

### Prerequisites

```bash
# Host toolchain
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
cargo install probe-rs  # or openocd
pip install picocom       # or minicom

# Target build (example for RISC-V)
cargo build --target riscv32imac-unknown-none-elf

# Flash to target
probe-rs target flash --chip rp2040

# Launch REPL (UART at 115200 baud)
picocom -b 115200 /dev/ttyACM0
```

### First Boot

1. Power on target hardware - CPU initializes vector table, clocks, SRAM
2. Holy Rust core engine loads - initializes SRAM Capability Registry
3. REPL/stream interface attaches - listens on UART/USB for source
4. Streaming single-pass verification - text tokenized, O(1) capabilities checked
5. Direct Ring 0 execution - CPU jumps to SRAM buffer, executes at hardware speed

### Example: GPIO Toggle via REPL

```rust
// Claim capability token for GPIO Port A
let mut gpio_a = cap_claim::<GPIOA>().expect("GPIOA already in use");

// Direct memory-mapped register write
poke(0x4002_0000 as *mut u32, 0x0000_0001);

// Inline toggle using capability token
gpio_a.pin(0).set_high();
```