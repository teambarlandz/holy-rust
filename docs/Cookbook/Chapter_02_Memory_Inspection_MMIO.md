# Cookbook Chapter 2: Memory Inspection & MMIO

*25 recipes for reading and writing raw memory — SRAM, flash, peripherals,
and the kernel's own data structures.*

**Reference map:**

| Region | ARM Range | Capability needed |
|--------|-----------|-------------------|
| Flash (.text/.rodata) | `0x08000000`+ | none |
| SRAM system | `0x20000000`+ | none |
| SRAM vectors | `0x20000400` | none |
| Capability registry | `0x20001000` | none |
| EXEC_BUFFER | `0x20002000` | none |
| Peripherals | `0x40000000`+ | matching token |

---

## Task 2.01 — Your first peek

**Goal:** Read any SRAM address without ceremony.

```text
holy> peek 0x20000000;
= 0x00000000 (0)
```

SRAM needs no capability. This is the "instant" path of the manifesto:
one volatile load.

## Task 2.02 — Your first poke

**Goal:** Store a word to SRAM and read it back.

```text
holy> poke 0x20000100 0xDEADBEEF;
OK

holy> peek 0x20000100;
= 0xDEADBEEF (3735928559)
```

## Task 2.03 — Scratch variables in raw memory

**Goal:** Use a fixed SRAM cell as persistent storage across REPL lines.

```text
holy> let slot = 0x20000200;
slot = 0x20000200 (536871936)

holy> poke slot 1234;
OK

holy> peek 0x20000200;
= 0x000004D2 (1234)
```

Names resolve at parse time; the address lands in the emitted stream as an
immediate.

## Task 2.04 — Read your own program's flash

**Goal:** Inspect kernel machine code at the reset vector.

```text
holy> peek 0x08000000;
= 0x20003000 (536872960)
```

Word 0 of flash is the initial stack pointer — `0x20003000`, top of the
system SRAM region. You are reading the binary you are running inside.

## Task 2.05 — Verify the reset handler address

**Goal:** Word 1 is the Reset entry point.

```text
holy> peek 0x08000004;
= 0x08000xxx (...)
```

Odd value = Thumb bit set. The core masks bit 0 when branching.

## Task 2.06 — Watch the capability registry change

**Goal:** See claim/drop flip bits in real time at `0x20001000`.

```text
holy> peek 0x20001000;
= 0x00000000 (0)

holy> cap_claim GPIOA;
CAP CLAIMED GPIOA id=0

holy> peek 0x20001000;
= 0x00000001 (1)

holy> cap_drop GPIOA;
CAP RELEASED GPIOA

holy> peek 0x20001000;
= 0x00000000 (0)
```

Bit 0 = GPIOA, bit 31 = SUPERUSER. The registry IS just memory.

## Task 2.07 — Claim two things, read one bitmask

**Goal:** Observe multiple claims packed into a single word.

```text
holy> cap_claim GPIOA;
CAP CLAIMED GPIOA id=0

holy> cap_claim UART0;
CAP CLAIMED UART0 id=2

holy> peek 0x20001000;
= 0x00000005 (5)
```

Bits 0 and 2 set → decimal 5. One load tells you the whole ownership state.

## Task 2.08 — Dump the relocated vector table

**Goal:** Read the SRAM copy of the exception table at `0x20000400`.

```text
holy> peek 0x20000400;
= 0x20003000 (536872960)

holy> peek 0x20000404;
= 0x08000xxx (...)
```

After boot relocation, VTOR points here; these words now dispatch exceptions.

## Task 2.09 — Inspect the JIT buffer before and after

**Goal:** Watch compiled function bytes appear in EXEC_BUFFER.

```text
holy> peek 0x20002000;
= 0x00000000 (0)

holy> fn f() { poke 0x20000200 7 }
FN f DEFINED

holy> peek 0x20002000;
= 0x???????? (non-zero!)
```

Non-zero words = threaded opcodes or Thumb code written by the last run.

## Task 2.10 — Compile-time peek inside expressions

**Goal:** Bind memory contents to a name in one line.

```text
holy> let sp_top = peek 0x08000000;
sp_top = 0x20003000 (536872960)

holy> sp_top - 4;
= 0x20002FFC (...)
```

The read happens while parsing; `sp_top` is frozen forever after.

## Task 2.11 — Arithmetic on addresses

**Goal:** Walk a register block with computed offsets.

```text
holy> let base = 0x20000200;
base = 0x20000200 (536871936)

holy> poke base + 4 1111;
OK

holy> poke base + 8 2222;
OK

holy> peek 0x20000208;
= 0x000008AE (2222)
```

Every address argument is a full expression.

## Task 2.12 — Fill memory with a pattern by hand

**Goal:** Write three descending values to consecutive cells.

```text
holy> poke 0x20000300 3;
OK

holy> poke 0x20000304 2;
OK

holy> poke 0x20000308 1;
OK

holy> peek 0x20000300;
= 0x00000003 (3)
```

No loops exist at top level — but fn bodies can repeat stores (Task 4.x).

## Task 2.13 — Copy a word via peek-then-poke

**Goal:** Manual memcpy, one word.

```text
holy> let v = peek 0x20000200;
v = 0x000004D2 (1234)

holy> poke 0x20000210 0x4D2;
OK
```

Two statements, zero allocation, deterministic timing.

## Task 2.14 — Swap two cells

**Goal:** Exchange contents using one scratch name.

```text
holy> let t = peek 0x20000200;
t = 0x000004D2 (1234)

holy> poke 0x20000200 0x8AE;
OK

holy> poke 0x20000208 0x4D2;
OK
```

You must know both values (peek prints them) or bind them first.

## Task 2.15 — Probe for existence: unmapped vs mapped

**Goal:** Distinguish peripheral space from unmapped space safely.

Unclaimed peripheral access fails SOFTLY at parse time:

```text
holy> peek 0x40011000;
ERR E001: CAPABILITY_VIOLATION - Peripheral token not claimed
```

Truly wild probes fault HARD (Ring 0 has no seatbelts):

```text
holy> peek 0x60000000;

**FAULT: core exception, halted**
```

Know your map before probing.

## Task 2.16 — SuperUser writes to unmapped MMIO

**Goal:** Reach registers outside the capability map, with an audit trail.

```text
holy> cap_claim SUPERUSER;
CAP CLAIMED SUPERUSER id=31

holy> poke 0x42200000 0x1;
OK

holy> sys_audit;
--- SUPERUSER AUDIT LOG ---
Total Unsafe Operations: 1
Recent Events:
ADDR: 0x42200000 | VAL: 0x00000001 | CYCLES: ...
```

Power without accountability is just chaos — hence the ring buffer.

## Task 2.17 — Audit-log multiple operations

**Goal:** Build a forensic record of every bypass write.

```text
holy> poke 0x50000000 0xA;
OK

holy> poke 0x50000004 0xB;
OK

holy> sys_audit;
--- SUPERUSER AUDIT LOG ---
Total Unsafe Operations: 3
Recent Events:
ADDR: 0x50000000 | VAL: 0x0000000A | CYCLES: ...
ADDR: 0x50000004 | VAL: 0x0000000B | CYCLES: ...
```

16-entry ring: oldest entries fall off after 16 events.

## Task 2.18 — Reads under SuperUser are not logged

**Goal:** Understand the audit policy: writes only.

```text
holy> peek 0x50000000;
= 0x00000000 (0)

holy> sys_audit;
--- SUPERUSER AUDIT LOG ---
Total Unsafe Operations: 3
```

Count unchanged — side-effect-free reads don't need forensics.

## Task 2.19 — Drop SuperUser, regain safety net

**Goal:** Return to guarded mode so typos fail loudly again.

```text
holy> cap_drop SUPERUSER;
CAP RELEASED SUPERUSER

holy> poke 0x42200000 0x2;
ERR E002: PERMISSION_DENIED - Unmapped MMIO access requires SuperUserCap
```

E002 fires because the address is unmapped AND no token covers it.

## Task 2.20 — reg_set_bit on a config cell

**Goal:** Turn on feature flags stored in SRAM.

```text
holy> poke 0x20000240 0x0000;
OK

holy> reg_set_bit 0x20000240 0;
OK

holy> reg_set_bit 0x20000240 4;
OK

holy> peek 0x20000240;
= 0x00000011 (17)
```

## Task 2.21 — reg_clr_bit to mask flags off

**Goal:** Clear bit 0 while preserving bit 4.

```text
holy> reg_clr_bit 0x20000240 0;
OK

holy> peek 0x20000240;
= 0x00000010 (16)
```

## Task 2.22 — Count bits set with div/mod arithmetic

**Goal:** Sum the bits of a byte-sized value.

For value `0b1011` = 11:

```text
holy> let b = 11;
b = 0x0000000B (11)

holy> b % 2 + b / 2 % 2 + b / 4 % 2 + b / 8 % 2;
= 0x00000003 (3)
```

Four terms, four bits, pure arithmetic — no AND/OR operators needed.

## Task 2.23 — Byte extraction from a word

**Goal:** Pull each byte of `0x12345678`.

```text
holy> let w = 0x12345678;
w = 0x12345678 (305419896)

holy> w % 256;
= 0x00000078 (120)

holy> w / 256 % 256;
= 0x00000056 (86)

holy> w / 65536 % 256;
= 0x00000034 (52)

holy> w / 16777216;
= 0x00000012 (18)
```

## Task 2.24 — Alignment discipline

**Goal:** Keep 32-bit accesses on 4-byte boundaries.

```text
holy> peek 0x20000204;
= ... (fine — divisible by 4)
```

An unaligned volatile access on Cortex-M3/M4 either faults or misbehaves
depending on CCR.UNALIGN_TRP. The kernel does not fix your alignment.
Rule: addresses divisible by 4, always.

## Task 2.25 — The full loop: inspect, modify, verify

**Goal:** A complete audit-grade sequence on one cell.

```text
holy> peek 0x20000300;
= 0x00000003 (3)

holy> reg_set_bit 0x20000300 7;
OK

holy> peek 0x20000300;
= 0x00000083 (131)
```

Peek state, transform minimally, verify result. This is the rhythm of all
bare-metal work.

---
*End of Chapter 2 — 50/250*
