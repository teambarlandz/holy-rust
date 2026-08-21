# HOLY RUST

A single-address-space, Ring-0 operating environment and interactive streaming JIT compiler.
Provides instant bare-metal execution and interactive hardware control backed by O(1)
linear capability safety proofs.

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![GitHub repo](https://img.shields.io/badge/GitHub-Repository-blue.svg)](https://github.com/holy-rust/holy-rust)

## Overview

Holy Rust eliminates the traditional OS boundary between user space and kernel space.
All code—the live REPL, capability verifier, streaming JIT compiler, and user scripts—
executes exclusively within Ring 0 with verified safety guarantees.

- **Single-Address-Space Architecture**: Unified physical memory space, no MMU translation
- **Zero-Syscall Execution**: Direct hardware access via inlined function calls
- **Linear Capability Model**: O(1) compile-time safety via non-copyable tokens
- **Streaming Single-Pass Compilation**: Tokenize and emit in one pass, no AST/MIR

## Quick Start

```bash
# Build Holy Rust for your target
cargo build --target <your-target>

# Launch the bare-metal REPL
cargo run -- REPL
```

## Documentation

- [CHAPTER_01: MANIFESTO](docs/CHAPTER_01_MANIFESTO.md) - Core vision and design principles
- [CHAPTER_02: CAPABILITY ENGINE](docs/CHAPTER_02_CAPABILITY_ENGINE.md) - Linear capabilities & hardware tokens
- [CHAPTER_03: STREAMING JIT](docs/CHAPTER_03_STREAMING_JIT.md) - Single-pass syntax streaming JIT
- [CHAPTER_04: RING 0 KERNEL](docs/CHAPTER_04_RING0_KERNEL.md) - Ring 0 execution model
- [CHAPTER_05: BARE-METAL REPL](docs/CHAPTER_05_BARE_METAL_REPL.md) - Interactive Shell architecture
- [CHAPTER_06: HAL & INTEGRATION](docs/CHAPTER_06_HAL_AND_INTEGRATION.md) - HAL and porting guide

---

The Bare-Metal Interactive OS