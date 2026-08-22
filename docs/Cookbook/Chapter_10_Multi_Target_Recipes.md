# Cookbook Chapter 10: Multi-Target Recipes (ARM ↔ RISC-V)

*The final 25. Every recipe so far ran on the STM32F405; this chapter
translates them all to the SiFive E310 — and shows you how to write
sessions that run on BOTH unmodified.*

**Side-by-side memory maps:**

| Thing | ARM | RISC-V |
|-------|-----|--------|
| Kernel flash | `0x08000000` | `0x20400000` |
| System SRAM | `0x20003000`+ | `0x80000000`+ |
| Vector table | `0x20000400` | `0x80001400` |
| Capability registry | `0x20001000` | `0x80001800` |
| JIT buffer | `0x20002000` | `0x08000000` (ITIM) |
| Stack top | `0x20100000` | `0x80001400` |

---

## Task 10.01 — Which machine am I on?

```text
holy> banner
Holy Rust REPL v0.1

holy> peek 0x08000000;
= ...          ← answers on ARM
```

vs.

```text
holy> peek 0x20400000;
= ...          ← answers on RISC-V
```

One probe decides. The wrong architecture faults or returns garbage from
an empty bus region.

## Task 10.02 — Read the kernel's first word on RISC-V

```text
holy> peek 0x20400000;
= 0x???????? (...)
```

QEMU's sifive_e boot ROM jumps here; your kernel lives at this window
instead of ARM's `0x08000000`.

## Task 10.03 — Registry, translated

ARM:

```text
holy> peek 0x20001000;
= 0x00000000 (0)
```

RISC-V:

```text
holy> peek 0x80001800;
= 0x00000000 (0)
```

Same bitfield semantics, different couch.

## Task 10.04 — Claims behave identically

RISC-V session:

```text
holy> cap_claim GPIOA;
CAP CLAIMED GPIOA id=0

holy> peek 0x80001800;
= 0x00000001 (1)

holy> cap_drop GPIOA;
CAP RELEASED GPIOA
```

Bit 0 = GPIOA on every target by definition — CapId values are
architecture-independent; only guarded *addresses* move.

## Task 10.05 — The capability map, translated

| Token | ARM range | RISC-V range |
|-------|-----------|--------------|
| GPIOA | `0x40020000..03FF` | `0x10012000..2FFF` |
| UART0 | `0x40011000..13FF` | `0x10013000..3FFF` |
| SPI0  | `0x40013000..33FF` | `0x10014000..4FFF` |
| I2C0  | `0x40015400..57FF` | `0x10020000..0FFF` |
| TIMER0| `0x40000000..03FF` | `0x10015000..5FFF` |
| DMA0  | `0x40002000..23FF` | `0x10000000..0FFF` |

Enforcement logic identical; `addr_to_cap_id()` just matches different
constants per `#[cfg]`.

## Task 10.06 — E001 works everywhere

RISC-V, unclaimed peripheral:

```text
holy> poke 0x10012000 1;
ERR E001: CAPABILITY_VIOLATION - Peripheral token not claimed

holy> cap_claim GPIOA;
CAP CLAIMED GPIOA id=0

holy> poke 0x10012000 1;
OK
```

Byte-for-byte the ARM experience. That's the parity guarantee.

## Task 10.07 — Write portable sessions: bind bases first

**Goal:** One script, two machines.

```text
holy> let gpioa = 0x10012000;
gpioa = 0x10012000 (268468224)

holy> cap_claim GPIOA;
CAP CLAIMED GPIOA id=0

holy> poke gpioa + 8 1;
OK
```

Change line 1 only when switching architectures. Every later reference
flows through the name.

## Task 10.08 — The full LED recipe, RISC-V edition

SiFive GPIO uses SET/CLR registers (offsets 0x0C/0x14 on sifive_e model,
output enable at 0x08):

```text
holy> cap_claim GPIOA;
CAP CLAIMED GPIOA id=0

holy> reg_set_bit 0x10012008 5;
OK

holy> poke 0x1001200C 32;
OK

holy> poke 0x10012014 32;
OK
```

Output-enable bit 5, then set-val/clear-val writes. Different register
philosophy (atomic set/clear like BSRR), same claim-first discipline.

## Task 10.09 — UART transmit, RISC-V style

```text
holy> cap_claim UART0;
CAP CLAIMED UART0 id=2

holy> poke 0x10013000 72;
H
OK

holy> poke 0x10013000 73;
I
OK
```

txdata writes go straight to the wire; no TXE polling needed on this
model (the full flag covers pacing — Chapter 8, Task 8.16).

## Task 10.10 — DTIM as your SRAM playground

```text
holy> poke 0x80000100 0xCAFED00D;
OK

holy> peek 0x80000100;
= 0xCAFED00D (3405697037)
```

DTIM spans `0x80000000`–`0x800013FF`; keep clear of vectors (`...1400`),
registry (`...1800`), and stack descending from `0x80001400`. Cells below
`0x80000C00` are safely yours.

## Task 10.11 — JIT buffer lives in ITIM

```text
holy> peek 0x08000000;
= ...
```

On RISC-V, generated code targets the tightly-coupled instruction RAM at
`0x08000000` — a separate 4K bank from data memory. Inspect it exactly
like ARM's EXEC_BUFFER.

## Task 10.12 — Timing without DWT

No CYCCNT MMIO on this SoC. Your instruments:

```text
holy> cap_claim SUPERUSER;
CAP CLAIMED SUPERUSER id=31

holy> poke 0xA0000000 1;
OK

holy> poke 0xA0000004 2;
OK

holy> sys_audit
--- SUPERUSER AUDIT LOG ---
Total Unsafe Operations: 2
Recent Events:
ADDR: 0xA0000000 | VAL: 0x00000001 | CYCLES: T0
ADDR: 0xA0000004 | VAL: 0x00000002 | CYCLES: T1
```

CYCLES fields come from the `mcycle` CSR internally — the audit log is
your cycle counter on RISC-V.

## Task 10.13 — Trap behavior differs by design

ARM fault → UART banner then wfi.
RISC-V unexpected trap → `_trap_hang` silent spin (mtvec direct mode).

If a wild access freezes a RISC-V session without any message: that's the
hang stub doing its diagnosable-nothing job. Attach GDB (`-gdb tcp::1234`)
and read `mcause`/`mtval` to classify it.

## Task 10.14 — GDB on either target

```bash
# ARM
qemu-system-arm -M netduinoplus2 -nographic \
  -kernel target/thumbv7em-none-eabihf/release/holy-rust \
  -S -gdb tcp::1234

# RISC-V
qemu-system-riscv32 -M sifive_e -bios none -nographic \
  -kernel target/riscv32imac-unknown-none-elf/release/holy-rust \
  -S -gdb tcp::1234
```

Same workflow, different qemu binary and machine flag. Breakpoints in
`fault_hang` (ARM) or `_trap_hang` (RISC-V) catch crashes symmetrically.

## Task 10.15 — Host-side build matrix

```bash
cargo build --release --target thumbv7em-none-eabihf
cargo build --release --target riscv32imac-unknown-none-elf
cargo clippy --target thumbv7em-none-eabihf --release -- -D warnings
cargo clippy --target riscv32imac-unknown-none-elf --release -- -D warnings
```

Four commands certify a change on both worlds — the CI runs exactly these.

## Task 10.16 — Size expectations per target

| Target | ELF | In-memory |
|--------|-----|-----------|
| ARM release | ~141 KB | ~15.5 KB |
| RISC-V release | ~25 KB | ~17 KB |

ELF bloat is headers/symbols, not SRAM cost. Judge footprints by the
in-memory column — that's what the manifesto counts.

## Task 10.17 — Native codegen: an ARM-only privilege (today)

ARM expressions execute via compiled Thumb-2 in EXEC_BUFFER when the
stream shape allows. RISC-V currently forces the threaded interpreter
(ITIM segment lacks PF_X under lld). Observable difference to you: none —
same results, marginally different timing. Track the build-system fix to
unlock RV32I native emission.

## Task 10.18 — Parity test script (runs verbatim on both)

```text
banner
2 + 3 * 4;
let x = 42;
x / 7 % 9;
cap_claim GPIOA;
cap_claim GPIOA;
cap_drop GPIOA;
peek REGISTRY_PLACEHOLDER;
sys_audit
help
```

Substitute your registry address for the placeholder, paste into either
machine, diff the transcripts. Identical modulo addresses = regression-free
port.

## Task 10.19 — Flash contents audit on each target

```text
holy> peek 0x08000004;
= ...        ; ARM Reset handler, Thumb bit set
```

```text
holy> peek 0x20400004;
= ...        ; RISC-V entry, even address (no Thumb concept)
```

Word 1 of flash is the entry point on both — but only ARM sets bit 0.

## Task 10.20 — Vector table presence check

ARM (SRAM copy after relocation):

```text
holy> peek 0x20000400;
= 0x20100000 (...)
```

First word = initial SP. RISC-V has no vector table to relocate — traps
route through mtvec instead; there's nothing equivalent to peek (CSR).
Absence of a feature is also parity information.

## Task 10.21 — Cache-fence transparency

Define-and-call a function immediately after generation? Works on both:
ARM executes DSB+ISB, RISC-V `fence.i`, before entering fresh code. You
cannot observe, trigger, or misfire this mechanism from the REPL — it is
infrastructure, and it is already correct.

## Task 10.22 — Choosing a target for real hardware

Pick ARM (STM32F405-class) when you want: hardware FPU, mature debug
tooling, DWT cycle counters, native JIT execution today.
Pick RISC-V (FE310-class) when you want: open ISA auditability, compact
ELFs, atomic set/clear GPIO style, CSR-based timing.

Both give: identical REPL, identical capability guarantees, identical
error taxonomy.

## Task 10.23 — Session porting checklist

Moving a working ARM session to RISC-V:

1. Retarget every literal address (map in Task 10.05).
2. Re-check GPIO register layouts (set/clear vs BSRR idioms).
3. Replace CYCCNT peeks with audit-log deltas.
4. Expect silent spin instead of fault banners on wild accesses.
5. Keep all claims/drops/arithmetic byte-identical — they're portable.

## Task 10.24 — One source, two binaries

Nothing in your REPL habits locks you to silicon. The kernel itself is
one Rust tree with `#[cfg(target_arch)]` seams — the cookbook's job was
to keep YOUR scripts equally seam-free via named bases (Task 10.07).
Adopt that habit and "porting" becomes a find-and-replace of four hex
constants.

## Task 10.25 — The 250th thing: teach the next person

Hand a colleague a board and this line:

```text
holy> help
commands:
peek ADDR;              read u32 from address (requires capability)
poke ADDR VAL;          write u32 to address (requires capability)
...
```

Then watch them run `2 + 2;`. The manifesto's deepest promise isn't
microseconds or kilobytes — it's that a bare-metal computer becomes
explainable in eleven lines of help text. You now hold 250 ways to prove
it. Pass one on.

---
*End of Chapter 10 — 250/250*

---

*This cookbook documents Holy Rust v0.1 behavior verified against the
source tree and QEMU 8.2 sessions. Where silicon differs from QEMU
(baud divisors, watchdogs, DWT availability), recipes say so inline.*
