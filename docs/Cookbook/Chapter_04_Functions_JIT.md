# Cookbook Chapter 4: Functions & the JIT

*25 recipes for defining, calling, and reasoning about JIT-compiled
functions. Two live functions maximum — spend your slots wisely.*

**Hard limits:**

| Resource | Limit | Error |
|----------|-------|-------|
| Live functions | 2 | `ERR FN TABLE FULL` |
| Body token stream | 32 words | `ERR STREAM FULL` |
| Name length | 16 chars | `ERR NAME TOO LONG` |
| Definition length | one REPL line, 128 bytes | truncated input |

---

## Task 4.01 — Define and call your first function

```text
holy> fn hello() { poke 0x20000100 1 }
FN hello DEFINED

holy> hello();
OK
```

The body compiled into threaded opcodes; the call executed them. `OK`
because side effects don't yield values.

## Task 4.02 — Verify the side effect landed

```text
holy> peek 0x20000100;
= 0x00000001 (1)
```

Functions write memory; you verify with ordinary peeks. There is no
mystery state — everything lands in addressable SRAM.

## Task 4.03 — A function that computes

**Goal:** Fold an arithmetic body at definition time.

```text
holy> fn calc() { 2 + 3 * 4 }
FN calc DEFINED

holy> calc();
OK
```

Surprise: `OK`, not `20`. Bare expressions inside bodies are **folded to
constants during definition** and emit zero runtime words. Bodies exist for
side effects — pokes, peeks, calls — not math.

## Task 4.04 — Do your math at top level instead

```text
holy> 2 + 3 * 4;
= 0x00000014 (20)
```

Top-level expressions print their value. Left-to-right: `(2+3)*4 = 20`.

## Task 4.05 — A function whose poke uses computed values

**Goal:** Constants are baked into the emitted stream.

```text
holy> fn set_flag() { poke 0x20000200 255 }
FN set_flag DEFINED

holy> set_flag();
OK

holy> peek 0x20000200;
= 0x000000FF (255)
```

The address and value became literals inside the token stream — parse-time
resolution, zero lookup cost at run time.

## Task 4.06 — peek inside a body leaves its value behind

```text
holy> fn sample() { peek 0x20000200 }
FN sample DEFINED

holy> sample();
OK
```

Body peeks push onto the VM operand stack; the call program discards it.
Useful for sequencing reads, less useful for return values — see Task 4.07.

## Task 4.07 — Return values come from top-level streams only

**Goal:** Get printed results from inline execution, not from fns.

```text
holy> peek 0x20000200;
= 0x000000FF (255)
```

Design consequence: functions orchestrate hardware; top-level lines report
state. If you need a printed value, peek directly.

## Task 4.08 — Functions calling functions

**Goal:** Splice one body into another at definition time.

```text
holy> fn low() { poke 0x20000204 0 }
FN low DEFINED

holy> fn high() { low() poke 0x20000204 1 }
FN high DEFINED

holy> high();
OK

holy> peek 0x20000204;
= 0x00000001 (1)
```

`high()`'s stream contains `low()`'s words inlined, then its own store.
Calls are copies — no indirect jumps, no call overhead.

## Task 4.09 — Splicing costs words: know your budget

Each `poke` ≈ 3 stream words (lit addr, lit val, write_prim). A call adds
its callee's entire body. With 32 words per body:

```text
holy> fn big() { low() low() low() }
FN big DEFINED
```

Three splices of a 3-word body plus halt — comfortably inside budget.
A 15-poke body would not be.

## Task 4.10 — Redefinition is forbidden

```text
holy> fn low() { poke 0x20000208 9 }
ERR FN REDEFINED
```

Names are permanent for the session. This guarantees that already-verified
bodies can never be silently swapped underneath you.

## Task 4.11 — Plan around two slots

```text
holy> fn a() { poke 0x20000210 1 }
FN a DEFINED

holy> fn b() { poke 0x20000214 2 }
FN b DEFINED

holy> fn c() { poke 0x20000218 3 }
ERR FN TABLE FULL
```

Two slots force small, composable designs: one primitive, one orchestrator.

## Task 4.12 — The orchestrator/primitive split

**Goal:** Maximize what two slots can do.

Slot 1 = primitive actions:

```text
holy> fn prim() { poke 0x20000220 1 poke 0x20000224 2 }
FN prim DEFINED
```

Slot 2 = sequence with repetition:

```text
holy> fn seq() { prim() prim() prim() }
FN seq DEFINED

holy> seq();
OK
```

Six stores from two definitions — splicing multiplies your reach.

## Task 4.13 — Calling an undefined name

```text
holy> ghost();
ERR UNKNOWN SYMBOL
```

## Task 4.14 — Capability checks apply to definitions

```text
holy> cap_claim GPIOA;
CAP CLAIMED GPIOA id=0

holy> fn gpio_on() { poke 0x40020018 32 }
FN gpio_on DEFINED

holy> fn sneaky() { poke 0x40013000 85 }
ERR E001: CAPABILITY_VIOLATION - Peripheral token not claimed
```

GPIOB/SPI addresses stay sealed even inside fn bodies. Enforcement has no
blind spots.

## Task 4.15 — Definitions persist for the whole session

```text
holy> gpio_on();
OK

holy> gpio_on();
OK
```

Call it ten lines later — still works. The compiler is a static; symbols
and bodies live until reset.

## Task 4.16 — One line per definition

The REPL is line-oriented. A body must be complete on its 128-byte line:

```text
holy> fn broken() { poke 0x20000230 1
ERR UNEXPECTED TOKEN
```

No continuation lines, no brace balancing across inputs. Keep bodies terse.

## Task 4.17 — Statement separators inside bodies

Semicolons between statements are accepted; the final statement may omit
the trailing semicolon before `}`:

```text
holy> fn tidy() { poke 0x20000240 1; poke 0x20000244 2 }
FN tidy DEFINED

holy> fn loose() { poke 0x20000248 3 }
FN loose DEFINED
```

Both forms compile identically.

## Task 4.18 — Names: 16 characters, case-sensitive

```text
holy> fn sixteen_chars_max() { poke 0x20000250 1 }
ERR NAME TOO LONG

holy> fn Sixteen_Char() { poke 0x20000250 1 }
FN Sixteen_Char DEFINED
```

`sixteen_chars_max` is 17 — too long. `Sixteen_Char` fits and differs from
any lowercase variant.

## Task 4.19 — peek-at-definition freezes values

**Goal:** Exploit compile-time reads deliberately.

```text
holy> poke 0x20000260 7;
OK

holy> fn snapshot() { poke 0x20000264 peek 0x20000260 }
ERR UNEXPECTED TOKEN
```

Body pokes take literal expressions — a nested `peek` term is not valid in
that position. Bind first:

```text
holy> let v = peek 0x20000260;
v = 0x00000007 (7)

holy> fn use_v() { poke 0x20000264 7 }
FN use_v DEFINED
```

You wrote the frozen value into the source yourself.

## Task 4.20 — Native vs threaded dispatch

**Goal:** Understand which engine runs your code.

`StreamProgram::run()` tries native Thumb-2 compilation first; complex or
unsupported stream shapes fall back to the threaded interpreter. Both
produce identical observable behavior — poke bytes land, stacks balance.
On RISC-V today everything runs threaded (native path gated pending ELF
segment permissions).

## Task 4.21 — Watch the JIT buffer grow

```text
holy> peek 0x20002000;
= 0x???????? (words present after last run)

holy> fn fresh() { poke 0x20000270 1 }
ERR FN TABLE FULL
```

(Slots spent.) Even rejected definitions leave earlier emissions visible:
EXEC_BUFFER is plain readable SRAM at `0x20002000`.

## Task 4.22 — Instruction cache coherence is handled for you

After writing machine code, the kernel executes DSB+ISB (ARM) or `fence.i`
(RISC-V) before entry. You never think about stale pipelines — define,
call, done.

## Task 4.23 — Deterministic timing per call

Every call replays a fixed token stream through fixed primitives: same
instructions, same cycle count, every time. No allocator pauses, no JIT
warm-up, no tiering. Latency variance across calls: zero.

## Task 4.24 — Composition pattern: parameterize by convention

**Goal:** Simulate parameters using agreed memory cells.

```text
holy> let param = 0x20000280;
param = 0x20000280 (536872064)

holy> poke param 99;
OK

holy> fn act() { poke 0x20000284 99 }
FN act DEFINED

holy> act();
OK
```

There are no arguments — but there are addresses. Write the parameter cell
first (top level), then call the reader function.

## Task 4.25 — Reset is the only garbage collector

No `sys_reset` exists: function slots free **only** at reboot, when
`init_data_bss` zeroes `.bss`. Design sessions accordingly: two well-named
functions beat five throwaway ones. When you outgrow the session, power-cycle
— the registry, symbols, and JIT buffer all return to factory state together.

---
*End of Chapter 4 — 100/250*
