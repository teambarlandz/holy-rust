# Chapter 1: The Ring-0 Interactive Paradigm & SASA

---

## 1.1 The Architect's Dilemma

Traditional embedded development follows a rigid, disconnected workflow that every hardware engineer knows and tolerates:

```text
[Host PC] Write Code -> Cross-Compile -> Linker Script -> Flash over SWD/JTAG -> Hardware Reset -> Run
```

If the firmware crashes, the LED blinks the wrong way, or the sensor reads garbage, the cycle repeats: fix the source, re-compile, re-flash (wearing out Flash cells with every cycle), and reset the processor. On a Cortex-M4 at 84 MHz, the compile-link-flash-reset loop takes 10 to 60 seconds depending on project size. That latency is not just an inconvenience --- it fundamentally changes how you think about hardware. You stop experimenting. You start guessing.

The cost compounds. Every Flash write cycle degrades the non-volatile memory. Flash cells on STM32F4 are rated for roughly 10,000 write/erase cycles. A developer iterating on a peripheral driver might burn 100 cycles in a single afternoon. Over a product's lifetime, those cycles matter.

Holy Rust eliminates this latency entirely. It embeds an interactive streaming parser and single-pass JIT compiler directly into the target microcontroller. The MCU becomes an interactive environment where logic is evaluated microsecond-by-microsecond, over a serial terminal.

## 1.2 The Streaming Pipeline

When you type a command into the Holy Rust REPL, the data flows through this pipeline:

```text
[Serial / UART Stream]
       │
       ▼
┌──────────────────┐
│  On-Chip Lexer   │   ASCII bytes → Token stream
└────────┬─────────┘
         │
         ▼
┌──────────────────┐
│ Single-Pass JIT  │   Tokens → Threaded opcodes or native machine code
└────────┬─────────┘
         │
         ▼
┌──────────────────┐
│ EXEC_BUFFER      │   4 KB SRAM execution target
│ (SRAM)           │
└────────┬─────────┘
         │
         ▼
┌──────────────────┐
│ Ring-0 Execution │   Direct hardware access, no privilege transitions
└──────────────────┘
```

There is no filesystem. There is no OS layer. There is no loader. The parser reads bytes directly from the UART receive buffer, compiles them into executable code in SRAM, and executes them --- all within a single REPL line evaluation. The entire pipeline, from UART byte to hardware register write, completes in microseconds.

## 1.3 Single-Address-Space Architecture (SASA)

Standard operating systems enforce memory safety using hardware Memory Management Units (MMUs) or Memory Protection Units (MPUs) through page tables, virtual memory, and privileged/user ring transitions. This introduces significant overhead:

- **Context switches** save and restore 16-32 registers plus page table base pointers.
- **Page table walks** consume 5-15 memory cycles on each TLB miss.
- **Memory protection** requires syscalls for every hardware access from user mode.

Holy Rust operates on a Single-Address-Space Architecture (SASA) that eliminates all of this:

- **Zero Privilege Isolation Overhead:** All execution occurs in CPU Ring 0 (Privileged Mode on ARM, Machine Mode on RISC-V). There are no user-mode transitions, no syscall instructions, no context switches.
- **Flat 32-Bit Memory Visibility:** Physical memory, peripheral registers, and internal SRAM share a single global address space. A `poke` to `0x40020000` writes to GPIOA. A `peek` from `0x20000000` reads from SRAM. No address translation.
- **Software-Guaranteed Boundaries:** Safety is not enforced by virtual memory, but through explicit O(1) capability ownership checking prior to volatile access. The capability engine is a bitfield in SRAM, not a page table in Flash.

This is not a simplification. It is a deliberate architectural choice that trades privilege isolation for deterministic, single-cycle hardware access.

## 1.4 Why Ring 0?

Ring 0 means the CPU executes with full, unrestricted access to the entire address space. On ARM Cortex-M, this is the default (and only) privilege level --- there is no user mode in the Cortex-M profile. On RISC-V, this corresponds to Machine Mode.

The implication: **every memory access can fault.** A wild `peek` into unmapped address space triggers a HardFault. A `poke` to a read-only peripheral register faults. There is no MMU to catch the mistake silently. This is by design. The fault handler prints a diagnostic over UART:

```text
**FAULT: core exception, halted**
```

Ring 0 is not a limitation. It is a simplification that makes the system predictable. Every instruction does exactly what the source says. There are no hidden privilege checks, no hidden memory translation, no hidden context saves. The system has one address space, one privilege level, and one execution thread.

## 1.5 Architectural Comparison

| Dimension | Standard Embedded Rust | FreeRTOS / C Kernel | Holy Rust |
|---|---|---|---|
| Compilation Model | Static Host Cross-Compile | Static Host Cross-Compile | Target On-Chip JIT |
| Execution Memory | Read-Only Flash (.text) | Read-Only Flash (.text) | Executable SRAM (EXEC_BUFFER) |
| Privilege Model | Ring 0 (Static) | Ring 0 / User Mode | Ring 0 (Interactive REPL) |
| Memory Allocation | Static / Optional Alloc | Dynamic Pool (pvPortMalloc) | Strictly `#![no_std]` & `no_alloc` |
| Hardware Safety | Compile-Time Borrowing | Mutexes / Semaphores | O(1) SRAM Token Registry |
| Feedback Latency | 10-60 Seconds | 10-60 Seconds | Microseconds (Instant) |
| Runtime Size | 8-64 KB text + heap | 10-100 KB text + heap | ~16-64 KB text, zero heap |
| Debug Interface | GDB + OpenOCD | GDB + OpenOCD | UART REPL (built-in) |

## 1.6 What Holy Rust Is Not

Holy Rust is not a general-purpose operating system. It has:

- No filesystem.
- No network stack.
- No process model.
- No dynamic memory allocation.
- No threading or preemptive scheduling.

It is a **single-threaded interactive REPL kernel** for bare-metal microcontrollers. The REPL *is* the operating system. You type a command, the kernel compiles it, executes it, and prints the result. The kernel never returns to a "desktop" or "shell" --- the REPL loop is the final boot destination, running forever.

This constraint is the source of Holy Rust's simplicity. With one thread, one address space, and one privilege level, there are no races, no deadlocks, no priority inversions, and no resource contention. The system is deterministic by construction.

## 1.7 The `#![no_std]` Contract

Holy Rust is written in Rust with `#![no_std]` and `#![no_main]`. This means:

- No standard library (no `std::io`, no `std::collections`, no `std::thread`).
- No heap allocator (`#[global_allocator]` is absent).
- No `extern crate alloc` anywhere in the source tree.
- The panic handler is custom (routes through UART, then parks the core).

Every data structure is stack-allocated or static. Every buffer is fixed-capacity. Every collection uses open addressing or ring buffers with compile-time sizes. The binary is fully self-contained: it links against `core` only, and the Rust compiler eliminates all dead code through LTO (Link-Time Optimization) and `codegen-units = 1`.

The result: a kernel that boots from Flash, initializes a few KB of SRAM, and runs the REPL loop forever. No OS, no runtime, no garbage collector.
