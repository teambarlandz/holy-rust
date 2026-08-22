# Cookbook Chapter 6: The REPL, System Commands & Telemetry

*25 recipes for driving the console itself: session control, line editing,
error recovery, and the three built-in inspection commands.*

**Built-in commands (complete list):** `help`, `banner`, `sys_audit` —
semicolon optional on all three. Everything else is `peek`, `poke`,
`reg_set_bit`, `reg_clr_bit`, `cap_claim`, `cap_drop`, `let`, `fn`,
or a bare expression.

---

## Task 6.01 — Print the command reference

```text
holy> help
commands:
peek ADDR;              read u32 from address (requires capability)
poke ADDR VAL;          write u32 to address (requires capability)
reg_set_bit ADDR BIT;   set register bit (requires capability)
reg_clr_bit ADDR BIT;   clear register bit (requires capability)
cap_claim NAME;         claim peripheral (GPIOA GPIOB UART0 SPI0 I2C0 TIMER0 DMA0 SUPERUSER)
cap_drop NAME;          release peripheral
let NAME = EXPR;        bind constant
fn NAME() { ... }       define callable body
EXPR;                   evaluate (+ - * / % left-to-right)
sys_audit               dump SuperUser audit log
banner                  reprint banner
```

Semicolon optional here:

```text
holy> help;
commands:
...
```

## Task 6.02 — Reprint the banner

```text
holy> banner
Holy Rust REPL v0.1
```

Use it to confirm the console is alive after long silences or baud-rate
changes. Costs nothing, touches nothing.

## Task 6.03 — Dump the audit log (empty state)

```text
holy> sys_audit
--- SUPERUSER AUDIT LOG ---
Total Unsafe Operations: 0
Recent Events:
```

Zero events means nobody has bypassed checks this boot.

## Task 6.04 — Use the audit counter as an event odometer

After N SuperUser writes:

```text
holy> sys_audit
--- SUPERUSER AUDIT LOG ---
Total Unsafe Operations: 7
Recent Events:
ADDR: ... | VAL: ... | CYCLES: ...
```

The total never decreases and survives ring wraparound — it is your
lifetime bypass counter.

## Task 6.05 — Read cycle timestamps forensically

Each entry's `CYCLES` field comes from DWT->CYCCNT (ARM) or `mcycle`
(RISC-V). Diff two entries' cycle counts to measure elapsed machine time
between two bypass writes:

```text
ADDR: 0x50000000 | VAL: 0x00000001 | CYCLES: 104231
ADDR: 0x50000004 | VAL: 0x00000002 | CYCLES: 104395
```

164 cycles between events — subtract by hand, plan accordingly.

## Task 6.06 — Cancel a half-typed line with Ctrl-C

```text
holy> poke 0x20000^C
holy>
```

Buffer cleared, fresh prompt, nothing evaluated.

## Task 6.07 — Kill the whole line with Ctrl-U

```text
holy> peek 0xDEAD_BEEF^U
holy>
```

Same effect as Ctrl-C but keeps you in Reading state for immediate retype.

## Task 6.08 — Fix typos with Backspace

```text
holy> pokk<BS><BS>e 0x20000100 5;
OK
```

Backspace echoes `\x08 \x08` — erases visually on any terminal.

## Task 6.09 — Submit an empty line safely

```text
holy>

holy>
```

Blank input compiles to nothing, prints nothing. Harmless heartbeat.

## Task 6.10 — Recover from any error without state loss

```text
holy> poke 0x40011000 1;
ERR E001: CAPABILITY_VIOLATION - Peripheral token not claimed

holy> let x = 42;
x = 0x0000002A (42)
```

Errors abort one line only. Symbols, capabilities, functions: all intact.
There is no "error mode" to escape.

## Task 6.11 — Missing semicolon diagnosis

```text
holy> poke 0x20000100 5
ERR MISSING SEMICOLON
```

Every statement except help/banner/sys_audit demands the terminator.

## Task 6.12 — Unknown word becomes an expression attempt

```text
holy> frobnicate;
ERR UNKNOWN SYMBOL
```

The parser tried to evaluate `frobnicate` as a variable reference. Typos
never execute anything.

## Task 6.13 — Decode every value print format

```text
= 0x00006E4 (1764)
```

Format: `0x` + 8 uppercase hex digits + decimal in parens. Hex for
registers, decimal for magnitudes — both always present.

## Task 6.14 — Recognize the two peripheral errors instantly

| Message | Meaning | Fix |
|---------|---------|-----|
| `E001: CAPABILITY_VIOLATION` | address belongs to a *mapped, unclaimed* peripheral | `cap_claim` its token |
| `E002: PERMISSION_DENIED` | address is *unmapped* MMIO with no SuperUser | `cap_claim SUPERUSER` |

E001 = known hardware, no ticket. E002 = terra incognita.

## Task 6.15 — Whitespace is forgiving

```text
holy>   poke   0x20000100    7   ;
OK

holy> poke 0x20000104 8;
OK
```

Spaces and tabs collapse anywhere. Commas exist in the lexer but serve no
grammar purpose — don't use them.

## Task 6.16 — Commands are lowercase, resources are UPPERCASE

```text
holy> PEEK 0x20000100;
ERR UNEXPECTED TOKEN

holy> cap_claim gpioa;
ERR UNKNOWN RESOURCE gpioa
```

By design: keywords lowercase (`peek poke let fn cap_claim cap_drop`),
token names uppercase (`GPIOA UART0 SUPERUSER`).

## Task 6.17 — One statement per line

```text
holy> let a = 1; let b = 2;
ERR UNEXPECTED TOKEN
```

No compound statements. The single-pass parser consumes exactly one
statement per submitted line.

## Task 6.18 — The 128-byte line budget

Input beyond 128 bytes is silently dropped at the driver level:

```text
holy> let v = 1111111111111111...   ; (bytes past 128 vanish)
```

Long hex constants fit fine (`0x` + 10 digits); long fn bodies are the risk
— keep definitions under ~120 characters including `fn name() { }`.

## Task 6.19 — Cold-boot verification ritual

**Goal:** Certify a fresh session in four lines.

```text
holy> banner
Holy Rust REPL v0.1

holy> 2 + 2;
= 0x00000004 (4)

holy> peek 0x20001000;
= 0x00000000 (0)

holy> sys_audit
--- SUPERUSER AUDIT LOG ---
Total Unsafe Operations: 0
Recent Events:
```

Banner alive, arithmetic sane, registry clean, audit empty. Green board.

## Task 6.20 — Session hygiene checklist

Before walking away:

```text
holy> peek 0x20001000;
= 0x00000020 (32)          ← something still claimed!

holy> cap_drop TIMER0;
CAP RELEASED TIMER0
```

Check the registry word; drop stragglers. Next session inherits zero locks.

## Task 6.21 — Echo confirms what the machine heard

Every printable byte echoes as typed. If your paste mangled a character,
you'll see it before pressing Enter — the terminal shows the true buffer.

## Task 6.22 — Non-printable bytes are ignored

Stray control bytes (tab-completion escapes, bracketed-paste markers) fall
through the feed loop silently. Only CR/LF submit, BS/DEL erase, ^C/^U act.

## Task 6.23 — CRLF, LF, both fine

```text
holy> peek 0x20000100;\r\n
= ...
```

`\r`, `\n`, or the pair all terminate the line identically. Terminal
emulators need no special configuration.

## Task 6.24 — Measure round-trip latency by eye

Type `banner` and watch response time: UART echo + parse + write at
115200-style serial. Consistent sub-100ms feel means the pipeline is
healthy. Variance here would mean variance everywhere — REPL timing IS
kernel timing.

## Task 6.25 — The complete tour in eleven lines

```text
holy> banner
holy> 1 + 1;
holy> let port = 0x40020018;
holy> cap_claim SUPERUSER;
holy> poke 0x50000000 0xAA;
holy> cap_drop SUPERUSER;
holy> sys_audit
holy> cap_claim GPIOA;
holy> poke 0x40020018 0x20;
holy> peek 0x40020018;
holy> cap_drop GPIOA;
```

Banner, math, binding, audited bypass, token lifecycle, enforced MMIO.
Eleven lines exercise every subsystem in the kernel.

---
*End of Chapter 6 — 150/250*
