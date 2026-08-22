# The Holy Rust Cookbook

**250 things Holy Rust can do — each with its how-to.**

Start with the [Prelude: Syntax Rules](00_Prelude_Syntax_Rules.md) —
it is the entire language on one page. Every chapter assumes it.

| # | Chapter | Tasks |
|---|---------|-------|
| 0 | [Prelude: Syntax Rules](00_Prelude_Syntax_Rules.md) | the grammar |
| 1 | [GPIO Control](Chapter_01_GPIO_Control.md) | 25 |
| 2 | [Memory Inspection & MMIO](Chapter_02_Memory_Inspection_MMIO.md) | 25 |
| 3 | [The Capability System](Chapter_03_Capability_System.md) | 25 |
| 4 | [Functions & the JIT](Chapter_04_Functions_JIT.md) | 25 |
| 5 | [Arithmetic & Expressions](Chapter_05_Arithmetic_Expressions.md) | 25 |
| 6 | [The REPL, System Commands & Telemetry](Chapter_06_REPL_System_Telemetry.md) | 25 |
| 7 | [Timers, Delays & Real-Time Patterns](Chapter_07_Timers_Real_Time.md) | 25 |
| 8 | [UART & Serial I/O](Chapter_08_UART_Serial_IO.md) | 25 |
| 9 | [Debugging & Fault Recovery](Chapter_09_Debugging_Fault_Recovery.md) | 25 |
| 10 | [Multi-Target Recipes (ARM ↔ RISC-V)](Chapter_10_Multi_Target_Recipes.md) | 25 |

**250 tasks total.**

## Conventions

- Every recipe is a real REPL session: `holy>` prompts, exact outputs.
- ARM addresses by default; Chapter 10 translates everything to RISC-V.
- No comments inside code blocks — the language has none (see Prelude P.10).
- Companion theory lives in `docs/Books/`; this cookbook is hands-on only.

## Quick start

```bash
cargo build --release --target thumbv7em-none-eabihf
qemu-system-arm -M netduinoplus2 -nographic \
    -kernel target/thumbv7em-none-eabihf/release/holy-rust
```

Then type along with Chapter 1, Task 1.01.
