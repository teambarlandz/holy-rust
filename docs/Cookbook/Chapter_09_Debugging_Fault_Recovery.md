# Cookbook Chapter 9: Debugging & Fault Recovery

*25 recipes for when things go wrong — and for proving they can't.
Ring 0 fails loudly or not at all; this chapter teaches you to listen.*

---

## Task 9.01 — Meet the fault banner

```text
holy> peek 0x60000000;

**FAULT: core exception, halted**
```

An unmapped read tripped the bus. The handler announced it over UART and
parked the core with `wfi`. Session over — this is the one unrecoverable
state, and it is *always* your code's fault.

## Task 9.02 — Know exactly which accesses can fault

| Access | Outcome |
|--------|---------|
| SRAM `0x2000xxxx` | always safe |
| Flash `0x080xxxxx` | always safe |
| Mapped peripheral, claimed | safe |
| Mapped peripheral, unclaimed | **E001 at parse time** — never executes |
| Unmapped `0x6xxxxxxx`, `0x9xxxxxxx`... | **hard fault** |

Only the last row bites. The capability system already ate the dangerous
middle case for you.

## Task 9.03 — The pre-flight map check

Before touching any address, verify its class:

```text
holy> let probe = 0x50000000;
probe = 0x50000000 (1342177280)

holy> cap_claim SUPERUSER;
CAP CLAIMED SUPERUSER id=31

holy> poke probe 1;
OK

holy> sys_audit
--- SUPERUSER AUDIT LOG ---
Total Unsafe Operations: 1
Recent Events:
ADDR: 0x50000000 | VAL: 0x00000001 | CYCLES: ...
```

If the address were unmapped AND the SoC had no decoder there, this pokes
would have faulted instead of printing OK — SuperUser removes parse-time
protection, so unmapped probes still die. Know your SoC's decoder map.

## Task 9.04 — Deliberate error-path testing

**Goal:** Verify enforcement works without risking a fault.

```text
holy> cap_drop SUPERUSER;
CAP RELEASED SUPERUSER

holy> poke 0x40013000 5;
ERR E001: CAPABILITY_VIOLATION - Peripheral token not claimed

holy> banner
Holy Rust REPL v0.1
```

Rejected at parse, kernel alive, prompt responsive. Errors that never
execute cannot corrupt anything.

## Task 9.05 — Distinguish the three failure voices

| Output | Source | Recoverable? |
|--------|--------|--------------|
| `ERR <REASON>` | parser rejected line | yes, instantly |
| `PANIC: <msg>` | Rust panic handler | no — wfi park |
| `**FAULT: core exception, halted**` | CPU exception | no — wfi park |

Learn these sounds; each implies a different next action.

## Task 9.06 — Trigger every ERR on purpose

A certification pass of all rejections:

```text
holy> 5 / 0;
ERR DIV BY ZERO

holy> let n17chars = 123;
n17chars = 0x0000007B (123)

holy> fn f() { }
ERR FN REDEFINED

holy> frob();
ERR UNKNOWN SYMBOL

holy> 3 & 2;
ERR UNSUPPORTED OPERATOR

holy> poke 0x20000100 1
ERR MISSING SEMICOLON
```

All caught, nothing executed, REPL intact.

## Task 9.07 — Post-error integrity check

```text
holy> peek 0x20001000;
= 0x00000000 (0)

holy> 100 + 23;
= 0x0000007B (123)
```

Registry unchanged, arithmetic sane. After any ERR, run both lines —
ten-second full-system health check.

## Task 9.08 — Use sys_audit as a flight recorder

After any suspicious sequence:

```text
holy> sys_audit
--- SUPERUSER AUDIT LOG ---
Total Unsafe Operations: 3
Recent Events:
ADDR: 0x40023830 | VAL: 0x00000001 | CYCLES: 88121
ADDR: 0x50000020 | VAL: 0x000000AA | CYCLES: 90344
ADDR: 0x40013000 | VAL: 0x00000055 | CYCLES: 95110
```

Every bypass write, in order, timestamped. Reconstruct the incident from
the ring even if you weren't watching live.

## Task 9.09 — Registry forensics

"Who holds what?" is one peek:

```text
holy> peek 0x20001000;
= 0x80000005 (2147483653)
```

Decode `0x80000005`: bits 0 (GPIOA), 2 (UART0), 31 (SUPERUSER). Someone —
probably you, three recipes ago — left god mode on.

## Task 9.10 — JIT buffer sanity inspection

```text
holy> peek 0x20002000;
= 0x0000B510 (...)

holy> peek 0x20002004;
= ...
```

Non-zero halfwords are real emitted instructions (B510 = PUSH {R4,LR}).
All-zero means nothing compiled yet. Garbage patterns here mean something
wrote over EXEC_BUFFER — check for stray pokes into `0x20002xxx` in the
audit log.

## Task 9.11 — Symbol exhaustion debugging

```text
holy> fn a() { poke 0x20000100 1 }
ERR FN TABLE FULL
```

Two slots spent earlier in the session. No free command exists — options:
(1) keep using existing functions via splicing patterns, (2) reboot.
Diagnose first: try calling your old functions; if they work, slots were
the issue, not corruption.

## Task 9.12 — Stream-budget failures

```text
holy> fn huge() { poke 0x20000100 1 poke ... (16+ stores) }
ERR STREAM FULL
```

Body exceeded 32 words (~10 stores max). Split across two definitions and
call them in sequence at top level.

## Task 9.13 — Lex errors mean non-language bytes

```text
holy> let x = "hello";
ERR LEX

holy> let y = 0xZZ;
ERR LEX
```

No strings, no bad digits. If pasting from docs, watch for smart quotes
and en-dashes — they're not in the ASCII alphabet.

## Task 9.14 — QEMU instruction tracing

Run QEMU with `-d in_asm -D trace.log`; every executed instruction lands
in the log with PC values. Grep it after a fault:

```bash
qemu-system-arm -M netduinoplus2 -nographic -kernel target/.../holy-rust \
    -d in_asm -D /tmp/trace.log
tail -40 /tmp/trace.log
```

The last instructions before the halt ARE your faulting access.

## Task 9.15 — GDB attachment workflow

```bash
qemu-system-arm -M netduinoplus2 -nographic \
    -kernel target/thumbv7em-none-eabihf/release/holy-rust \
    -S -gdb tcp::1234 &
arm-none-eabi-gdb target/thumbv7em-none-eabihf/release/holy-rust \
    -ex 'target remote :1234' -ex 'b fault_hang' -ex 'c'
```

Breakpoint on the fault handler catches every crash with full register
view: PC, LR, SP, xPSR — the dump the UART banner doesn't give you.

## Task 9.16 — Decode a real fault on hardware

When `**FAULT**` fires on physical silicon, attach and read:

- `PC` — exact faulting instruction (often inside EXEC_BUFFER → your fn)
- `CFSR` (`0xE000ED28`) — IACCVIOL/MSTKERR/PRECISERR bits classify it
- `BFAR` (`0xE000ED38`) — the offending address for bus faults

Map BFAR against the memory tables in the Book's Appendix B/C.

## Task 9.17 — Panic vs fault: reading PANIC output

```text
PANIC: explicit panic call
```

Panics come from Rust-level invariant breaks (none exist in normal REPL
operation — the hot paths avoid panicking entirely). If you ever see one,
it's a kernel bug: file it with the exact input line.

## Task 9.18 — The reset-and-resume doctrine

After any fatal stop:

```bash
# QEMU: just rerun
# Hardware: NRST button or debugger reset
```

Boot zeroes `.bss`, registry, symbols, EXEC_BUFFER. You lose session state
by design — nothing stale survives to haunt the next run. Re-do:
banner → math check → registry peek → audit (Task 9.07).

## Task 9.19 — Preserve evidence before resetting

On hardware with a debugger attached, dumps beat resets:

```
(gdb) x/8wx 0x20001000     # registry
(gdb) x/16wx 0xE000ED28    # CFSR region
(gdb) info registers
```

Then reset. Evidence first, hygiene second.

## Task 9.20 — Guarded experimentation pattern

**Goal:** Try a risky address class safely.

1. `cap_claim SUPERUSER` (audit trail armed)
2. Attempt ONE write
3. `sys_audit` immediately
4. `cap_drop SUPERUSER`
5. Only then generalize the pattern

One probe per audit review cycle — never batch blind writes.

## Task 9.21 — Watchdog honesty

No watchdog runs. A hang (impossible from parsed code — every statement
terminates) could only come from hardware-level spin. The `wfi` parks are
deliberate states, not hangs. If you need watchdog semantics, drive the
SoC's own IWDG through SuperUser pokes — same cookbook patterns as TIM2.

## Task 9.22 — Ctrl-C is your emergency brake

Mid-line regret:

```text
holy> poke 0x4001100C 0^C
holy>
```

The radioactive CR1 write from Task 8.13, aborted BEFORE Enter. Ctrl-C
works during Reading only — once submitted, the line runs to completion.
There is no interrupt-key preemption of executing statements (single-
threaded kernel, deterministic execution).

## Task 9.23 — Think before Enter: the mental checklist

Dangerous lines share signatures: unmapped bases (`0x5`, `0x6`, `0x9`,
`0xA`, `0xC`, `0xE0..` beyond known debug regs), CR1-style control regs,
BRR. Before pressing Enter on any such line: Is it audited? Do I know the
decoder map? What's my recovery? Three questions, five seconds, zero
faults.

## Task 9.24 — Build a session transcript habit

Because everything is text, debugging IS diffing:

```text
holy> peek 0x40020000;
= 0x00000400 (1024)
...
holy> peek 0x40020000;
= 0x00000450 (1104)
```

Two snapshots differ by `0x50` — unexpected MODER bits appeared between
reads. Scrollback is your logic analyzer; copy-paste transcripts into
notes for post-mortems.

## Task 9.25 — The complete incident-response drill

Full rehearsal, start to finish:

```text
holy> cap_claim SUPERUSER;
CAP CLAIMED SUPERUSER id=31

holy> poke 0x50000000 0xCAFE;
OK

holy> sys_audit
--- SUPERUSER AUDIT LOG ---
Total Unsafe Operations: 1
Recent Events:
ADDR: 0x50000000 | VAL: 0x0000CAFE | CYCLES: ...

holy> cap_drop SUPERUSER;
CAP RELEASED SUPERUSER

holy> peek 0x20001000;
= 0x00000000 (0)
```

Risky write made, recorded, reviewed, cleaned up. That loop — arm, act,
inspect, disarm — is the whole discipline.

---
*End of Chapter 9 — 225/250*
