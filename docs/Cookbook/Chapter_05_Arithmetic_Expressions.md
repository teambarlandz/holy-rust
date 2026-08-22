# Cookbook Chapter 5: Arithmetic & Expressions

*25 recipes for the calculator hiding inside your microcontroller.
Everything is u32. Everything is left-to-right. Everything wraps.*

---

## Task 5.01 — Plain addition

```text
holy> 2 + 2;
= 0x00000004 (4)
```

## Task 5.02 — Hex and decimal mix freely

```text
holy> 0x10 + 16;
= 0x00000020 (32)
```

Both lex to the same `Literal(u32)` token.

## Task 5.03 — Subtraction that would go negative

```text
holy> 1 - 2;
= 0xFFFFFFFF (4294967295)
```

Wraps to all-ones. There is no sign bit; 0xFFFFFFFF *is* -1 in disguise.
Plan for it: compare against expected constants rather than "negative".

## Task 5.04 — Multiplication overflow wraps silently

```text
holy> 65536 * 65536;
= 0x00000000 (0)

holy> 100000 * 100000;
= 0x2540BE400 → low 32 bits = 0x540BE400 (1410065408)
```

`wrapping_mul`: high bits vanish. For address math this is exactly what you
want; for magnitudes, stay small or precompute hex literals.

## Task 5.05 — Integer division truncates

```text
holy> 7 / 2;
= 0x00000003 (3)

holy> 1 / 1000;
= 0x00000000 (0)
```

## Task 5.06 — Modulo gives the remainder

```text
holy> 7 % 2;
= 0x00000001 (1)

holy> 255 % 16;
= 0x0000000F (15)
```

`% 256`, `% 65536` extract bytes/halfwords — your bitwise toolkit.

## Task 5.07 — THE precedence trap

```text
holy> 2 + 3 * 4;
= 0x00000014 (20)
```

Twenty, not fourteen. Evaluation is strictly left-to-right:
`(2+3)*4`. There are no precedence levels anywhere in the grammar.
When porting expressions from C, re-associate them yourself.

## Task 5.08 — Parentheses change nothing

```text
holy> (2 + 3) * 4;
= 0x00000014 (20)
```

Parens parse but never alter order. Write them for humans if you like;
the machine ignores the grouping.

## Task 5.09 — Chained division cascades left

```text
holy> 1000 / 10 / 10;
= 0x0000000A (10)

holy> 1000 / (10 * 10);
= ... same thing here — but:

holy> 64 / 4 * 2;
= 0x0000020 (32)      ; (64/4)*2 = 32, NOT 64/(4*2)=8
```

## Task 5.10 — Digit separators for readability

```text
holy> 0xDEAD_BEEF + 0;
= 0xDEADBEEF (3735928559)

holy> 1_000_000 / 1000;
= 0x000003E8 (1000)
```

Underscores are stripped by the lexer — pure formatting.

## Task 5.11 — Literal overflow is caught at lex time

```text
holy> 0x1FFFFFFFF + 0;
ERR LEX
```

Values above `0xFFFFFFFF` never enter the parser. Split your constant or
mask it by hand before typing.

## Task 5.12 — Division by zero dies at PARSE time

```text
holy> 5 / 0;
ERR DIV BY ZERO

holy> poke 0x20000100 1 / 0;
ERR DIV BY ZERO
```

No runtime fault, no partial execution. The line is rejected whole.

## Task 5.13 — Modulo by zero too

```text
holy> 5 % 0;
ERR DIV BY ZERO
```

Same guard, same moment.

## Task 5.14 — Division by an expression that folds to zero

```text
holy> 5 / 2 - 2;
ERR DIV BY ZERO
```

The divisor `2-2` folded to 0 during parsing — caught even though no
literal `0` appears in your source.

## Task 5.15 — Bind once, reuse everywhere

```text
holy> let base = 0x40020000;
base = 0x40020000 (1073872896)

holy> base + 0x18;
= 0x40020018 (1073872920)

holy> base / 4096;
= 0x00040020 (262176)
```

## Task 5.16 — Names compose into new names

```text
holy> let word = 0x12345678;
word = 0x12345678 (305419896)

holy> let hi = word / 65536;
hi = 0x00001234 (4660)

holy> let lo = word % 65536;
lo = 0x00005678 (22136)

holy> hi * 65536 + lo;
= 0x12345678 (305419896)
```

## Task 5.17 — Rebinding replaces the value

```text
holy> let x = 1;
x = 0x00000001 (1)

holy> let x = x + 1;
x = 0x00000002 (2)
```

Each binding immutable at evaluation; rebinding just overwrites the symbol
slot. Counters work fine across REPL lines.

## Task 5.18 — peek inside arithmetic reads ONCE

```text
holy> cap_claim GPIOA;
CAP CLAIMED GPIOA id=0

holy> let idr = peek 0x40020010;
idr = 0x00000000 (0)

holy> poke 0x40020018 0xFF;
OK

holy> idr;
= 0x00000000 (0)
```

`idr` captured the old state forever. Re-peek for fresh values.

## Task 5.19 — Fresh read vs frozen copy

```text
holy> peek 0x40020010;
= 0x000000FF (255)
```

Top-level `peek ADDR;` always executes a live volatile load.

## Task 5.20 — Build any mask from powers of two

No shift operator? Multiply.

```text
holy> let bit3 = 1 * 8;
bit3 = 0x00000008 (8)

holy> let bits_3_5 = 8 + 32;
bits_3_5 = 0x00000028 (40)
```

For shifts beyond 2^16, type the hex literal directly — clearer anyway.

## Task 5.21 — Extract-and-compare pattern

**Goal:** Branch-free status checks.

```text
holy> let status = 0b... → 0xD;
ERR LEX
```

(no binary literals — use hex)

```text
holy> let status = 0xD;
status = 0x0000000D (13)

holy> status % 2;
= 0x00000001 (1)

holy> status / 4 % 2;
= 0x00000001 (1)

holy> status / 2 % 2;
= 0x00000000 (0)
```

Bits 0 and 2 set, bit 1 clear — decided entirely in arithmetic.

## Task 5.22 — Round up with div/mod

```text
holy> let n = 17;
n = 0x00000011 (17)

holy> (n + 3) / 4;
= 0x00000005 (5)
```

Ceiling of n/4 = `(n+3)/4`. The classic trick works because everything
truncates toward zero.

## Task 5.23 — Two's complement negation trick

```text
holy> let a = 100;
a = 0x00000064 (100)

holy> 0 - a;
= 0xFFFFFF9C (4294967196)
```

`0 - a` yields the two's complement. Add it back to verify: `a + 0xFFFFFF9C`
wraps to 0.

## Task 5.24 — Unsupported operators fail cleanly

```text
holy> 3 << 2;
ERR UNSUPPORTED OPERATOR

holy> 3 & 1;
ERR UNSUPPORTED OPERATOR
```

The lexer recognizes `< > ! & | ^ ~ : ?` but the grammar rejects them.
Multiply/divide/modulo replace shifting/masking by design.

## Task 5.25 — A full desk-check session

**Goal:** Verify the evaluator against known answers before trusting it
with hardware math.

```text
holy> 9 + 9;
= 0x00000012 (18)

holy> 9 * 9;
= 0x00000051 (81)

holy> 81 / 9 % 10;
= 0x00000000 (0)

holy> 81 / 9 + 1;
= 0x0000000A (10)

holy> 0xFFFF_FFFF + 1;
= 0x00000000 (0)
```

All eight behaviors confirmed: add, mul, div, mod, precedence, wrapping,
parse-time guards. Your calculator is certified.

---
*End of Chapter 5 — 125/250*
