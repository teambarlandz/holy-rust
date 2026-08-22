# Cookbook Chapter 8: UART & Serial I/O

*25 recipes for talking through the wire you're already typing on.
The console UART is a claimed peripheral like any other.*

**USART1 map (ARM, base `0x40011000`):**

| Reg | Offset | Bits that matter |
|-----|--------|------------------|
| SR | +0x00 | TXE=7, TC=6, RXNE=5, ORE=3 |
| DR | +0x04 | data |
| BRR | +0x08 | baud divisor |
| CR1 | +0x0C | UE=13, TE=3, RE=2 |

**SiFive UART0 map (RISC-V, base `0x10013000`):**

| Reg | Offset | Bits |
|-----|--------|------|
| txdata | +0x00 | full=31 |
| rxdata | +0x04 | empty=31 |

---

## Task 8.01 — Claim the console

```text
holy> cap_claim UART0;
CAP CLAIMED UART0 id=2
```

Yes — you can claim the UART you're typing through. The kernel's own
put_byte uses raw access and keeps working regardless.

## Task 8.02 — Read the line status

```text
holy> peek 0x40011000;
= 0x000000C0 (192)
```

`0xC0` = bits 7 and 6 set = TXE (transmit empty) and TC (transmission
complete). Idle and ready.

## Task 8.03 — Decode TXE in one expression

```text
holy> peek 0x40011000 / 128 % 2;
= 0x00000001 (1)
```

Bit 7 → divide by 128, mod 2. One means "safe to write DR".

## Task 8.04 — Transmit one raw byte

**Goal:** Put an actual byte on the wire from the REPL.

```text
holy> poke 0x40011004 65;
A
OK
```

65 = ASCII 'A'. It appears in your terminal because you just transmitted
it through the same port your console uses. The kernel echoes it back
into the session log.

## Task 8.05 — Spell with decimal codes

```text
holy> poke 0x40011004 72;
H
OK

holy> poke 0x40011004 73;
I
OK
```

H=72, I=73. Each poke waits internally? No — *you* must check TXE first
for guaranteed pacing (Task 8.06); single bytes usually race-free.

## Task 8.06 — Polite transmit: check, then send

```text
holy> peek 0x40011000 / 128 % 2;
= 0x00000001 (1)

holy> poke 0x40011004 33;
!
OK
```

Poll-then-poke is the discipline interrupt handlers use too.

## Task 8.07 — CRLF done right

Terminals want `\r\n` (13, 10):

```text
holy> peek 0x40011000 / 128 % 2;
= 0x00000001 (1)

holy> poke 0x40011004 13;

OK

holy> poke 0x40011004 10;
OK
```

The 13 produced a visible blank line — newline sent into the stream.

## Task 8.08 — Receive: check RXNE

```text
holy> peek 0x40011000 / 32 % 2;
= 0x00000000 (0)
```

Bit 5 clear = no incoming byte waiting. Type something in another window
at your peril — the REPL itself consumes input first!

## Task 8.09 — Why RXNE is usually zero: the REPL eats input

The kernel polls the same receive path for your commands. Any byte you
send to the board gets consumed as REPL input unless the REPL is busy.
To feed data programmatically while keeping the console, you need a second
UART — or accept that stdin IS the data channel (which is the point).

## Task 8.10 — Overrun detection and clearing

If bytes arrive faster than they're read, ORE (bit 3) latches:

```text
holy> peek 0x40011000 / 8 % 2;
= 0x00000000 (0)
```

Clear sequence on real silicon: read SR, then read DR. In QEMU the flag
rarely sets — but the recipe stands.

## Task 8.11 — Wait-for-complete (TC)

TC = bit 6 confirms the shift register drained:

```text
holy> peek 0x40011000 / 64 % 2;
= 0x00000001 (1)
```

Critical before entering low-power modes on real hardware.

## Task 8.12 — Mute the receiver

```text
holy> reg_clr_bit 0x4001100C 2;
OK
```

CR1.RE off: incoming bytes ignored. Restore:

```text
holy> reg_set_bit 0x4001100C 2;
OK
```

## Task 8.13 — THE DANGER ZONE: touching UE/TE

Turning off UE (bit 13) or TE (bit 3) kills your only console:

```text
holy> reg_clr_bit 0x4001100C 3;
```

(silence — no more output, prompt dead)

**Recovery:** power-cycle the board/QEMU. The kernel re-runs uart::init()
at boot which re-sets UE|TE|RE. Lesson: treat `0x4001100C` as radioactive.

## Task 8.14 — Baud rate: look, don't touch

```text
holy> peek 0x40011008;
= 0x00000xxx (divisor)
```

QEMU ignores BRR mostly; real silicon doesn't. Changing it mid-session
garbles everything downstream. Inspect freely; never poke without a
recovery plan identical to Task 8.13.

## Task 8.15 — RISC-V: transmit one byte

```text
holy> cap_claim UART0;
CAP CLAIMED UART0 id=2

holy> poke 0x10013000 65;
A
OK
```

txdata at offset 0. SiFive UARTs pace writes via the full flag:

## Task 8.16 — RISC-V: honor the full flag

```text
holy> peek 0x10013000 / 2147483648 % 2;
ERR DIV BY ZERO
```

(2^31 exceeds u32 when divided — divide differently!)

```text
holy> let full_bit = peek 0x10013000 / 2 % ... 
```

Cleaner: bind then test high bit via division ladder, or exploit that
empty/full is just bit 31 — compare the whole word:

```text
holy> let t = peek 0x10013000;
t = 0x00000000 (0)

holy> t / 2147483648;
= 0x00000000 (0)
```

Zero = not full = safe to transmit.

## Task 8.17 — RISC-V: receive side

```text
holy> let r = peek 0x10013004;
r = 0x80000000 (2147483648)

holy> r / 2147483648;
= 0x00000001 (1)
```

Bit 31 set = rx queue empty; low 8 bits are data when clear.

## Task 8.18 — The kernel's ring buffer (concept)

RX IRQ handlers push bytes into a 256-byte SPSC ring (`uart::irq_handler`
→ `ring_pop`). Your REPL drains it every poll cycle. You cannot peek this
buffer directly (it lives in `.bss`, address unknown by design) — its
existence explains burst tolerance: up to 256 bytes buffered during long
evaluations.

## Task 8.19 — Burst-tolerance demonstration

Paste 20 lines of commands at once: all execute sequentially, none lost,
because the ring absorbed them while line N parsed. This is why pasted
sessions work reliably.

## Task 8.20 — Binary-safe transmission

Bytes above 127 are fine — DR takes all 9 bits if configured; 8 here:

```text
holy> poke 0x40011004 255;
ÿ
OK
```

Your terminal renders 0xFF per its own encoding; the wire carried exactly
eight ones.

## Task 8.21 — Signaling protocol over the console

**Goal:** Frame machine-readable markers around human output.

Transmit SOH (1), payload, EOT (4):

```text
holy> poke 0x40011004 1;
OK

holy> poke 0x40011004 85;
U
OK

holy> poke 0x40011004 4;
OK
```

A host script can split on \x01/\x04 — the REPL becomes a controlled
telemetry source.

## Task 8.22 — Measure wire time with CYCCNT

```text
holy> cap_claim SUPERUSER;
CAP CLAIMED SUPERUSER id=31

holy> poke 0x50000030 65;
A
OK

holy> poke 0x50000034 66;
B
OK

holy> sys_audit
...
ADDR: 0x50000030 | VAL: 0x00000041 | CYCLES: T0
ADDR: 0x50000034 | VAL: 0x00000042 | CYCLES: T1
```

T1-T0 ≈ one byte time at current baud (115200 ≈ 84 MHz/… ≈ ~729 cycles)
plus overheads — sanity-check your link speed arithmetically.

## Task 8.23 — Two-claim discipline for mixed traffic

When a recipe touches both GPIO signaling and UART handshaking:

```text
holy> cap_claim GPIOA;
CAP CLAIMED GPIOA id=0

holy> cap_claim UART0;
CAP CLAIMED UART0 id=2

holy> peek 0x20001000;
= 0x00000005 (5)
```

Registry word proves both held before the critical section begins.

## Task 8.24 — Silent-mode trick: drop UART0 mid-session

Dropping UART0 does NOT silence the kernel (raw driver path bypasses the
registry) — but it DOES block *your* pokes to UART registers:

```text
holy> cap_drop UART0;
CAP RELEASED UART0

holy> poke 0x40011004 88;
ERR E001: CAPABILITY_VIOLATION - Peripheral token not claimed
```

Console survives; direct wire access revoked. Capability semantics vs
kernel needs, cleanly separated.

## Task 8.25 — Full session: serial beacon

**Goal:** Emit a repeating marker pattern between normal commands.

```text
holy> cap_claim UART0;
CAP CLAIMED UART0 id=2

holy> fn beep() { poke 0x40011004 46 }
FN beep DEFINED

holy> beep();
.
OK

holy> beep();
.
OK

holy> cap_drop UART0;
CAP RELEASED UART0
```

Each call emits one period. A host watching the log sees heartbeats
interleaved with your work — presence proof with zero extra hardware.

---
*End of Chapter 8 — 200/250*
