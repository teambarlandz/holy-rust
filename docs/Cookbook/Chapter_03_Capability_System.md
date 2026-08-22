# Cookbook Chapter 3: The Capability System

*25 recipes covering the entire token lifecycle: claiming, releasing,
bypassing, auditing, and reasoning about ownership.*

**Token table:**

| Name | id | Guards (ARM) |
|------|----|--------------|
| `GPIOA` | 0 | `0x40020000`–`0x400203FF` |
| `GPIOB` | 1 | `0x40020400`–`0x400207FF` |
| `UART0` | 2 | `0x40011000`–`0x400113FF` |
| `SPI0`  | 3 | `0x40013000`–`0x400133FF` |
| `I2C0`  | 4 | `0x40015400`–`0x400157FF` |
| `TIMER0`| 5 | `0x40000000`–`0x400003FF` |
| `DMA0`  | 6 | `0x40002000`–`0x400023FF` |
| `SUPERUSER` | 31 | bypass + audit |

---

## Task 3.01 — Claim your first token

```text
holy> cap_claim GPIOA;
CAP CLAIMED GPIOA id=0
```

One atomic OR into the registry bitfield. O(1), no allocation, no lock.

## Task 3.02 — Discover double-claim protection

```text
holy> cap_claim GPIOA;
CAP CLAIMED GPIOA id=0

holy> cap_claim GPIOA;
CAP BUSY GPIOA
```

The second claim fails atomically — the bit was already set. This is the
entire concurrency model: single-bit test-and-set.

## Task 3.03 — Release and re-claim

```text
holy> cap_drop GPIOA;
CAP RELEASED GPIOA

holy> cap_claim GPIOA;
CAP CLAIMED GPIOA id=0
```

Tokens are reusable; linearity means *at most one* holder at a time.

## Task 3.04 — Dropping something you don't hold

```text
holy> cap_drop SPI0;
CAP NOT HELD SPI0
```

The registry distinguishes "free" from "never claimed this session" — both
are safe no-ops that refuse to lie.

## Task 3.05 — Typo-proofing: unknown resources fail loudly

```text
holy> cap_claim GPIAO;
ERR UNKNOWN RESOURCE GPIAO
```

(That's a letter O, not a zero.) Name resolution is an exact-match table;
there is no fuzzy matching to silently grant the wrong peripheral.

## Task 3.06 — Every token has a stable numeric id

```text
holy> cap_claim TIMER0;
CAP CLAIMED TIMER0 id=5

holy> cap_claim DMA0;
CAP CLAIMED DMA0 id=6
```

ids are the bit positions in the registry word — useful when reading the
registry directly (Task 3.09).

## Task 3.07 — Enforcement fires at PARSE time

```text
holy> poke 0x40000000 0x89ABCDEF;
ERR E001: CAPABILITY_VIOLATION - Peripheral token not claimed
```

Nothing executed. The parser rejected the statement before emitting a single
opcode. Bad code never runs — it doesn't even compile.

## Task 3.08 — Claim unlocks exactly its own range

```text
holy> cap_claim TIMER0;
CAP CLAIMED TIMER0 id=5

holy> poke 0x40000000 0x1;
OK

holy> poke 0x40013000 0x1;
ERR E001: CAPABILITY_VIOLATION - Peripheral token not claimed
```

TIMER0 covers only `0x40000000..0x400003FF`. SPI0's registers stay sealed.

## Task 3.09 — Read the whole ownership state from memory

```text
holy> peek 0x20001000;
= 0x00000020 (32)
```

Bit 5 set = TIMER0 held. The capability system is one word of SRAM you can
inspect with ordinary tools — no hidden kernel state.

## Task 3.10 — Stack up multiple claims

```text
holy> cap_claim GPIOA;
CAP CLAIMED GPIOA id=0

holy> cap_claim GPIOB;
CAP CLAIMED GPIOB id=1

holy> cap_claim UART0;
CAP CLAIMED UART0 id=2

holy> peek 0x20001000;
= 0x00000007 (7)
```

Bits 0+1+2 = 7. Ports A, B, and the console UART, all owned by you.

## Task 3.11 — Drop everything in reverse order

```text
holy> cap_drop UART0;
CAP RELEASED UART0

holy> cap_drop GPIOB;
CAP RELEASED GPIOB

holy> cap_drop GPIOA;
CAP RELEASED GPIOA

holy> peek 0x20001000;
= 0x00000020 (20)
```

Registry returns to just-TIMER0 state. Order doesn't matter — each release
clears exactly one bit.

## Task 3.12 — Function bodies inherit the check

**Goal:** Definition-time enforcement catches bad addresses early.

```text
holy> fn bad() { poke 0x40020400 1 }
ERR E001: CAPABILITY_VIOLATION - Peripheral token not claimed
```

GPIOB unclaimed → the whole fn definition is rejected. You cannot stash
illegal code inside a function for later.

## Task 3.13 — Define the same fn AFTER claiming

```text
holy> cap_claim GPIOB;
CAP CLAIMED GPIOB id=1

holy> fn ok() { poke 0x40020400 1 }
FN ok DEFINED

holy> ok();
OK
```

The check runs once, at definition. Claims held now cover calls forever after
— but see Task 3.14.

## Task 3.14 — Runtime enforcement still guards execution

**Goal:** Understand why dropping mid-session still matters.

```text
holy> cap_drop GPIOB;
CAP RELEASED GPIOB

holy> ok();
OK
```

The compiled stream executes raw volatile stores — runtime pokes inside fn
bodies were validated at definition time. Top-level `poke` statements get the
fresh parse-time check every line. Defense in depth: two layers, different
moments, same registry.

## Task 3.15 — SUPERUSER claims like any other token

```text
holy> cap_claim SUPERUSER;
CAP CLAIMED SUPERUSER id=31

holy> cap_claim SUPERUSER;
CAP BUSY SUPERUSER
```

Bit 31 of the same bitfield. No special casing in storage — special casing
lives in `enforced_poke_u32`.

## Task 3.16 — SuperUser bypasses peripheral checks

```text
holy> poke 0x40013000 0x55;
OK
```

SPI0 was never claimed, yet the write succeeded — the SuperUser branch
short-circuits the registry lookup entirely.

## Task 3.17 — ...but logs every bypass write

```text
holy> sys_audit;
--- SUPERUSER AUDIT LOG ---
Total Unsafe Operations: 1
Recent Events:
ADDR: 0x40013000 | VAL: 0x00000055 | CYCLES: ...
```

Address, value, cycle timestamp. Accountability is not optional.

## Task 3.18 — Fill the 16-entry ring and watch wraparound

```text
holy> poke 0x50000010 1;
OK

holy> poke 0x50000014 2;
OK

holy> poke 0x50000018 3;
OK
```

(after 16 total events)

```text
holy> sys_audit;
--- SUPERUSER AUDIT LOG ---
Total Unsafe Operations: 16
Recent Events:
ADDR: 0x50000004 | VAL: 0x00000005 | CYCLES: ...
...
ADDR: 0x50000018 | VAL: 0x00000003 | CYCLES: ...
```

Oldest entries evicted; `total_audits` keeps the true lifetime count.

## Task 3.19 — Reads under SuperUser stay free

```text
holy> peek 0x40013000;
= 0x00000000 (0)
```

No log entry added. Audit policy: writes are dangerous, reads are cheap.

## Task 3.20 — Exit god mode deliberately

```text
holy> cap_drop SUPERUSER;
CAP RELEASED SUPERUSER

holy> poke 0x40013000 0x55;
ERR E001: CAPABILITY_VIOLATION - Peripheral token not claimed
```

Safety rails snap back instantly — one bit clear, full enforcement.

## Task 3.21 — Grant audit counting

**Goal:** Track how many times SuperUser was granted this boot.

Each successful `cap_claim SUPERUSER` bumps `SUPERUSER_AUDIT_COUNT`
(a static AtomicU32 in `.rodata`). Claim/drop cycles and watch `sys_audit`
totals across sessions grow monotonically — grants are counted even after
the token is released.

## Task 3.22 — Ownership discipline: claim → use → drop per task

**Goal:** Structure interactive work like a linear-type program.

```text
holy> cap_claim GPIOA;
CAP CLAIMED GPIOA id=0

holy> poke 0x40020018 0x20;
OK

holy> cap_drop GPIOA;
CAP RELEASED GPIOA
```

Minimum window of ownership. The Rust-side `Cap<T>` API (no `Copy`, no
`Clone`, no implicit `Drop`) enforces the identical discipline at compile
time for kernel-mode drivers.

## Task 3.23 — Two-layer defense summary check

**Goal:** Verify both enforcement layers independently.

Layer 1 (parse): unclaimed top-level poke → `E001` before compilation.
Layer 2 (runtime): enforced_poke_u32 re-checks the registry on every
EnforcedPoke outcome — belt and suspenders, zero cost when clean.

```text
holy> peek 0x20001000;
= 0x00000000 (0)

holy> poke 0x40020000 0x1;
ERR E001: CAPABILITY_VIOLATION - Peripheral token not claimed
```

## Task 3.24 — Registry arithmetic: decode arbitrary states

**Goal:** Interpret any registry word by hand.

Value `0x80000021`: bits 0 (GPIOA), 5 (TIMER0), 31 (SUPERUSER).

```text
holy> let r = 0x80000021;
r = 0x80000021 (2147483681)

holy> r % 2;
= 0x00000001 (1)
```

GPIOA held. Divide by powers of two, mod 2, for each bit you care about.

## Task 3.25 — The nuclear reset: reboot clears everything

The registry lives in `.capability_registry` (SRAM) and is initialized to
zero at boot (`init_data_bss`). Power-cycle or debugger-reset returns all 256
resource slots to free — no persistent locks, no stale ownership, ever.

```text
holy> peek 0x20001000;
= 0x00000000 (0)
```

---
*End of Chapter 3 — 75/250*
