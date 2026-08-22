# Cookbook Chapter 7: Timers, Delays & Real-Time Patterns

*25 recipes for time-aware programming without an OS. No `sleep`, no
scheduler — just cycle counters, hardware timers, and deterministic code.*

**Time sources available to `peek`:**

| Source | Address | Notes |
|--------|---------|-------|
| DWT_CYCCNT | `0xE0001004` | ARM cycle counter, free-running |
| SysTick current | `0xE000E018` | 24-bit downcounter |
| TIM2_CNT | `0x40000024` | needs TIMER0 claim |

---

## Task 7.01 — Enable the ARM cycle counter

**Goal:** Turn on DWT_CYCCNT so it actually counts.

```text
holy> cap_claim SUPERUSER;
CAP CLAIMED SUPERUSER id=31

holy> reg_set_bit 0xE000EDFC 24;
OK

holy> reg_set_bit 0xE0001000 0;
OK

holy> cap_drop SUPERUSER;
CAP RELEASED SUPERUSER
```

DEMCR.TRCENA, then DWT_CTRL.CYCCNTENA. Unmapped debug addresses need
SuperUser; both writes are audited automatically.

## Task 7.02 — Read the running count

```text
holy> peek 0xE0001004;
= 0x0012A5F0 (1223664)

holy> peek 0xE0001004;
= 0x0012B3C1 (1227713)
```

Two reads, different values — proof of life at 84 MHz.

## Task 7.03 — Measure elapsed cycles

**Goal:** Bind two snapshots, subtract mentally (or on paper).

```text
holy> let t0 = peek 0xE0001004;
t0 = 0x0012B3C1 (1227713)

holy> let t1 = peek 0xE0001004;
t1 = 0x0012BC9F (1229983)

holy> t1 - t0;
= 0x000008DE (2270)
```

2270 cycles ≈ 27 µs at 84 MHz — that's your typing latency, not the
kernel's. Intra-line costs are far smaller (Task 7.05).

## Task 7.04 — Timestamp an event

```text
holy> poke 0x20000200 0xAA;
OK

holy> let evt_time = peek 0xE0001004;
evt_time = 0x0012D400 (1234944)
```

The name freezes the exact cycle your store landed nearby — good to within
a few instructions.

## Task 7.05 — Measure one statement's cost precisely

**Goal:** Delta between adjacent lines measures parse+execute+UART of the
*middle* line plus overheads; for tight numbers, compare two empty-ish
lines vs a poke line and subtract.

```text
holy> let a = peek 0xE0001004;
a = ...

holy> poke 0x20000204 0x55;
OK

holy> let b = peek 0xE0001004;
b = ...

holy> b - a;
= 0x00000xxx (...)
```

Run it twice; the difference between trials is your noise floor.

## Task 6→7.06 — Cross-check with the audit log

SuperUser writes carry free timestamps:

```text
holy> cap_claim SUPERUSER;
CAP CLAIMED SUPERUSER id=31

holy> poke 0x50000020 1;
OK

holy> poke 0x50000024 2;
OK

holy> sys_audit
--- SUPERUSER AUDIT LOG ---
Total Unsafe Operations: 2
Recent Events:
ADDR: 0x50000020 | VAL: 0x00000001 | CYCLES: ...
ADDR: 0x50000024 | VAL: 0x00000002 | CYCLES: ...
```

Subtract the two CYCLES fields: instrumented delay with zero extra code.

## Task 7.07 — SysTick: the free-running downcounter

```text
holy> peek 0xE000E018;
= 0x0000F3A2 (62370)

holy> peek 0xE000E018;
= 0x0000F1B7 (61879)
```

Counts DOWN toward zero from SYST_RVR. If it reads 0 forever, the tick is
disabled — configure STCSR (`0xE000E010`) first if you need it.

## Task 7.08 — Claim the hardware timer

```text
holy> cap_claim TIMER0;
CAP CLAIMED TIMER0 id=5

holy> peek 0x40000024;
= 0x00000000 (0)
```

TIM2's counter register readable immediately after claim.

## Task 7.09 — Start TIM2 counting

**Goal:** Set prescaler=0, auto-reload=max, enable counter.

Clock gate first (APB1ENR needs SuperUser):

```text
holy> cap_claim SUPERUSER;
CAP CLAIMED SUPERUSER id=31

holy> reg_set_bit 0x40023840 0;
OK

holy> cap_drop SUPERUSER;
CAP RELEASED SUPERUSER

holy> poke 0x4000002C 0xFFFFFFFF;
OK

holy> reg_set_bit 0x40000000 0;
OK
```

CR1.CEN (bit 0) starts the counter.

## Task 7.10 — Watch TIM2 run

```text
holy> peek 0x40000024;
= 0x00001A2B (6699)

holy> peek 0x40000024;
= 0x00003F41 (16193)
```

Live hardware timebase independent of the CPU pipeline.

## Task 7.11 — Reset a timer mid-flight

```text
holy> reg_clr_bit 0x40000000 0;
OK

holy> poke 0x40000024 0;
OK

holy> reg_set_bit 0x40000000 0;
OK
```

Stop, zero, restart. Three commands, deterministic.

## Task 7.12 — Prescaler arithmetic

**Goal:** Make the counter tick at a known rate.

Timer clock 84 MHz; want ~1 µs ticks: PSC = 83.

```text
holy> poke 0x40000028 83;
OK
```

CNT now increments every 84 clocks. Formula: tick_Hz = f_clk / (PSC + 1).

## Task 7.13 — Build unrolled software delays

**Goal:** Burn fixed cycles without a loop keyword — splice reads.

```text
holy> fn pause() { peek 0x20000100 peek 0x20000104 peek 0x20000108 }
FN pause DEFINED

holy> pause();
OK
```

Three volatile loads ≈ fixed instruction budget per call. Chain calls for
longer waits:

```text
holy> fn wait() { pause() pause() pause() }
FN wait DEFINED
```

Word budget check: each peek ≈ 2 stream words; 32-word body limit allows
~14 peeks per definition.

## Task 7.14 — Calibrate an unrolled delay

```text
holy> let c0 = peek 0xE0001004;
c0 = ...

holy> pause();
OK

holy> let c1 = peek 0xE0001004;
c1 = ...

holy> c1 - c0;
= 0x000000xx (measured)
```

Now you know exactly how long `pause()` lasts — forever, since code never
recompiles differently.

## Task 7.15 — Fixed-ratio duty cycling

**Goal:** LED waveform with exact on/off proportion.

```text
holy> fn led_on() { poke 0x40020018 32 }
FN led_on DEFINED

holy> fn led_off() { poke 0x40020018 2097152 }
FN led_off DEFINED

holy> fn duty() { led_on() led_on() led_on() led_off() }
FN duty DEFINED
```

75% duty by construction — three on-splices per off-splice, all identical
lengths.

## Task 7.16 — Poll a flag until ready

**Goal:** Wait for UART TX-ready style status bits.

```text
holy> cap_claim UART0;
CAP CLAIMED UART0 id=2

holy> peek 0x40011000 / 128 % 2;
= 0x00000001 (1)
```

TXE is bit 7: divide by 2^7, mod 2. Repeat the line until it prints 1 —
manual polling, full visibility.

## Task 7.17 — Timeout guard with cycle budget

**Goal:** Detect "never becomes ready" instead of hanging forever.

```text
holy> let start = peek 0xE0001004;
start = ...

holy> peek 0x40011000 / 128 % 2;
= 0x00000001 (1)

holy> peek 0xE0001004 - 0;
= 0x...

holy> (peek 0xE0001004) - 0;
ERR UNEXPECTED TOKEN
```

(peek-terms can't follow operators directly — bind instead):

```text
holy> let now = peek 0xE0001004;
now = ...

holy> now - start;
= 0x0000xxxx (elapsed)
```

Elapsed over budget? Bail out. Your timeout logic is manual but explicit.

## Task 7.18 — Schedule actions on counter thresholds

**Goal:** Act when TIM2 crosses a value.

```text
holy> peek 0x40000024;
= 0x000186A0 (99999)

holy> peek 0x40000024 / 100000 % 2;
= 0x00000000 (0)
```

Zero means "still under 100k". When it flips to 1, your window opened.
Poll by re-running the line — REPL-as-scheduler.

## Task 7.19 — Measure jitter across repetitions

**Goal:** Prove determinism empirically.

```text
holy> let j0 = peek 0xE0001004;
j0 = ...

holy> duty();
OK

holy> let d1 = peek 0xE0001004;
d1 = ...

holy> d1 - j0;
= 0x00000xxx (run 1)

holy> duty();
OK
...
```

Repeat five times; identical deltas mean zero jitter. This is the manifesto's
promise made visible.

## Task 7.20 — Sequence with inter-step deadlines

**Goal:** Space three stores exactly N cycles apart using TIM2.

```text
holy> peek 0x40000024 / 1000 % 2;
= 0x00000001 (1)

holy> poke 0x20000300 1;
OK

holy> peek 0x40000024 / 1000 % 2;
= 0x00000001 (1)

holy> poke 0x20000304 2;
OK
```

Each line waits (by you re-polling) until the next millisecond tick fires,
then acts. Deadlines enforced by hardware, decisions by you.

## Task 7.21 — What happens when you blow a deadline

Nothing automatic. Ring 0 has no preemption, no watchdog bite, no penalty —
just wall-clock truth recorded in CYCCNT. Post-mortem:

```text
holy> sys_audit
```

Compare CYCLES fields against your plan. The system records; you adjudicate.

## Task 7.22 — RISC-V timing without CSRs

On sifive_e, `mcycle` is CSR-only — unreachable by `peek`. Options:
audit-log CYCLES fields still work (the kernel reads mcycle internally),
or use the AON/CLINT memory-mapped counters where your SoC exposes them.
Same recipes, different address.

## Task 7.23 — Free-run wraparound handling

CYCCNT wraps every 2^32 / 84e6 ≈ 51 seconds. Guard deltas:

```text
holy> let d = 100;
d = 0x00000064 (100)

holy> 0 - 100 + 50;
= 0xFFFFFFB6 (4294967222)
```

If `t1 < t0`, true delta = `(0xFFFFFFFF - t0) + t1 + 1`. Compute it once,
note it, move on.

## Task 7.24 — Benchmark suite in six lines

```text
holy> let a = peek 0xE0001004;
holy> 1000 + 23;
holy> let b = peek 0xE0001004;
holy> b - a;
holy> let c = peek 0xE0001004;
holy> c - b;
```

First delta includes evaluation of a printed expression; second is pure
overhead. The gap is your REPL tax — usually tens of cycles.

## Task 7.25 — Real-time design rules recap

1. All costs are static — measure once, know forever.
2. No allocator, no GC pause, no priority inversion exists to fear.
3. Delays are unrolled splices: bounded by the 32-word body budget.
4. Hardware timers give sub-microsecond scheduling edges.
5. The audit log timestamps every risky write for free.

Determinism isn't a feature here; it's the absence of machinery that could
make things vary.

---
*End of Chapter 7 — 175/250*
