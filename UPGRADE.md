## REPL Built-in Reference

These commands execute **immediately in the REPL**. They are kernel service verbs and
meta-commands, not Holy Rust language statements: they are never compiled into `EXEC_BUFFER`
and cannot appear inside `fn` bodies.

### Capability verbs

| Command                 | Requires      | Output                                  |
| ----------------------- | ------------- | --------------------------------------- |
| `cap_claim NAME;`       | free resource | `CAP CLAIMED <NAME> id=N` or `CAP BUSY` |
| `cap_drop NAME;`        | held token    | `CAP RELEASED` / `CAP NOT HELD`         |
| `reg_set_bit ADDR BIT;` | matching cap  | `OK`                                    |
| `reg_clr_bit ADDR BIT;` | matching cap  | `OK`                                    |

### Driver verbs (v0.2.0)

| Command            | Peripheral                                    | Capability gate | Output                 |
| ------------------ | --------------------------------------------- | --------------- | ---------------------- |
| `pwm PERIOD DUTY;` | TIM2 (ARM) / PWM0 (riscv32)                   | Timer0 token    | `PWM ARR=<n> CCR1=<n>` |
| `pwm_duty DUTY;`   | same, live compare update                     | Timer0 token    | `OK`                   |
| `spi_tx BYTE;`     | SPI1 (ARM) / SPI0 (riscv32), full-duplex byte | Spi0 token      | `SPI RX=<n>`           |

### Diagnostics and meta

| Command       | Output                                                           |
| ------------- | ---------------------------------------------------------------- |
| `sys_audit;`  | SuperUser access log dump                                        |
| `flash_test;` | FPEC model probe result                                          |
| `bench;`      | threaded vs native cycles/exec + speedup ratio (1000 iterations) |
| `banner`      | reprints boot banner                                             |
| `help`        | full command reference                                           |

---

## Embedded-Standard Assessment

### What Holy Rust can do today

- Interactive MMIO control via `poke` / `peek`
- Arithmetic and compute kernels as persistent named functions (`fn`, stored on ARM)
- Capability-scoped peripheral access, fail-closed at parse time
- PWM output configuration and SPI byte transfers
- All code JIT-compiled to native Thumb-2 / RV32I with deterministic cycle bounds

Sufficient today for: sensor polling loops (manual unroll), actuator control,
register-level device bring-up, and deterministic test harnesses on MCU-class targets.

### What disqualifies it from general embedded projects today

| Gap                                          | Impact                                                   | Fix path                                             |
| -------------------------------------------- | -------------------------------------------------------- | ---------------------------------------------------- |
| No loops (`loop N {}`) in grammar            | iterative algorithms require manual unrolling            | AXIS-4 spec exists; implement emitter support        |
| `fn` are zero-arg                            | no parameterized drivers or reusable abstractions        | needs calling-convention design                      |
| No conditional branches                      | control flow limited to arithmetic only                  | needs bounded-if spec preserving O(1) parse          |
| Device verbs (`pwm`, `spi_tx`) are REPL-only | cannot compose peripheral sequences into stored programs | promote to stream primitives alongside lit/add/write |
| riscv32 has no program store                 | DTIM fully carved at 8K                                  | ITIM+PRCI enable on real HW restores 4K              |
| Flash persistence is a stub under QEMU       | store lost on reset (QEMU model limitation)              | real FPEC works; verify on silicon                   |
| No structs, arrays, strings, pointers        | data model = u32 scalars only                            | long-term language evolution                         |

### Verdict

**Not yet a general-purpose embedded language.** Holy Rust v0.2.0 is a deterministic
control and scripting layer — think Forth with proofs — designed as the companion to a
host-compiled no_std Rust kernel, not as a replacement for C/Zephyr application development.

The path from scripting layer to application language is ordered by leverage:

1. **Loops** - unlock every iterative pattern
2. **fn arguments** - unlock driver parameterization
3. **Bounded conditionals** - unlock decision logic without breaking determinism proofs
4. **L3 verb promotion** - unlock peripheral-sequence composability

Each step must preserve: LL(1) single-pass parse, no heap allocation, capability checks
at compile time, and cycle-counted bounds per WCEF.md.
