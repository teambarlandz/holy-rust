# Cookbook Chapter 1: GPIO Control

*25 recipes for driving STM32F405 GPIO from the REPL. RISC-V users: swap in
the SiFive addresses from Appendix E of the Book.*

**Reference map (STM32F405):**

| Register | Offset | Address |
|----------|--------|---------|
| RCC_AHB1ENR | — | `0x40023830` |
| GPIOA.MODER | +0x00 | `0x40020000` |
| GPIOA.OTYPER | +0x04 | `0x40020004` |
| GPIOA.OSPEEDR | +0x08 | `0x40020008` |
| GPIOA.PUPDR | +0x0C | `0x4002000C` |
| GPIOA.IDR | +0x10 | `0x40020010` |
| GPIOA.ODR | +0x14 | `0x40020014` |
| GPIOA.BSRR | +0x18 | `0x40020018` |

---

## Task 1.01 — Claim the GPIO port

**Goal:** Take exclusive ownership of GPIOA before touching any register.

```text
holy> cap_claim GPIOA;
CAP CLAIMED GPIOA id=0
```

Without this token, every `poke` to `0x4002xxxx` dies with `E001` at parse
time. The claim costs one atomic `fetch_or` — nanoseconds.

## Task 1.02 — Turn on the GPIOA clock

**Goal:** Gate the peripheral's clock on via RCC. An unclocked port reads as
zero and ignores writes.

```text
holy> cap_claim GPIOA;
CAP CLAIMED GPIOA id=0

holy> poke 0x40023830 0x00000001;
OK
```

Bit 0 of AHB1ENR enables GPIOA. Note RCC (`0x40023830`) is unmapped MMIO —
it needs `SUPERUSER`, not `GPIOA`:

```text
holy> cap_drop GPIOA;
CAP RELEASED GPIOA

holy> cap_claim SUPERUSER;
CAP CLAIMED SUPERUSER id=31

holy> poke 0x40023830 0x00000001;
OK

holy> sys_audit;
--- SUPERUSER AUDIT LOG ---
Total Unsafe Operations: 1
Recent Events:
ADDR: 0x40023830 | VAL: 0x00000001 | CYCLES: ...
```

## Task 1.03 — Set PA5 to output mode

**Goal:** MODER5 = `01`.

```text
holy> poke 0x40020000 0x00000400;
OK
```

Bits [11:10] hold MODER5. `0x0400` = bit 10 set = output mode.

## Task 1.04 — Drive PA5 high

**Goal:** Atomic set through BSRR (never read-modify-write an output latch
you share with interrupts).

```text
holy> poke 0x40020018 0x00000020;
OK
```

Writing bit 5 of BSRR's lower half sets PA5 and leaves every other pin
untouched.

## Task 1.05 — Drive PA5 low

**Goal:** Atomic clear through BSRR's upper half.

```text
holy> poke 0x40020018 0x00200000;
OK
```

Bit 21 = "clear pin 5". One write, no RMW race.

## Task 1.06 — Read a pin's input level

**Goal:** Sample PA5 through the input data register.

```text
holy> peek 0x40020010;
= 0x00000000 (0)

holy> let raw = peek 0x40020010;
raw = 0x00000020 (32)
```

`raw & 32` is non-zero iff PA5 is high. The `peek ... ;` statement form prints
the value directly.

## Task 1.07 — Toggle a pin without reading it

**Goal:** Flip PA5 using only writes — safe even if an interrupt shares the
port.

```text
holy> let high = 0x0020;
high = 0x00000020 (32)

holy> let low = 0x00200000;
low = 0x00200000 (2097152)

holy> poke 0x40020018 0x00000020;
OK

holy> poke 0x40020018 0x00200000;
OK
```

Two BSRR writes = one toggle cycle, each instruction atomic.

## Task 1.08 — Choose push-pull or open-drain

**Goal:** OTYPER5 = 0 (push-pull, default) or 1 (open-drain).

```text
holy> reg_clr_bit 0x40020004 5;
OK
```

Open-drain for shared buses:

```text
holy> reg_set_bit 0x40020004 5;
OK
```

`reg_set_bit`/`reg_clr_bit` do the read-modify-write for you in one command.

## Task 1.09 — Attach a pull-up

**Goal:** PUPDR5 = `01` (pull-up) so the pin reads high when floating.

```text
holy> poke 0x4002000C 0x00000400;
OK
```

Pull-down would be bits [11:10] = `10`: `poke 0x4002000C 0x00000800;`

## Task 1.10 — Crank up output slew rate

**Goal:** OSPEEDR5 = `11` (very high speed) for fast edge transitions.

```text
holy> poke 0x40020008 0x00000C00;
OK
```

## Task 1.11 — Configure several pins in one write

**Goal:** PA5, PA6, PA7 all outputs simultaneously.

MODER5..7 occupy bits [15:10]; output mode per pin is binary `01`:

```text
holy> let modes = 0b0101010100;
ERR LEX
```

There is no binary literal syntax — compute it in hex:

```text
holy> let modes = 0x1540;
modes = 0x00001540 (5440)

holy> poke 0x40020000 0x00001540;
OK
```

## Task 1.12 — Set and clear different pins atomically

**Goal:** Set PA5 high while clearing PA6 low, in ONE store.

```text
holy> poke 0x40020018 0x00000020 + 0x00400000;
OK
```

Left-to-right sum: `0x20 | 0x400000` packed into a single BSRR word.
One volatile write, both pins updated, zero interleaving window.

## Task 1.13 — Same recipe, port B

**Goal:** Drive PB3. Everything shifts by `+0x400`.

```text
holy> cap_claim GPIOB;
CAP CLAIMED GPIOB id=1

holy> reg_set_bit 0x40020400 6;
OK

holy> poke 0x40020418 0x00000008;
OK
```

MODER3 lives at bits [7:6], so `reg_set_bit` on bit 6 selects output mode.
BSRR bit 3 drives the pin.

## Task 1.14 — Reset a port to power-on state

**Goal:** Return GPIOA to all-input, no-pull defaults.

```text
holy> poke 0x40020000 0x00000000;
OK

holy> poke 0x40020004 0x00000000;
OK

holy> poke 0x4002000C 0x00000000;
OK
```

MODER resets to 0 (input); OTYPER and PUPDR reset to 0.

## Task 1.15 — Verify configuration by reading it back

**Goal:** Confirm what actually landed in the register — trust nothing.

```text
holy> peek 0x40020000;
= 0x00000400 (1024)
```

`0x400` in MODER confirms exactly one pin in output mode. If you see
unexpected extra bits, some other config wrote here first.

## Task 1.16 — Build register values from named constants

**Goal:** Make scripts self-documenting.

```text
holy> let gpioa = 0x40020000;
gpioa = 0x40020000 (1073872896)

holy> let moder = gpioa + 0;
moder = 0x40020000 (1073872896)

holy> let bsrr = gpioa + 0x18;
bsrr = 0x40020018 (1073872920)

holy> poke bsrr 0x20;
OK
```

Names are constants resolved at parse time — zero runtime cost.

## Task 1.17 — Compute masks instead of memorizing them

**Goal:** Generate the BSRR set-value for pin N arithmetically.

Pin 9 set mask = 2^9:

```text
holy> let n = 9;
n = 0x00000009 (9)

holy> let mask = 1 * 512;
mask = 0x00000200 (512)

holy> poke 0x40020018 0x200;
OK
```

(No shift operator exists — multiply by powers of two: `1 * 2 * 2 * ...`.
For big shifts, write the hex literal.)

## Task 1.18 — Test whether a pin is high, numerically

**Goal:** Reduce IDR to a clean 0/1 answer.

```text
holy> let level = peek 0x40020010;
level = 0x00000020 (32)

holy> level / 32 % 2;
= 0x00000001 (1)
```

Divide by the pin's weight (2^N), mod 2. Works because everything is
unsigned integer math.

## Task 1.19 — Extract a 4-bit nibble from IDR

**Goal:** Read pins PA8–PA11 as a group.

```text
holy> peek 0x40020010 / 256 % 16;
= 0x00000005 (5)
```

Shift right 8 (divide by 2^8), mask low 4 bits (mod 16). Division-and-modulo
is your bitwise toolkit.

## Task 1.20 — Define a reusable led_on / led_off pair

**Goal:** Compile pin control into the JIT buffer once, call it forever.

```text
holy> fn led_on() { poke 0x40020018 0x20 }
FN led_on DEFINED

holy> fn led_off() { poke 0x40020018 0x200000 }
FN led_off DEFINED

holy> led_on();
OK

holy> led_off();
OK
```

Bodies are checked against the capability registry at definition time.

## Task 1.21 — Blink with software delays

**Goal:** On/off sequence long enough to see (in QEMU: to log).

```text
holy> fn strobe() { led_on() led_off() led_on() led_off() }
FN strobe DEFINED

holy> strobe();
OK
```

Function bodies splice their token streams inline — calls are free, not
function-pointer indirection.

## Task 1.22 — Pulse-width control via repetition

**Goal:** Asymmetric waveform: long-high, short-low duty cycle.

```text
holy> fn wide_high() { led_on() led_on() led_off() }
FN wide_high DEFINED

holy> fn narrow_high() { led_on() led_off() }
FN narrow_high DEFINED

holy> wide_high();
OK

holy> narrow_high();
OK
```

Crude but deterministic — each `led_*` call is a fixed number of cycles,
so duty ratio is exact.

## Task 1.23 — Read-modify-write MODER safely

**Goal:** Add PA9 as output without clobbering PA5's existing mode.

```text
holy> let m = peek 0x40020000;
m = 0x00000400 (1024)

holy> poke 0x40020000 0x00040400;
OK
```

You computed `old | (1<<20)` yourself (bit 20 = MODER9 low bit). The REPL
gives you raw RMW building blocks; `reg_set_bit` automates single bits.

## Task 1.24 — One-write pin burst via BSRR packing

**Goal:** Set PA5, PA8, PA12 high; clear PA6, PA7 — single store.

Set mask: bits 5+8+12 = `0x1120`. Clear mask (upper half): bits (6+16)=22,
(7+16)=23 → `0x00C00000`.

```text
holy> let burst = 0x1120 + 0xC00000;
burst = 0x00C01120 (12585088)

holy> poke 0x40020018 0xC01120;
OK
```

This is why BSRR exists: one address, one write, five pins changed.

## Task 1.25 — Release the port when finished

**Goal:** Return GPIOA to the registry so the next session can claim it.

```text
holy> cap_drop GPIOA;
CAP RELEASED GPIOA

holy> poke 0x40020018 0x20;
ERR E001: CAPABILITY_VIOLATION - Peripheral token not claimed
```

The post-drop error proves enforcement is still live. Linear tokens: claim,
use, drop — no leaks, no double-owners.

---
*End of Chapter 1 — 25/250*
