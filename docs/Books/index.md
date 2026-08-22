# The Holy Rust Programming Book

*Split into separate chapter files for easier reading and maintenance.*

## Table of Contents

| # | Chapter Title | File |
|---|--------------|------|
| 1 | The Ring-0 Interactive Paradigm & SASA | [Chapter_01_The_Ring0_Interactive_Paradigm.md](Chapter_01_The_Ring0_Interactive_Paradigm.md) |
| 2 | Memory & Hardware Primitives (peek, poke, and MMIO) | [Chapter_02_Memory_And_Hardware_Primitives.md](Chapter_02_Memory_And_Hardware_Primitives.md) |
| 3 | The O(1) Linear Capability Safety Engine | [Chapter_03_Capability_Safety_Engine.md](Chapter_03_Capability_Safety_Engine.md) |
| 4 | Single-Pass Streaming JIT Mechanics | [Chapter_04_Single_Pass_Streaming_JIT.md](Chapter_04_Single_Pass_Streaming_JIT.md) |
| 5 | System Inspection, Memory Layout, and Telemetry | [Chapter_05_System_Inspection_Memory_Layout.md](Chapter_05_System_Inspection_Memory_Layout.md) |
| 6 | Field Diagnostics, Hard Faults, and Comprehensive Reference | [Chapter_06_Field_Diagnostics.md](Chapter_06_Field_Diagnostics.md) |
| 7 | Getting Started — Setting Up Your Environment | [Chapter_07_Getting_Started.md](Chapter_07_Getting_Started.md) |
| 8 | The REPL as Operating System | [Chapter_08_REPL_As_Operating_System.md](Chapter_08_REPL_As_Operating_System.md) |
| 9 | Interrupts and Real-Time Response | [Chapter_09_Interrupts_And_Real-Time.md](Chapter_09_Interrupts_And_Real-Time.md) |
| 10 | Multi-Target Development (ARM vs RISC-V) | [Chapter_10_Multi_Target_Development.md](Chapter_10_Multi_Target_Development.md) |
| 11 | Building Real Hardware Drivers | [Chapter_11_Building_Real_Hardware_Drivers.md](Chapter_11_Building_Real_Hardware_Drivers.md) |

## Appendices

| # | Appendix Title | File |
|---|---------------|------|
| A | Complete Command Reference (Alphabetical) | [Appendices.md#a-complete-command-reference-alphabetical](Appendices.md#a-complete-command-reference-alphabetical) |
| B | ARM Memory Map (from `memory.x`) | [Appendices.md#b-arm-memory-map-from-memoryx](Appendices.md#b-arm-memory-map-from-memoryx) |
| C | RISC-V Memory Map (from `memory-riscv.x`) | [Appendices.md#c-risc-v-memory-map-from-memory-riscvx](Appendices.md#c-risc-v-memory-map-from-memory-riscvx) |
| D | Error Code Table | [Appendices.md#d-error-code-table](Appendices.md#d-error-code-table) |
| E | Capability ID Registry (All CapId Variants) | [Appendices.md#e-capability-id-registry-all-capid-variants](Appendices.md#e-capability-id-registry-all-capid-variants) |
| F | Architecture Comparison Table | [Appendices.md#f-architecture-comparison-table](Appendices.md#f-architecture-comparison-table) |
| G | Binary Size Report | [Appendices.md#g-binary-size-report](Appendices.md#g-binary-size-report) |
| H | Software Licensing | [Appendices.md#h-software-licensing](Appendices.md#h-software-licensing) |

---

**Project**: Holy Rust — Ring-0 Interactive Computing, Single-Pass JIT Execution, and O(1) Capability Safety

**Targets**: `thumbv7em-none-eabihf` (ARM Cortex-M4F) and `riscv32imac-unknown-none-elf` (RISC-V SiFive E310)

**License**: Apache 2.0

---

*Generated from source files in `src/`. This book is derived from the codebase and documents the architectural commitments, verified manifesto compliance, and practical usage of the Holy Rust bare-metal REPL kernel.*