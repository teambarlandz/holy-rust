# HR-OS (Holy Rust OS) — Production Roadmap, HAL Spec & Implementation Blueprint

> **Enterprise-Ready Engineering Package** — First-principles translation of HR-OS mathematical specs (`HR-OS/*.md`) into buildable hardware, verifiable invariants, and zero-cost `no_std` Rust.
>
> **Critical Mandate:** Pure SASA (VA ≡ PA, zero TLB/page walks) · Compile-time `no_std` kernel + LL(1) O(1) streaming JIT into `EXEC_BUFFER` · Linear-algebra 256-bit SIMD capability matrix (O(1) cycles) · Deterministic 43-cycle multi-core scheduler with autonomous PCIe/DMA rings. No POSIX, no MMU, no Ring 0↔3 gates, no microkernel IPC, no ELF dynamic linking.

---

## Table of Contents

- [Deliverable 1 — First-Principles Production Roadmap & Milestone Matrix (Phase 0–4)](#deliverable-1--first-principles-production-roadmap--milestone-matrix-phase-04)
- [Deliverable 2 — Repository Architecture & Directory Blueprint](#deliverable-2--repository-architecture--directory-blueprint)
- [Deliverable 3 — Mathematics-First HAL Trait Specs (`no_std`)](#deliverable-3--mathematics-first-hardware-abstraction-layer-hal-trait-specs-nostd)
- [Deliverable 4 — Mathematical & Hardware Physics Edge Case Watchlist](#deliverable-4--mathematical--hardware-physics-edge-case-watchlist)
- [Appendix — Cycle Budget Ledger & Invariant Map](#appendix--cycle-budget-ledger--invariant-map)

---

# Deliverable 1 — First-Principles Production Roadmap & Milestone Matrix (Phase 0–4)

All phases enforce **Definition of Done (DoD) as a cycle-count or byte-count inequality** — no subjective criteria. Every bound derives from `AXIS-*.md`, `WCEF.md`, `BENCHMARK.md`, and `ZERO-COPY.md`.

## Phase 0 — Toolchain, Linker, Custom Targets & QEMU HIL Harness

**Invariant:** Reproducible cross-build from host `x86_64-unknown-linux` to all HR-OS targets without host linker contamination. SASA section addresses (vectors `0x20000400`, registry `0x20001000`, code `0x20002000` / ITIM `0x08000000` on SiFive) are linker-enforced, not runtime-patched.

### Tasks

| #   | Work Item                                                                                                                                                                                                                                            | Artifact             | Owner     |
| --- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------- | --------- |
| 0.1 | Pin `rustc 1.97+` nightly + `rust-src`; `rustup target add thumbv7em-none-eabihf riscv32imac-unknown-none-elf x86_64-hros-none thumbv7em-none-eabihf riscv64-hros-none` (custom JSON)                                                                | `targets/*.json`     | Toolchain |
| 0.2 | Author 3 custom targets: `x86_64-hros-none.json` (`none` OS, `rust-lld`, `-C link-arg=-Tlinker.ld`, `disable-redzone:true`, `panic=abort`, `features:-mmx,-sse,+soft-float`), `thumbv7em-none-eabihf`, `riscv64-hros-none`                           | `targets/`           | Toolchain |
| 0.3 | `linker/` family: `memory.x` (ARM 128K flash @0x08000000 / 52K sram @0x20003000 + carved vectors/registry/sram_code), `memory-riscv.x` (SiFive E: flash 0x20400000, DTIM 8K, ITIM 4K), `memory-layout.x`/`memory-layout-riscv.x` + `linker.ld` alias | `linker/`            | Platform  |
| 0.4 | `build.rs` arch-selector + `cargo:rustc-link-arg=-T<linker>` + `rerun-if-changed` validation (ORIGIN/LENGTH/INCLUDE)                                                                                                                                 | `build.rs`           | Platform  |
| 0.5 | `.cargo/config.toml` per-target runners: `qemu-system-arm -M netduinoplus2 -cpu cortex-m4 -nographic -kernel` and `qemu-system-riscv32 -machine sifive_e -nographic -bios none -kernel`; `probe-rs` flash profiles                                   | `.cargo/config.toml` | HIL       |
| 0.6 | `Cargo.toml` workspace: `[profile.release] opt-level="z" lto=true codegen-units=1 panic="abort" strip=true` ; `[profile.dev] panic="abort"` ; `build-std=["core","compiler_builtins"]`                                                               | `Cargo.toml`         | Toolchain |
| 0.7 | CI `/.github/workflows/ci.yml`: `cargo build --target <each>`, `cargo clippy -- -D warnings`, `cargo fmt --check`, QEMU `--semihosting` UART harness                                                                                                 | `.github/`           | CI        |

### Acceptance Criteria (DoD — Mathematical)

| Criterion             | Inequality / Check                                                                                                                                                                                            | Verification                                               |
| --------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------- |
| Build reproducibility | `cargo build --target thumbv7em-none-eabihf --release && cargo build --target riscv32imac-unknown-none-elf --release && cargo build --target x86_64-hros-none.json --release` → 0 errors, 0 `clippy` warnings | CI log                                                     |
| Linker contract       | `ORIGIN(sram_code) == 0x20002000` (ARM) / `0x08000000` (RISC-V ITIM), `__sram_vectors_base == 0x20000400`, `__capreg_base == 0x20001000`, `_stack_top == ORIGIN(sram)+LENGTH(sram)`                           | `nm` / `cargo bleed` / `llvm-objdump --headers`            |
| Binary size           | `arm == 25–150 KiB`, `riscv == 25–45 KiB` stripped (`strip=true`, `opt-level=z`, LTO)                                                                                                                         | `size` / `ls -lh` (Thought.md §14.1: 141K ARM, 25K RISC-V) |
| QEMU boot             | `qemu-system-arm ... -kernel <elf>` prints `Holy Rust REPL v0.1\r\n` + `holy> ` within `100 ms` wall-clock                                                                                                    | `expect`-script harness                                    |
| HIL parity            | QEMU USART1 `@0x40011000` (SR TXE bit7 RXNE bit5, DR+0x04, CR1 UE                                                                                                                                             | TE                                                         | RE) and SiFive UART0 `@0x10013000` txdata/rxdata behave identically to probe-rs flash on real netduinoplus2 / FE310 board | CI matrix `qemu` + `hardware` jobs |

**Cycle Budget:** None (build-time only) but blocks all later phases.

---

## Phase 1 — Bare-Metal Foundation (SASA Setup, Vector Traps, Early Console, Bootstrap)

**Invariant:** VA ≡ PA identity map. No MMU enable, no page tables allocated. All execution Ring 0 / EL1 / M-mode. Every fault is _visible_ (UART-announced `fault_hang`, no silent lockup).

### Tasks

| #   | Work Item                                                                                                                                                                                                                                         | Spec Source                        |
| --- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------- |
| 1.1 | Hand-rolled `Reset` (ARM: plain `fn Reset()->!` after HW loads SP; RISC-V `#[naked] Reset` → `la sp/gp` norelax → `tail rust_boot_riscv`) + `init_data_bss()` volatile `.data LMA→VMA` + zero `.bss` via `__sidata/__sdata/__edata/__sbss/__ebss` | `AXIS-1.md`, `Thought.md §5`       |
| 1.2 | `.isr_vector` linker-emitted 16-word table `LONG(_stack_top) LONG(Reset) LONG(fault_hang)×` for NMI/HardFault/…/SysTick, reserved 0, plus `KEEP(*(.isr_vector))` for device IRQs                                                                  | `memory-layout.x`, `Thought.md §4` |
| 1.3 | SRAM vector relocation: copy `__vector_start..__vector_end` → `RAM_VECTOR_TABLE` (`.sram_vectors` @0x20000400, `repr(align(1024))`) → `VTOR=0xE000ED08` + `dsb/isb`; RISC-V `mtvec` → `_trap_hang: j _trap_hang` direct mode (`&!0x3`)            | `src/kernel/interrupt.rs`          |
| 1.4 | Direct MMIO early console (`drivers/uart.rs`): ARM CR1 UE                                                                                                                                                                                         | TE                                 | RE init, TXE/RXNE poll, `write_str`/`write_hex_u32`/`write_dec_u32` zero-alloc, 256-B SPSC RX ring + `irq_handler()` stub | `AXIS-2.md`, `Thought.md §3` |
| 1.5 | `panic_handler` → UART `PANIC:` + `message()` + `wfi` loop                                                                                                                                                                                        | `src/main.rs`                      |
| 1.6 | `BANNER` + `fault_hang` → `\r\n**FAULT: core exception, halted**\r\n` + `wfi` (freed from silent `0x00` vector)                                                                                                                                   | `kernel/interrupt.rs`              |

### Acceptance Criteria (DoD)

| Criterion            | Inequality / Check                                                                                                                                      | Verification                                                           |
| -------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------- |
| Identity map         | `peek_u32(addr) == poke_u32(addr,val); peek_u32(addr)==val` for SRAM `@0x20003000`, MMIO `@0x40020000`, `EXEC_BUFFER @0x20002000` — 0 translation steps | REPL `peek/poke` + `llvm-objdump -p` shows **no** `TTBR`/`SATP` writes |
| Vector integrity     | `VTOR == 0x20000400`, `RAM_VECTOR_TABLE` 1024-B aligned, `LONG(Reset)` odd (Thumb-bit not double-tagged)                                                | `gdb`/`monitor info registers`; Thought.md §12.1 regression test       |
| Trap visibility      | `peek <unmapped>` → `**FAULT: core exception, halted**` within 2 cycles (not lockup)                                                                    | QEMU smoke (`docs` fault test)                                         |
| Early console        | `init()` → `write_str("Holy Rust REPL v0.1\r\n")` → QEMU `-serial stdio` captures banner in < 12 cycles TXE path                                        | `expect`                                                               |
| Data/BSS             | `static X: u32=0xDEADBEEF` in `.data` reads back 0xDEADBEEF post-reset; `.bss` zero                                                                     | self-test in `boot()`                                                  |
| `.eh_frame` stripped | `/DISCARD/ *(.eh_frame*) *(.comment*)` → `readelf -S` shows none                                                                                        | `readelf`                                                              |

**Cycle Budget:** `init_data_bss` ≤ `(.data_words+ .bss_words)`×2 loads/stores (linear, bounded by linker).

---

## Phase 2 — Axis 1 (Lock-Free Scheduler) & Axis 3 (SIMD Capability Matrix)

**Invariant:** Time is a hardware crystal, not an OS tick estimate. All scheduling decisions O(1) and atomic CAS-coherent; no mutex, no TLB flush. Safety is a _bit-test_, not a page walk.

### Tasks Axis 1

| #    | Work Item                                                                                                                                                                                                                | Cycle Target      |
| ---- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ----------------- |
| 2A.1 | SysTick/APIC/`mtime` config: `N = f_CPU × Δt` (e.g., 84 MHz×1 ms=84 000 ticks), `STK_LOAD/STK_VAL/STK_CTRL=0x07`                                                                                                         | config-time       |
| 2A.2 | XSAVE minimal frame: HW auto-stack `xPSR/PC/LR/R12/R3-R0` (12 cyc) + SW push `R4-R11` (8 cyc) + `sched: circular index + CAS ring` (3 cyc) + pop (8) + auto-unstack (12) = **43 cyc**                                    | 43 cyc            |
| 2A.3 | `LockFreeTaskQueue<T>` per `UPGRADE.md` §Step2: `#[repr(C,align(64))] head:AtomicUsize tail:AtomicUsize tasks:[*mut TCB;256]`; `CAS` via `LDREX/STREX` or `CAS`/`cmpxchg`; `WFE/SEV` (ARM) / `MONITOR/MWAIT` (x86) / IPI | 8–12 cyc dispatch |
| 2A.4 | Shadow stacks / PAC / CET hook for call-depth `D≤Dmax`, recursion banned                                                                                                                                                 | 0 jitter          |

### Tasks Axis 3

| #    | Work Item                                                                                                                                                                                    | Cycle Target |
| ---- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------ |
| 2B.1 | `REGISTRY_BITS: RegistryBits([AtomicU32;8])` `@0x20001000` (256 resources @ 4 KB, 128 KiB for 64 tasks×16 K caps fits L1) + `acquire/fetch_or`, `release/fetch_and`, `available/load & mask` | 3 cyc scalar |
| 2B.2 | Guard injection (JIT): ARM64 `LSR X2,X0,#12; LSR X3,X2,#6; AND X4,X2,#63; LDR X5,[X21,X3,LSL#3]; LSR X5,X5,X4; TBZ X5,#0,.FAULT_TRAP; STR X1,[X0]` — **3 instr**                             | 3 cyc        |
| 2B.3 | SIMD 256-bit upgrade (AVX2/NEON): `VANDPS ymm2,ymm0,ymm1; VPTEST ymm2,ymm1; JNC .FAULT_TRAP` — 1 cycle for 256 blocks (=1 MiB)                                                               | **1 cyc**    |
| 2B.4 | `CapId` enum + `addr_to_cap_id()` per-arch maps (ARM STM32F405 `0x40020000 GPIOA …`, RISC-V FE310 `0x10012000 GPIOA …`) + `check_access()->Ok/Err` (SuperUser bypass → audit ring)           | O(1)         |
| 2B.5 | `Cap<T>`/`PinGuard<'a,T,N>` linear tokens (`!Copy` by omission, no `Drop` → `drop_cap(cap)` explicit), `HardwareResource::RESOURCE_ID`, `GpioPort::BASE`, `resolve_name()` for REPL          | typestate    |

### Acceptance Criteria (DoD)

| Criterion            | Inequality                                                                                                                | Verification                                                                              |
| -------------------- | ------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------- |
| Context switch       | `T_ctx == 43 ±0 cyc` @168 MHz → `0.255 µs`; measured `Cycles(Cortex-M4) = 12+8+3+8+12`                                    | `DWT->CYCCNT` delta in `kernel/interrupt.rs` bench                                        |
| Scheduler jitter     | `σ(T_ctx) == 0` (pure SASA, no TLB flush) vs Linux `1k–10k cyc` / FreeRTOS `84 cyc`                                       | 10 000 switches `max-min==0`                                                              |
| Guard latency scalar | `C_guard == 3 cyc` (1 LSR +1 LDR +1 TBZ)                                                                                  | `src/capabilities/registry.rs` microbench                                                 |
| Guard latency vector | `C_guard_256 == 1 cyc` (256×4 KB blocks) · reduction 66.6% (`I_base+ N×3 → I_base+N×1`)                                   | AVX2 `VANDPS+VPTEST` bench                                                                |
| Capability memory    | `64 tasks ×16384 bits /8 == 128 KiB`                                                                                      | `size` of `.capability_registry`                                                          |
| Atomicity            | `acquire()` false iff bit already set; `release()` idempotent; MESI coherence holds under `sifive_e` multi-core IPI flood | `loom` / QEMU multi-core race                                                             |
| Zero placeholder     | No `todo!()` / `unimplemented!()` ; every `unsafe` has `// SAFETY:`                                                       | `rg -n "todo!\|unimplemented!\|TODO" src` == 0 ; `clippy::undocumented_unsafe_blocks` ==0 |
| No alloc             | `!` crate `alloc`, ring 256 B + registry 32 B + tables static                                                             | `cargo metadata                                                                           | grep alloc` == none |

---

## Phase 3 — Axis 2 (Autonomous PCIe/DMA Rings) & Axis 4 (LL(1) Streaming JIT)

**Invariant:** Host CPU never `memcpy` bulk data; peripherals bus-master. Compilation is _streaming ASCII → `EXEC_BUFFER`_ in one linear scan, no AST, no backtracking, no disk ELF.

### Tasks Axis 2

| #    | Work Item                                                                                                                                                                                                                                                                  | Latency       |
| ---- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------- |
| 3A.1 | ECAM identity map: `Target=ECAM_Base + (B<<20)                                                                                                                                                                                                                             | (D<<15)       | (F<<12) | R`; sweep`B 0..255 D 0..31 F 0..7`→`peek`Vendor`0xFFFF`skip; BAR sizing`Vmask→~(Vmask & ~0xF)+1`(e.g.,`0xFFF00000→1 MiB`) | O(1) ECAM, O(N) enum once |
| 3A.2 | `AutonomousDmaRing` (`#[repr(align(64))] descriptors:[DmaDesc;128] head:AtomicU32 tail:AtomicU32`): `Ptr_HEAD` (HW) / `Ptr_TAIL` (driver), `C=(HEAD-TAIL-1)%K`, `CLFLUSHOPT/DC CVAC` + PCIe TLP, `HW updates HEAD via bus mastering, CPU polls` — **0 CPU cycles** per TLP | 0 cyc blocked |
| 3A.3 | O(1) DMA config-time range check `k_start=A>>12 k_end=(A+L-1)>>12 mask=((1<<(n))-1)<<(k&63) authorized=(W[I]&mask)==mask` ; `Zero-In-Flight` (no IOMMU, no IoTLB miss) + async ISR-as-ring-push `IRQ_Map[N]->TaskID → ring buffer → TailChain READY`                       | single AND    |

### Tasks Axis 4

| #    | Work Item                                                                                                                                                                                                                                                                                                  | Budget                                                                                                                                  |
| ---- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------- |
| 3B.1 | `Lexer<'a>` zero-alloc `&[u8]` cursor (32-B scratch), tokens `KwFn/KwLet/Identifier(&'a [u8])/Literal(u32)/LParen/RParen/LBrace/RBrace/...` ; `next_token()` O(1) lookahead, `peek` slot; decimal/0x hex/`_` separators, overflow → `Token::Error`                                                         | `C_lexer ≤25 cyc/B`                                                                                                                     |
| 3B.2 | `Compiler` persistent `symbols:64 slots FNV-1a, fns:4×64 words, stream:128 words` (static REPL-owned), LL(1) grammar `poke/peek/loop/delay/cap_claim/cap_drop/reg_set_bit…` `let`/`fn`(){} ; left-to-right no-pred, no precedence, recursion banned `D≤Dmax`                                               | O(n)                                                                                                                                    |
| 3B.3 | `primitives.rs` Flash `.rodata` `lit_prim/load_reg_prim/write_reg_prim/add/sub/mul/div/halt_prim` stack-RPN (pure `vm_push/pop` , div0→0)                                                                                                                                                                  | threaded 100 µs                                                                                                                         |
| 3B.4 | `emitter.rs` Thumb-2 `MOVW 0xF240/MOVT 0xF2C0/MOVS 0x2000 / STR 0x6000/LDR 0x6800 / ADDS 0x1800 / BX 0x4770 / SDIV` + RV32 `LUI/ADDI hi20=(imm+0x800)>>12 / SW/LW / ADD/Rtype MUL/DIV / RET 0x00008067`; `emit_mov_imm` fast `MOVS≤255` else `MOVW/MOVT`, `emit_*` `Result<EmitError>` with overflow check | native emit                                                                                                                             |
| 3B.5 | `native.rs` two-reg COMPILER (`ACC=r0/a0, SCR= r1/a1`) `lit [op lit]* [load                                                                                                                                                                                                                                | store] halt`→`EXEC_BUFFER`(ARM ITIM/SRAM_code) +`flush_instruction_cache()` (`dsb/isb`/`fence.i`) → `execute_sram_buffer(offset)` `base | 1` (ARM Thumb-bit) | ≥1 ms compile |
| 3B.6 | REPL `drivers/repl.rs` `Idle→Reading→Evaluating→Printing`, line 128 B, echo, `\b \b`, `Ctrl-U` kill, `Ctrl-C` cancel, `peek/poke` → `enforced_*`, `cap_claim/drop` → registry, `sys_audit`                                                                                                                 | single-pass                                                                                                                             |

### Acceptance Criteria (DoD)

| Criterion           | Inequality                                                                                                                                          | Verification                                                 |
| ------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------ |
| DMA zero-copy       | `bytes_copied ==0` ; throughput = DRAM bus; `transfer_latency(1.5 KiB)` = **8 cyc `0.048 µs`** vs Linux `400 cyc 2.5 µs` / mbox `140 cyc 0.85 µs`   | `ZERO-COPY.md` bench `LSR/STXR/BIC/ORR`                      |
| DMA CPU offload     | CPU cycles per `submit_transfer()` ≤ 120 → upgraded `0` blocked (autonomous ring poll)                                                              | `DWT` before/after TLP                                       |
| ECAM address        | `target == ECAM_Base+(B<<20)+(D<<15)+(F<<12)+R` for all `BDF` combos, size calc matches BAR                                                         | unit test vs `pci_read_config` golden                        |
| JIT linear          | `T_JIT(S) = S × C_lexer` `C≤25 cyc/B`; 256-B max `6400 cyc ≈38 µs @168 MHz`                                                                         | `WCEF.md` bound + QEMU `CYCCNT`                              |
| Emitter correctness | `MOVW/MOVT/STR/LDR` encoding matches ARM ARM / RV manual; `thumb disasm ==` & `riscv disasm ==` ; overflow → `Err(EmitError::Overflow)` not corrupt | `cargo test --features std` host-side encode helpers         |
| End-to-end poke     | ASCII `poke 0x40021018 1` → LED latch in `≈85 cyc 0.50 µs` (`Axis4 25+Axis3 3+Axis4 12+Axis1 43+Axis2 2`)                                           | `E2E-SYSTEM-TRACE.md` matrix + QEMU GPIO virtual port `peek` |
| Native fallback     | `RISC-V native early-return` (PT_LOAD RX vs RW) + `fence.i` infra ready; ARM `exec_buffer_entry()` `addr                                            | 1` correct                                                   | Thought.md §13.4 regression |
| Single-pass         | No AST allocated, no `loop{while(true)}` without `loop N{}` bound, no recursion cycles                                                              | `rg "AST\|while\(true\)"` ==0                                |

---

## Phase 4 — Formal Verification & HIL Fuzzing (Cycle-Accurate Determinism Proofs)

**Invariant:** Every deadline is a theorem, not a hope. WCET = `T_JIT+T_Exec+T_Cap+T_Ctx`, RTA proves `R_i ≤ D_i` with _zero_ page-fault/TLB jitter.

### Tasks

| #   | Work Item                                                                                                                                                                                                            | Method                               |
| --- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------ |
| 4.1 | WCET ledger per `WCEF.md §1–3`: `T_JIT = S×25`, `T_Cap=N×3` (scalar) / `N×1` (vector), `T_Ctx=43`, `T_Exec=Σ BB_cost×bound` with bounded `loop N {}` + recursion ban; `build-std` ensures no hidden allocator jitter | static analysis                      |
| 4.2 | RTA schedulability: `R_i^{(k+1)} = W_Ti + Σ_{j<i} ceil(R_i^{(k)}/P_j)×W_Tj` ; prove `∀i R_i≤D_i`                                                                                                                     | TLA+/Coq or hand-proof+model checker |
| 4.3 | HIL fuzz: byte-stream mutator over UART REPL (valid ASCII + glitch bytes) → no MMU fault escapes `REGISTRY_BITS`; DMA range sweeps; SIMD unaligned vectors; 10 edge cases (Deliverable 4)                            | libFuzzer/QEMU                       |
| 4.4 | Fault injection: Invalid opcode `#UD/UNDEF` → `1-cyc` `.FAULT_TRAP` `C_Task←0`; WWDT `t_upper` NMI → task kill + `WWDT_REFRESH_REG` feed; DMA `Vendor=0xFFFF`/ BAR wrap `~(mask)+1` stress                           | QEMU `-d int`                        |
| 4.5 | Benchmark suite `BENCHMARK.md @168 MHz`: context 43, IRQ 12, IPC 8, `malloc 0`, jitter 0 vs FreeRTOS 84/12-25/120 & seL4 310/120-180/310                                                                             | `DWT->CYCCNT`/`mcycle` CSV           |
| 4.6 | Coverage reports: branch coverage on `lexer/parser/emitter/registry` + `unsafe` audit sign-off                                                                                                                       | `cargo coverage`                     |

### Acceptance Criteria (DoD)

| Criterion       | Inequality                                                                                                                           | Verification                                        |
| --------------- | ------------------------------------------------------------------------------------------------------------------------------------ | --------------------------------------------------- |
| WCET bound      | `∀ Ti E(Ti) ≤ D_i` ; `max E` measured ≤ `RTA proof + 0 jitter` (no `+10k` page-fault tail)                                           | RTA report + CYCCNT trace                           |
| Fuzz survival   | `1M UART byte-mutations + 10k DMA range-mutations` → 0 host crashes, 0 capability escapes (all `Err(CapViolation)` or `.FAULT_TRAP`) | CI fuzz job artifact                                |
| Trap recovery   | Invalid opcode → `.FAULT_TRAP` `STR R3,[R2]` zeros vector + `TASK_STATE_DEAD` + `B AXIS1_SCHEDULE_NEXT` in <15 cyc                   | `INVALID-OP-CODES.md` trace + QEMU `info registers` |
| Watchdog window | Lower `t_lower` feed → fault (anti-hog), upper `t_upper` overflow → NMI; feeding inside `(t_lower,t_upper)` succeeds                 | `WCEF` timer sweep plot                             |
| Determinism     | `1000× poke/peek roundtrip` latency `σ ==0` (43 cyc switch + 0 TLB)                                                                  | histogram CSV (`mean, σ, max-min`)                  |
| No dyn          | `grep -r "dyn.*Trait\|Box<dyn"` `src/` ==0 ; vtable erased                                                                           | `cargo bloat` + `nm`                                |

### Milestone Matrix (Summary)

| Phase | Duration | Exit Gate (cycles)                               | Exit Gate (bytes)             | Owner     |
| ----- | -------- | ------------------------------------------------ | ----------------------------- | --------- |
| 0     | 2 w      | builds 3 targets 0 warn                          | `strip` ARM ≤150K RISC-V ≤45K | Toolchain |
| 1     | 2 w      | VTOR→SRAM, fault→UART <15 cyc                    | `.vector` 16 words            | Kernel    |
| 2     | 3 w      | `T_ctx 43` jitter 0, guard 3→1                   | 128 KiB cap SRAM cached       | Axes 1/3  |
| 3     | 4 w      | JIT 25 c/B, poke 85 cyc 0.5 µs, DMA 0 copy 8 cyc | EXEC 4 KiB, DMA 128×desc      | Axes 2/4  |
| 4     | 3 w      | `R_i≤D_i` proved, fuzz 0 escape, σ=0             | coverage >85%                 | Verify    |

---

# Deliverable 2 — Repository Architecture & Directory Blueprint

Derived from `holy-rust/RoadMap.md §0` + `docs/*` + `Thought.md` §4–13. Migrated from mono-`src/` to Cargo workspace with strict zero-cost crate boundaries (no cross-crate `alloc`, no hidden vtables).

## Canonical Tree

```text
holy-rust/                          # workspace root (Cargo.toml [workspace])
├── Cargo.toml                      # [workspace] resolver="2", members = [...]
├── Cargo.lock
├── rust-toolchain.toml             # channel="nightly", components=["rust-src","llvm-tools"]
├── build.rs                        # arch-select linker trampoline (see Phase 0)
├── linker/
│   ├── memory.x                    # ARM STM32F4 128K@0x08000000 / 52K sram @0x20003000 + carved vectors/registry/sram_code
│   ├── memory-riscv.x              # RISC-V SiFive 0x20400000 flash, 8K DTIM, 4K ITIM
│   ├── memory-layout.x             # shared SECTIONS .isr_vector (NOLOAD) + KEEP + /DISCARD/
│   ├── memory-layout-riscv.x       # riscv variant w/out vector table
│   ├── linker.ld                   # alias to memory.x (compat for x86_64-hros-none)
│   └── HR-OS_SASA.ld               # (optional) consolidated physical map view 0x0000_0000…FFFF_FFFF
├── targets/
│   ├── x86_64-hros-none.json       # "os":"none" , "disable-redzone":true , "panic":"abort" , "-mmx,-sse,+soft-float"
│   ├── thumbv7em-none-eabihf.json  # (or use builtin) cortex-m4F
│   ├── riscv32imac-unknown-none-elf.json
│   └── riscv64-hros-none.json
├── .cargo/
│   └── config.toml                 # runners: qemu-system-arm netduinoplus2 & sifive_e, build-std ["core","compiler_builtins"]
├── .github/workflows/ci.yml        # build + clippy + fmt + qemu expect harness
├── docs/
│   ├── CHAPTER_01_MANIFESTO.md ... CHAPTER_06_HAL_AND_INTEGRATION.md
│   ├── SYSTEM_BOUNDARIES_AND_ECOSYSTEM.md
│   ├── MANIFESTO-COMPLIANCE.md
│   ├── AXIS-*.md / BENCHMARK.md / ZERO-COPY.md / WCEF.md / DMA.md / UPGRADE.md  # (mirrored from HR-OS/)
│   └── PRODUCTION_BLUEPRINT.md     # ← this file (also at HR-OS/HR-OS_PRODUCTION_BLUEPRINT.md)
├── crates/
│   ├── hros-hal/                   # ← Deliverable 3 : mathematics-first trait specs (ZERO-COST)
│   │   ├── Cargo.toml              # no_std, no alloc, crate-type=["lib"], embedded-hal optional
│   │   └── src/
│   │       ├── lib.rs              # #![no_std] pub mod {switch, irq, cap, exec}
│   │       ├── switch.rs           # ContextSwitch trait (≤43 cyc)
│   │       ├── irq.rs              # InterruptController trait (physical vector dispatch)
│   │       ├── cap.rs              # VectorCapabilityEngine trait (256-bit SIMD)
│   │       └── exec.rs             # ExecutionBuffer trait (I-cache fence + emit)
│   ├── hros-arch-arm/              # ARM Cortex-M4/M7 HAL impls
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── switch.rs           # STM32ContextSwitch : 12+8+3+8+12 cyc impl (asm push/pop, msr VTOR)
│   │       ├── irq.rs              # NVIC : VTOR @0xE000ED08, DSB/ISB, attach_jit_irq
│   │       ├── cap.rs              # scalar TBZ + NEON v2×64 (future) stub → WCEF 1-cyc
│   │       └── exec.rs             # Thumb2Emitter wrappers + dsb/isb
│   ├── hros-arch-riscv/            # RISC-V RV32IMAC / RV64 impls
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── switch.rs           # csrrw sp, mscratch ; sw/ct + gp relaxation __global_pointer$
│   │       ├── irq.rs              # mtvec direct/vectored, global_asm! _trap_hang
│   │       ├── cap.rs              # scalar + vector extension hook (RVV)
│   │       └── exec.rs             # RV32Emitter + fence.i
│   ├── hros-arch-x86/              # (stretch) x86_64 bare-metal (APIC, TSS, no MMU)
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── switch.rs           # APIC Timer, IRET-safe push r4..r11
│   │       ├── irq.rs              # IDT @0x0000, IOAPIC MSI-X
│   │       ├── cap.rs              # AVX2 VANDPS/VPTEST path (1 cyc)
│   │       └── exec.rs             # x86 emitter + clflush/mfence
│   ├── hros-cap/                   # O(1) linear capability engine (Phase 2B)
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs              # pub use registry::*, tokens::*, audit::*
│   │       ├── registry.rs         # REGISTRY_BITS @0x20001000 AtomicU32[8], addr_to_cap_id, check_access
│   │       ├── tokens.rs           # Cap<T>, PinGuard<'a,T,N>, HardwareResource, GpioPort, resolve_name
│   │       └── audit.rs            # 16×AuditEntry ring, SUPERUSER_AUDIT_LOG, get_cycle_count()
│   ├── hros-kernel/                # Ring 0 core infrastructure (Phase 1)
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs              # pub mod {memory, exec, interrupt}; pub const BANNER
│   │       ├── memory.rs           # peek_u32/poke_u32 volatile, enforced_poke/peek, init_data_bss, reg_set_bit RMW
│   │       ├── exec.rs             # EXEC_BUFFER 4K .sram_code, vm_push/pop, run_threaded_stream, exec_buffer_entry |1, flush_icache, execute_sram_buffer
│   │       └── interrupt.rs        # fault_hang, RAM_VECTOR_TABLE align(1024), relocate_vector_table, attach_jit_irq, boot_relocate_vectors
│   ├── hros-jit/                   # Single-pass streaming JIT (Phase 3B)
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── lexer.rs            # Lexer<'a>, Token<'a> zero-alloc slice borrow
│   │       ├── parser.rs           # Compiler, Symbol 64×, Fn 4×64, stream 128, Outcome, check_access()
│   │       ├── primitives.rs       # lit/load_reg/write_reg/add/sub/mul/div/halt MicroPrimitive
│   │       ├── emitter.rs          # TargetEmitter trait + Thumb2Emitter/Riscv32Emitter + encode_* pure helpers
│   │       └── native.rs           # two-reg native compile_and_run (ACC=r0/a0) threaded fallback
│   ├── hros-drivers/               # HAL-adjacent drivers (Phase 3A + REPL)
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── uart.rs             # mmio per-arch, put_byte poll TXE, poll_get_byte, SPSC 256-B ring, irq_handler()
│   │       ├── repl.rs             # State Idle→Reading→Evaluating→Printing, 128-B line, feed(), compile(), execute(), sys_audit
│   │       ├── pcie.rs             # (future) ECAM enumerator, BAR sizing, AutonomousDmaRing
│   │       └── timer.rs            # SysTick/APIC/RISC-V mtime N=f×Δt, WWDT window
│   └── hros-core/                  # Bin glue + boot entry (replaces old src/main.rs)
│       ├── Cargo.toml              # [[bin]] name="hros_kernel" path="src/main.rs"
│       └── src/
│           ├── main.rs             # #![no_std] #![no_main] panic_handler + Reset/ rust_boot_riscv + boot()→repl::run()
│           └── lib.rs              # re-exports (optional)
├── xtask/                          # (optional) cargo xtask qemu-arm, xtask fuzz, xtask bench
│   ├── Cargo.toml
│   └── src/main.rs
└── memory.x -> linker/memory.x     # symlink for legacy INCLUDE memory.x path
```

## Cargo Workspace Manifest (Sketch)

```toml
# holy-rust/Cargo.toml — workspace root
[workspace]
resolver = "2"
members = [
  "crates/hros-hal",
  "crates/hros-arch-arm",
  "crates/hros-arch-riscv",
  "crates/hros-arch-x86",
  "crates/hros-cap",
  "crates/hros-kernel",
  "crates/hros-jit",
  "crates/hros-drivers",
  "crates/hros-core",
  "xtask",
]

[workspace.package]
edition = "2021"
version = "0.1.0"
license = "MIT OR Apache-2.0"

[workspace.dependencies]
embedded-hal = "1.0"
hros-hal = { path = "crates/hros-hal" }
hros-cap = { path = "crates/hros-cap" }
hros-kernel = { path = "crates/hros-kernel" }
hros-jit = { path = "crates/hros-jit" }

[profile.release]
opt-level = "z"
lto = true
codegen-units = 1
panic = "abort"
strip = true
debug = false
```

## Strict Zero-Cost Boundary Responsibilities

| Crate          | Public Responsibility                                                                                                                                                 | Must **NOT**                                                 | Budget                      |
| -------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------ | --------------------------- |
| `hros-hal`     | Pure trait specs, `const MAX_CYCLES`, `#[inline(always)]` signatures, error types; zero impl, zero `unsafe` surface beyond trait `unsafe` marker; `no_std`/`no_alloc` | depend on `hros-arch-*`, `embedded-hal` impl, allocator      | 0 B RAM                     |
| `hros-arch-*`  | `impl HalTrait for Concrete` (`NVIC`, `MTVec`, `Thumb2Impl`); `unsafe` MMIO writes gated by `cap_id` when available; DSB/ISB, fence.i, AVX intrinsics isolated here   | know REPL, lexer, symbol table                               | ≤43 cyc switch              |
| `hros-cap`     | `AtomicU32[8]` bitfield @0x20001000, `Cap<T>`/`PinGuard` linear tokens, `addr_to_cap_id` per-arch `match`, `audit` ring                                               | emit opcodes, own UART                                       | 192 B audit + 32 B registry |
| `hros-kernel`  | `peek/poke` volatile, `EXEC_BUFFER 4K`, `RAM_VECTOR_TABLE`, `fault_hang`, `init_data_bss`                                                                             | parse ASCII, own capability policy                           | 4 KiB exec + 3 KiB vectors  |
| `hros-jit`     | `Lexer<'a>`/`Compiler`/`TargetEmitter`/`MicroPrimitive`, vector guard injection                                                                                       | touch VTOR, DMA HW                                           | O(n) lex ≤25 c/B            |
| `hros-drivers` | `uart` MMIO+ring, `repl` state machine, `pcie` ECAM/DMA ring, `timer` ticks                                                                                           | duplicate capability checks (calls `hros-cap` single source) | 256 B ring + 128 B line     |
| `hros-core`    | `Reset` entry, banner, `boot_relocate_vectors()->run()`; panic `wfi`                                                                                                  | contain business logic                                       | entry only                  |

**Zero-Cost Enforcement Rules (CI)**

1. No `dyn Trait`, `Box<dyn _>`, `alloc` in any crate (`cargo metadata | grep alloc` → ∅, `grep -r "dyn.*Trait"` → ∅). Generics monomorphize; vtables erased.
2. Every `unsafe` documents `// SAFETY:` with alignment + volatile + SASA invariant (clippy `undocumented_unsafe_blocks` = deny).
3. No `TODO`/`todo!()`/`unimplemented!()` — complete modules only (`RoadMap.md` Cross-Milestone #1).
4. `#[inline(always)]` on hot paths (`peek/poke`, `check_access`, `emit_*`, `acquire`).
5. `repr(align(64))` on `LockFreeTaskQueue`, `DirectDmaRing`; `align(1024)` on `RAM_VECTOR_TABLE` (VTOR), `align(4)` on `REGISTRY_BITS`/`EXEC_BUFFER`.
6. `.cargo/config.toml` + `build.rs` are the _only_ host-toolchain coupling; no `std` leaks into `no_std` crates (`#![cfg_attr(not(feature="std"), no_std)]` gate in `lib.rs`).

---

# Deliverable 3 — Mathematics-First Hardware Abstraction Layer (HAL) Trait Specs (`no_std`)

> **No vtable contract:** Traits are _static-dispatch only_ — consumers bound as `fn foo<H: ExecutionBuffer>()` / `struct Kernel<H: Hal>` so monomorphization erases indirection. Do **not** use `Box<dyn ExecutionBuffer>` or `&dyn InterruptController`. CI enforces `grep dyn` = ∅.

Core crate `crates/hros-hal/src/lib.rs` (compilable `no_std`):

```rust
//! hros-hal — mathematics-first HAL trait specs (no_std, zero-cost).
//! See HR-OS/AXIS-*.md, UPGRADE.md. Every impl must preserve cycle invariants.

#![no_std]
#![deny(unsafe_op_in_unsafe_fn)]
#![warn(clippy::undocumented_unsafe_blocks)]

pub mod cap;
pub mod exec;
pub mod irq;
pub mod switch;

pub trait Hal: Sized {
    type Switch: switch::ContextSwitch;
    type Irq: irq::InterruptController;
    type Cap: cap::VectorCapabilityEngine;
    type Exec: exec::ExecutionBuffer;
}
```

---

## 3.1 `ContextSwitch` — 43-Cycle Register Save/Restore

Invariant `Φ: S × T_old × T_new → S'` bijection. HW auto-stack 12 + SW push 8 + schedule 3 + pop 8 + unstack 12 = 43 ±0.

```rust
//! crates/hros-hal/src/switch.rs

/// Deterministic context switch mechanics. Every impl must uphold
/// `TOTAL_CYCLES == 43` at 168 MHz (0.51 µs ±0 jitter, SASA → no TLB flush).
pub trait ContextSwitch: Sized {
    /// Total bounded cycles for a full switch (incl. HW auto-stack).
    const TOTAL_CYCLES: usize = 43;
    /// HW auto-stack cycles (xPSR/PC/LR/R12/R3-R0).
    const CYCLES_AUTO_STACK: usize = 12;
    /// SW callee-save push (R4-R11).
    const CYCLES_MANUAL_PUSH: usize = 8;
    /// Scheduler pointer bump.
    const CYCLES_SCHED: usize = 3;
    /// SW pop + auto-unstack
    const CYCLES_RESTORE: usize = 20; // 8 + 12

    /// Callee-saved register block. Repr C for asm `stm/ldm` layout parity.
    /// `R4` at lowest address matches `push {r4-r11}` order.
    #[repr(C)]
    type Frame: Copy;

    /// Save `R4-R11` onto `*sp` (SP descending full). Returns new SP.
    /// # Safety: `sp` points inside `[SP_limit,SP_base]` of `T_old`.
    unsafe fn save_callee(sp: *mut u8) -> *mut u8;

    /// Restore `R4-R11` from `*sp`. Returns new SP.
    /// # Safety: `sp` points at a frame written by `save_callee`.
    unsafe fn restore_callee(sp: *const u8) -> *const u8;

    /// Atomically advance circular/priority queue head. Pure `AtomicUsize::CAS` (no mutex).
    /// Returns next task index `N`. Must be 3 cycles worst-case (ldrex/strex loop ≤2 iters).
    fn next_task(current: usize, len: usize) -> usize;

    /// Full switch: save `old_sp` → slot `old`, load `new` slot → `sp`, restore.
    /// # Safety: single-core critical section or CAS-protected; both frames valid.
    unsafe fn switch(current_sp: *mut *mut u8, next_sp: *const u8);
}
```

_Reference impl (ARM, excerpt — `hros-arch-arm/src/switch.rs`):_

```rust
use hros_hal::switch::ContextSwitch;
use core::arch::asm;
pub struct ArmM4Switch;
impl ContextSwitch for ArmM4Switch {
    type Frame = [u32; 8]; // R4-R11
    #[inline(always)]
    unsafe fn save_callee(sp: *mut u8) -> *mut u8 {
        let mut out = sp;
        // SAFETY: sp in TCB bounds; stm IA DB = decrement-before, Ring 0 SASA.
        unsafe { asm!("stmdb {sp}!, {{r4-r11}}", sp = inout(reg) out, options(nostack)) }
        out
    }
    #[inline(always)]
    unsafe fn restore_callee(sp: *const u8) -> *const u8 {
        let mut inp = sp as *mut u8;
        unsafe { asm!("ldmia {sp}!, {{r4-r11}}", sp = inout(reg) inp, options(nostack)) }
        inp
    }
    #[inline(always)]
    fn next_task(cur: usize, len: usize) -> usize { (cur + 1) % len }
    #[inline(always)]
    unsafe fn switch(cur: *mut *mut u8, nxt: *const u8) {
        unsafe {
            let saved = Self::save_callee(*cur);
            core::ptr::write(cur, saved);
            let restored = Self::restore_callee(nxt);
            core::arch::asm!("mov sp, {0}", in(reg) restored, options(nostack));
        }
    }
}
```

---

## 3.2 `InterruptController` — Direct Physical Vector Dispatch

Invariant: peripherals assert voltage/MSI-X → NVIC/GIC/APIC → VTOR/mtvec → `.sram_vectors` handler in <12 cycles (pure HW bounds).

```rust
//! crates/hros-hal/src/irq.rs

/// Physical interrupt controller — no kernel IRQ thread, no IOMMU.
pub trait InterruptController: Sized {
    /// Slots in SRAM vector table (HR-OS: 256 raw, 32 typed IRQs).
    const SLOTS: usize = 32;
    /// Max dispatch latency IRQ→ISR first insn.
    const MAX_LATENCY_CYCLES: usize = 12;

    /// Relocate CPU vector base to `table` (ARM VTOR=0xE000ED08, RISC-V mtvec).
    /// # Safety: run once before enabling IRQs; table 1024-B aligned (ARM).
    unsafe fn relocate(table: *const u8);

    /// Read pending IRQ number (e.g., `ICSR &0x1FF` / `IAR`).
    fn pending() -> Option<usize>;

    /// Install `handler` at `slot` (None = disable). Atomic + DSB/ISB or fence.i.
    /// # Safety: handler is `extern "C" fn()` with interrupt ABI; lives ≥ slot lifetime.
    unsafe fn attach(slot: usize, handler: Option<unsafe extern "C" fn()>);

    /// Acknowledge & clear pending bit `slot` on peripheral status reg.
    /// # Safety: MMIO address derived from `slot` is valid.
    unsafe fn ack(slot: usize);

    /// Returns true if `slot` is an NMI (cannot be masked, WDT path).
    fn is_nmi(slot: usize) -> bool;
}
```

---

## 3.3 `VectorCapabilityEngine` — 256-Bit SIMD Bitmask Validation

Invariant `P(a,C)=(W_{k>>6} >> (k &63)) &1` in O(1). Scalar 3 cyc → vector 1 cyc for 256×4 KiB = 1 MiB.

```rust
//! crates/hros-hal/src/cap.rs

/// Capability token identifier (bit index N).
pub type CapId = u16;

/// 256-bit request mask (4×64) vs task vector `Vcap ∈ {0,1}²⁵⁶`.
/// `authorized = (Vcap & Mreq) == Mreq`.
#[repr(C, align(32))]
#[derive(Copy, Clone)]
pub struct Mask256(pub [u64; 4]);

/// O(1) vector capability engine — scalar + 256-bit SIMD paths.
pub trait VectorCapabilityEngine: Sized {
    /// Granularity shift `M` where block size `S=2^M` (HR-OS M=12 ⇒ 4 KiB).
    const SHIFT: u32 = 12;
    /// Max cycles scalar path.
    const CYCLES_SCALAR: usize = 3;
    /// Max cycles vector path (256 bits).
    const CYCLES_VECTOR: usize = 1;

    /// Scalar predicate `P(addr,C)` : bit-test one 4 KiB block.
    fn verify_scalar(addr: u32, vcap_base: *const u64) -> bool;

    /// Vector predicate for contiguous `len` blocks starting at `addr` (len ≤256).
    /// `mask` encodes the N requested bits. Returns true iff all required bits set.
    /// Must use 256-bit vector ALU when available (AVX2 VANDPS+VPTEST / NEON).
    fn verify_vector(addr: u32, mask: Mask256, vcap_base: *const u64) -> bool;

    /// Build `Mask256` for `[addr, addr+len*4096)`. Returns None if `len>256`.
    fn build_mask(addr: u32, len: usize) -> Option<Mask256>;

    /// Physical `addr` → `CapId` (None = SRAM/flash/unrestricted).
    fn addr_to_cap(addr: u32) -> Option<CapId>;

    /// Atomically claim `id` (`fetch_or`). False if already held.
    fn acquire(id: CapId) -> bool;
    /// Atomically release `id` (`fetch_and !mask`).
    fn release(id: CapId);
    /// True if `id` is free.
    fn available(id: CapId) -> bool;
}
```

_Scalar fallback (compiles on any target, `hros-cap/src/registry.rs` style):_

```rust
#[inline(always)]
fn verify_scalar(addr: u32, base: *const u64) -> bool {
    let k = addr >> 12;
    let idx = (k >> 6) as usize;
    let bit = (k & 63) as u32;
    // SAFETY: base+idx in REGISTRY_BITS; single-cycle load+test.
    unsafe { ((*base.add(idx) >> bit) & 1) == 1 }
}
```

_Vector hook (x86 AVX2 — `hros-arch-x86/src/cap.rs`):_

```rust
#[cfg(target_arch = "x86_64")]
#[inline(always)]
unsafe fn verify_vector_avx(addr: u32, mask: Mask256, base: *const u64) -> bool {
    use core::arch::x86_64::*;
    let vcap = _mm256_loadu_si256(base.add((addr>>12) as usize & !3) as *const __m256i);
    let mreq = _mm256_loadu_si256(mask.0.as_ptr() as *const __m256i);
    let and = _mm256_and_si256(vcap, mreq);
    _mm256_testc_si256(and, mreq) != 0 // (and & mreq) == mreq
}
```

---

## 3.4 `ExecutionBuffer` — I-Cache Fence, Barriers & Opcode Emission

Invariant: `peek/poke` aliasing `EXEC_BUFFER` must not desync I-cache; emission is `volatile` + nomagic + bounded.

```rust
//! crates/hros-hal/src/exec.rs

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum EmitError { Overflow, BadRegister, Unaligned }

/// Executable SRAM buffer: write → fence → call. Size = 4096 (§exec.rs).
pub trait ExecutionBuffer: Sized {
    const SIZE: usize = 4096;
    const ALIGN: usize = 4;

    /// Base pointer of the RWX region (linker `.sram_code`).
    fn base() -> *mut u8;
    /// Bytes written so far (cursor).
    fn len(&self) -> usize;
    /// Remaining capacity.
    fn remaining(&self) -> usize { Self::SIZE - self.len() }

    /// Emit 16-bit halfword (Thumb-2) — volatile store, bounds-checked.
    /// # Safety: caller owns buffer (single-owner contract).
    unsafe fn emit16(&mut self, hw: u16) -> Result<(), EmitError>;

    /// Emit 32-bit word (RV32I / ARM32) — volatile store.
    /// # Safety: same.
    unsafe fn emit32(&mut self, word: u32) -> Result<(), EmitError>;

    /// Data Synchronization Barrier + Instruction Synchronization.
    /// ARM: `dsb; isb`, RISC-V: `fence.i`, x86: `mfence`+`clflush` as needed.
    /// # Safety: must follow last emit, before any `call`.
    unsafe fn flush_icache(&self);

    /// Cast buffer base (+ `offset`) to `fn()->u32` and call (with `base|1` Thumb fix on ARM).
    /// # Safety: `offset` < SIZE, buffer holds valid ISA for `target_arch`.
    unsafe fn call(&self, offset: usize) -> u32;

    /// Convenience: emit `ret` (`BX LR` / `JALR x0,0(ra)`).
    unsafe fn emit_ret(&mut self) -> Result<(), EmitError>;
}
```

_Fence impl (mirrors `kernel/exec.rs`):_

```rust
#[inline(always)]
unsafe fn flush_icache(&self) {
    #[cfg(target_arch="arm")] unsafe { core::arch::asm!("dsb", "isb", options(nostack)) }
    #[cfg(target_arch="riscv32")] unsafe { core::arch::asm!("fence.i", options(nostack)) }
    #[cfg(target_arch="x86_64")] unsafe { core::arch::asm!("mfence", options(nostack)) }
}
```

**Zero-Cost Usage Pattern (no vtable):**

```rust
// ❌ forbidden: dynamic dispatch
// fn run(e: &dyn ExecutionBuffer) { e.flush_icache(); }

// ✅ required: static dispatch — monomorphized, dead-code eliminated
fn run<E: ExecutionBuffer>(buf: &mut E) {
    unsafe {
        buf.emit16(0x4770).unwrap(); // ret
        buf.flush_icache();          // dsb/isb or fence.i inlined
        let v = buf.call(0);         // transmute via base|1 (ARM)
    }
}
struct Kernel<H: Hal> { _p: core::marker::PhantomData<H> }
```

All traits carry `#[inline(always)]` on hot paths; `cargo bloat` must show 0 bytes for unused arch backends.

---

# Deliverable 4 — Mathematical & Hardware Physics Edge Case Watchlist

> Each entry is a first-principles failure of **transistor physics × direct HW execution in Ring 0 SASA**. Mitigations are cycle-quantified.

| #   | Failure Mode                                                            | Physics / Silicon Root Cause                                                                                                                                                                                                               | HR-OS Trigger                                                                         | Severity |
| --- | ----------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------- | -------- |
| 1   | **I-Cache / D-Cache Desynchronization (EXEC_BUFFER aliasing)**          | Harvard cores (Cortex-M7, SiFive, x86) buffer instruction fetch separate from data store; `STR` into `0x20002000` hits D-cache/L1 but I-cache still holds stale zeros → fetch garbage → UsageFault/lockup                                  | JIT writes native `MOVW/MOVT/STR` then `BX` without `fence`                           | Critical |
| 2   | **False Sharing & Cache-Line Bouncing (lock-free CAS ring)**            | MESI coherence: `head` and `tail` on same 64-B line bounce cache ownership between cores per CAS → `ns` stalls, unbounded jitter even though `CAS` is “atomic”                                                                             | `LockFreeTaskQueue { head,tail,tasks[256] }` with `#[repr(align(4))]`                 | High     |
| 3   | **Unaligned 256-Bit SIMD Vector Reads**                                 | AVX `vmovaps` (#GP if addr%32≠0), NEON `vld1q` penalty on 16-B misalign, SiFive vector misaligned load trap; `addr>>12` block index may start mid-word → `Mask256` straddles word boundary                                                 | `verify_vector(addr=0x40021018)` → `k=0x40021` bit33 spans 2 words                    | High     |
| 4   | **Pipeline Hazards During 64+ Core Atomic Storm (CAS thundering herd)** | LL/SC reservation granule + store buffer drain + bus snooping storm → `LDREX/STREX` loop livelock, `fetch_or` retries unbounded → violates 3-cycle scheduling guarantee                                                                    | 64 cores contend `LockFreeTaskQueue.tail`, `REGISTRY_BITS` acquire                    | High     |
| 5   | **VTOR / mtvec / IDT Alignment Fault**                                  | Cortex-M VTOR requires 128/256/1024-B alignment (bits[6:0] RES0); `mtvec` low 2 bits = mode, addr must be 4-B aligned; x86 IDT 8-B entries with DPL checks; misalign → HardFault #13 GPF at boot                                           | `RAM_VECTOR_TABLE` mis-placed, `_trap_hang` unaligned, linker org +0x401              | Critical |
| 6   | **Ring 0 Wild Peek Overwrites System Control State**                    | No MMU → `poke 0xE000ED08 (VTOR)` or `0xE0001004 DWT->CYCCNT` overwrites vector base or cycle counter → instant control-flow hijack / time-base corruption                                                                                 | REPL `poke 0xE000ED08 …` with `SUPERUSER` audit but without range deny list           | Critical |
| 7   | **DMA Write-While-Verify Race (TOCTOU)**                                | DMA engine DMAs _after_ config-time `verify_vector` check; if task reclaims capability between check and `poke(BAR, addr)` (≈10 cycles), DMA writes to revoked region after transfer                                                       | `enforced_poke` → DMA `poke(BAR_ADDR)` split; capability revoked by concurrent core   | High     |
| 8   | **Branch Predictor & Speculative Leakage of Capability Bit**            | Superscalar cores (Cortex-A, x86) speculatively execute `TBZ .FAULT_TRAP` fall-through → transient `STR` touches forbidden peripheral before mispredict squash → observable via cache timing side-channel                                  | 3-instr guard `LSR/LDR/TBZ` without `DSB`/`CSDB`                                      | Medium   |
| 9   | **SysTick Drift vs WWDT Window Violation (Exact Physics)**              | Crystal `±50 ppm` + prescaler rounding → SysTick quantum `Δt=1 ms` actual `83 992..84 008` ticks; WWDT `[t_lower,t_upper]` hard window (forbid early `feed`) → `loop 1000 { poke }` inside `t_lower` → NMI even though quantum not expired | Tight `t_lower=0.8 ms t_upper=1.0 ms` with 84 MHz rounding                            | High     |
| 10  | **ITIM/Flash PT_LOAD Execute-Permission Trap (ELF phantom)**            | LLD marks RW `static mut` sections as RW PT_LOAD only (no X flag); QEMU `sifive_e` (and real PMP) enforces `X≠0` → `fetch` at `0x08000000 ITIM` → `Instruction Access Fault` despite `rwx` in `MEMORY`                                     | RISC-V `EXEC_BUFFER` @ ITIM → QEMU early-return (`native.rs` gate) hides real HW path | Medium   |

### Mitigation Matrix (1:1)

| #   | Mitigation (Code + Hardware)                                                                                                                                                                                                                                                                                                                                                           | Verification                                                                                                                                                                       | Residual Cycle Cost                                                                                          |
| --- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------ |
| 1   | **Mandatory `flush_instruction_cache()`** after every emission batch: `dsb; isb` (ARM) / `fence.i` (RISC-V) / `mfence; clflushopt` (x86) via `ExecutionBuffer` trait; `build.rs` emits `__sram_code` as RWX `0x20002000` (linker `rwx`), RISC-V split `.sram_code`→ITIM + `PHDRS PT_LOAD flags=5 R+X` via `llvm-objcopy --set-section-flags .sram_code=code,data,load,alloc` after LTO | `qemu -d in_asm` trace shows post-emit `fence.i`; ITIM fetch succeeds on patched ELF with `readelf -l` X flag                                                                      | `2 cyc` (barrier)                                                                                            |
| 2   | **Cache-line isolation**: `#[repr(C, align(64))] struct LockFreeTaskQueue { head: AtomicUsize, tail: AtomicUsize, _pad:[u8;56], tasks:[*mut TCB;256] }` + `registry` padded `RegistryBits([AtomicU32;8], _pad:[u32;8])` → head/tail never co-resident; producer/consumer padded separate lines; `ordering: Acquire/Release` not `SeqCst`                                               | `cargo test` `cache_line_check: addr_of!(head)%64==0` ; `bench: 64-core CAS storm σ=0`                                                                                             | `0 cyc` (layout)                                                                                             |
| 3   | **`_mm256_loadu_si256` / `vld1q_u64` unaligned path**: vector engine aligns `base.add((k>>6) & !3)` → loads 32-B aligned chunk, shifts mask by `k&63` in-register (`vshl`/`vperm`), tail-word mask via scalar epilogue; build_mask returns `Option<Mask256>` with `debug_assert!(len<=256)`                                                                                            | unit test `verify_vector` vs `verify_scalar` loop for all `offset 0..63` addresses                                                                                                 | `+1 mov` vs fault                                                                                            |
| 4   | **Bounded CAS + backoff + local queues**: tail CAS loop capped 4 iterations → fallback `sealed RingBuffer` per-core (work-stealing), `WFE`/`SEV` IPI not spin; `registry::acquire` uses `fetch_or` with `Ordering::AcqRel` but isolates `SuperUser` (bit31) in separate word to avoid herd; per-core sharded `CAP_BITS_LOCAL[core][word]` + global OR                                  | `loom` model `64×push_task` terminates ≤12 cyc p95; hardware `SEV` power trace ≤5 mW                                                                                               | ≤12 cyc p95 (vs unbounded)                                                                                   |
| 5   | **`#[repr(align(1024))]` + linker `ASSERT(. &0x3FF==0,"VTOR align")`**: `RAM_VECTOR_TABLE: VectorTable` 1024-B aligned (safety margin vs 128-B min), RISC-V `_trap_hang` `global_asm!(".balign 4; j _trap_hang")`, `build.rs` validates `mtvec &!0x3` at link time (`nm                                                                                                                | grep RAM_VECTOR_TABLE` addr %1024==0)                                                                                                                                              | boot test `VTOR==0x20000400` + `mtvec[1:0]==0`                                                               | `0 cyc`                       |
| 6   | **Deny-list + SuperUser audit-only**: `enforced_poke_u32` adds `deny_range(0xE0000000..0xE00FFFFF)` (SCS/VTOR/DWT) → `Err(PermissionDenied)` even under `is_superuser_active()`; `SUPERUSER_AUDIT_LOG` ring still records attempt for forensics; REPL `help` lists denylist                                                                                                            | `poke 0xE000ED08` → `E002: PERMISSION_DENIED` even with `SUPERUSER` (unit test)                                                                                                    | `+1 range cmp`                                                                                               |
| 7   | **Atomic verify-then-commit with CAS on `DMA Busy` bit**: driver `acquire_dma_channel()` → sets `DMA_CH_BUSY` bit in same word as capability; `enforced_poke(BAR)` CAS-checks bit still set; HW clears on `Ptr_HEAD==Ptr_TAIL` completion IRQ; no `drop_cap` can clear while `BUSY` set (mask `& ~0x80` protected)                                                                     | race test `taskA poke+revoke                                                                                                                                                       |                                                                                                              | DMA poll` 10k iters 0 escapes | `1 CAS` |
| 8   | **`CSDB`/`DSB SY` + `SB` barrier after guard**: ARM `TBZ .FAULT_TRAP; CSDB` (Consumption of Speculative Data Barrier) or `DSB SY+ISB` to stop speculative store; x86 `_mm_lfence` after `VPTEST`; `verify_scalar` emits barrier via `core::hint::black_box` to defeat predictor training                                                                                               | `Spectre-v1` cache-flush+reload harness shows 0 delta in `L1` hit after mispredict (QEMU `icount` + real Cortex-A72 `PMU`)                                                         | `1 cyc` (CSDB)                                                                                               |
| 9   | **Calibrated tick + window hysteresis**: measure `f_CPU` at boot via DWT reference `CYCCNT delta / known delay` ; compute `N = (f×Δt+512)>>10` rounded; set WWDT `[t_lower'=t_lower+2σ, t_upper'=t_upper-2σ]` with `σ=0.1%` (84 ticks) margin; feed only at `t_lower<now<t_upper` (not at quantum start)                                                                               | long-run `1M ticks` histogram `max-min ≤16 ticks` ; `WWDT NMI` 0 false positives in 24h soak                                                                                       | ±84 ticks guardband                                                                                          |
| 10  | **`PHDRS` + `objcopy` dual path**: CI runs `llvm-objcopy --set-section-flags .sram_code=alloc,load,code,data` after link, then `readelf -l                                                                                                                                                                                                                                             | grep LOAD`shows`RWE`; `memory-riscv.x`keeps PHDRS decoupled from`> flash AT`via`__etext`separate LMA; QEMU test matrix runs both`bios none -kernel`(ROM jump) and`-bios <patched>` | `qemu sifive_e` now executes native `2+3;` → `=0x00000005 (5)` not threaded fallback; ITIM `X` flag verified | link-time only                |

---

# Appendix — Cycle Budget Ledger & Invariant Map

## Global Cycle Ledger (@168 MHz, 5.95 ns/cyc)

| Subsystem             | Scalar / Native            | Vector / Upgraded     | HR-OS   | FreeRTOS   | seL4/Linux       |
| --------------------- | -------------------------- | --------------------- | ------- | ---------- | ---------------- |
| Context switch        | 43 cyc 0.255 µs            | same (lock-free 8-12) | **43**  | 84 0.50 µs | 310 1.85 µs      |
| IRQ→ISR               | 12                         | same                  | **12**  | 12-25      | 120-180          |
| Guard per `peek/poke` | 3                          | **1** (256 blocks)    | **1–3** | N/A (nil)  | MPU 300+         |
| IPC 1.5 KiB           | 8 cyc 0.048 µs (cap shift) | same                  | **8**   | 120+       | 310/140+ 0.85 µs |
| DMA 1.5 KiB           | 0 blocked (ring) + 120 cfg | same                  | **0**   | 120 poll   | IOMMU fault      |
| JIT per byte          | 25                         | same                  | **25**  | N/A        | gcc s            |
| `poke` e2e            | 85 cyc 0.50 µs             | 83 cyc                | **≈85** | —          | —                |
| Jitter `σ`            | 0                          | 0                     | **0**   | low        | med              |

## Invariant → Crate → Spec → Test Traceability

| Invariant                        | Crate                                   | Spec File                          | QEMU Test                       |
| -------------------------------- | --------------------------------------- | ---------------------------------- | ------------------------------- |
| SASA VA≡PA                       | `hros-kernel/memory.rs`, `linker/*.x`   | `AXIS-2.md`                        | `peek 0x20003000` echo          |
| 43-cyc switch                    | `hros-hal/switch`, `hros-arch-*/switch` | `AXIS-1.md`                        | `DWT` bench                     |
| O(1) scalar 3 / vector 1         | `hros-cap/registry` + `hros-hal/cap`    | `AXIS-3.md`, `UPGRADE.md` Step1    | `check_access` unit             |
| LL(1) 25 c/B JIT 85 cyc e2e      | `hros-jit/*`                            | `AXIS-4.md`, `E2E-SYSTEM-TRACE.md` | `poke 0x40021018 1` → LED       |
| Zero-copy DMA 8 cyc, 0 copy      | `hros-drivers/pcie`                     | `ZERO-COPY.md`, `DMA.md`           | `submit_transfer` bench         |
| WWDT window + `.FAULT_TRAP` <15c | `hros-kernel/interrupt`                 | `INVALID-OP-CODES.md`              | `peek <unmapped>` → `**FAULT**` |
| WCET RTA `R_i≤D_i` 0 jitter      | `WCEF.md` ledger                        | `WCEF.md` §4                       | 10k-run histogram               |
| No alloc, no vtable, no TODO     | all `crates/*`                          | `RoadMap.md` Cross                 | `rg todo!` + `grep dyn` ∅       |

---

## How to Build This Document

```bash
# from holy-rust/  (workspace root after crate split)
cargo build --target thumbv7em-none-eabihf --release
cargo build --target riscv32imac-unknown-none-elf --release
cargo build --target x86_64-hros-none.json --release   # after Phase 0

# QEMU HIL (both arches)
cargo run --target thumbv7em-none-eabihf --release   # → qemu-system-arm netduinoplus2
cargo run --target riscv32imac-unknown-none-elf --release # → sifive_e

# Expect harness
cargo xtask qemu-arm   # or: expect scripts/qemu-repl.expect

# Verify invariants
rg -n "todo!|unimplemented!|TODO" crates/          # must be empty
rg -n "dyn\s+\w+|Box<dyn" crates/                 # must be empty (no vtable)
cargo clippy -- -D warnings -D clippy::undocumented_unsafe_blocks
llvm-objdump -h target/*/release/hros_kernel | grep sram_code  # RWE
```

---

_Document derived from `HR-OS/AXIS-1.md … AXIS-4.md`, `BENCHMARK.md`, `DMA.md`, `E2E-SYSTEM-TRACE.md`, `INVALID-OP-CODES.md`, `SYNTHESIS.md`, `UPGRADE.md`, `WCEF.md`, `ZERO-COPY.md`, `Holy-Rust-Unikernel-Operating-System.md`, `FORWARD.md` and the `holy-rust` implementation (`Thought.md`, `RoadMap.md`, `src/{capabilities,kernel,compiler,drivers}/*`, `memory*.x`, `build.rs`). All cycle bounds are at the documented 168 MHz Cortex-M4 unless noted as 84 MHz STM32F4._
