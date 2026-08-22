# The Holy Rust Cookbook

## Prelude: Syntax Rules

*Read this first. Every recipe in this cookbook assumes you know these rules.*

Holy Rust is a small language by design. The entire grammar fits on one page
because the manifesto demands it: LL(1), single pass, left-to-right, no AST,
no precedence levels. What you type is what executes, in the order you typed it.

---

## P.1 The Golden Rules

1. **Every statement ends with `;`** — except `help`, `banner`, and `sys_audit`,
   where the semicolon is optional.
2. **Expressions evaluate strictly left-to-right.** There is NO operator
   precedence. `2 + 3 * 4` is `(2 + 3) * 4 = 20`, not 14.
3. **Everything is a 32-bit unsigned integer.** Addresses, values, variables,
   function results. All arithmetic wraps (`wrapping_add/sub/mul`).
4. **All names are constants.** A `let` binding can never be reassigned to a
   different value — wait, it can be re-bound, but each binding is immutable
   at evaluation time. `peek ADDR` inside an expression reads memory ONCE, at
   compile time.
5. **One line = one statement.** The REPL reads a line (up to 128 bytes),
   compiles it in a single pass, and executes it immediately.

---

## P.2 Numbers

| Form | Example | Meaning |
|------|---------|---------|
| Decimal | `42` | 42 |
| Hexadecimal | `0x2A` | 42 |
| Hex with separators | `0x4002_0000` | 1073872896 (underscores ignored) |
| Decimal with separators | `1_000_000` | 1000000 |

- Hex prefix accepts `0x` or `0X`; digits `0-9`, `a-f`, `A-F`.
- Overflow is detected at lex time: `ERR LEX` ("literal overflow").
- There are **no negative literals**. `-` is a binary operator only.
  To get 0xFFFFFFFF, write `0xFFFFFFFF` or compute `0 - 1`.

---

## P.3 Names (Identifiers)

- Characters: `a-z`, `A-Z`, `0-9`, `_`. Must start with a letter or `_`.
- Maximum length: **16 characters** (`NAME_MAX`). Longer → `ERR NAME TOO LONG`.
- Case-sensitive: `led_on` and `LED_ON` are different names.
- Keywords (reserved): `let`, `fn`.
- Command words (`peek`, `poke`, `cap_claim`, ...) are recognized contextually,
  not reserved — but do not use them as variable names.

---

## P.4 Statements (the complete list)

### Bindings
```text
let NAME = EXPR;
```
Evaluates `EXPR` once, stores the result in the symbol table (32 slots).
Prints `NAME = 0xHEX (decimal)`.

### Functions
```text
fn NAME() { STMT; STMT; ... }
```
Compiles the body into the JIT pipeline. Body statements may be `poke`,
`peek`, calls to previously-defined functions, or bare expressions.
No semicolon needed after `}`. Max 2 live functions, max 32 stream words
per body. Prints `FN NAME DEFINED`.

### Memory access (capability-enforced)
```text
peek ADDR;
poke ADDR VALUE;
reg_set_bit ADDR BIT;
reg_clr_bit ADDR BIT;
```
`ADDR` and `VALUE` are full expressions. Peripheral addresses require the
matching capability token — the check happens at PARSE time, before any
code runs. SRAM and flash addresses are unrestricted.

### Capabilities
```text
cap_claim NAME;
cap_drop NAME;
```
Valid names: `GPIOA`, `GPIOB`, `UART0`, `SPI0`, `I2C0`, `TIMER0`, `DMA0`,
`SUPERUSER`.

### Expressions
```text
EXPR;
```
Evaluates and prints `= 0xHEX (decimal)`. If the expression contains only
side effects (e.g. a poke inside a fn), prints `OK`.

### System commands
```text
help        ; optional semicolon
banner      ; optional semicolon
sys_audit   ; optional semicolon
```

---

## P.5 Operators

| Op | Meaning | Notes |
|----|---------|-------|
| `+` | add | wrapping |
| `-` | subtract | wrapping |
| `*` | multiply | wrapping |
| `/` | divide | divisor 0 → `ERR DIV BY ZERO` at parse time |
| `%` | modulo | divisor 0 → `ERR DIV BY ZERO` at parse time |
| `=` | assignment | only in `let` |

All binary operators have **equal precedence** and associate left:

```text
holy> 2 + 3 * 4;
= 0x00000014 (20)

holy> (2 + 3) * 4;
= 0x00000014 (20)          ; same thing — parens are decorative here

holy> 100 / 7 % 3;
= 0x00000002 (2)           ; ((100 / 7) % 3) = (14 % 3) = 2
```

Lexed-but-unsupported operators (`< > ! & | ^ ~ : ?`) produce
`ERR UNSUPPORTED OPERATOR` if used in an expression.

---

## P.6 `peek` as an Expression Term

`peek ADDR` is a first-class term anywhere an expression expects a value:

```text
holy> cap_claim GPIOA;
CAP CLAIMED GPIOA id=0

holy> let moder = peek 0x40020000;
moder = 0x0A8000C00 (...)

holy> peek 0x40020010 + 1;     ; read IDR, add one
= ...
```

The read happens **at compile time** (during parsing of the line). The bound
symbol captures that instant's value forever. Ring 0 makes no promises: a
compile-time peek of a wild address faults exactly like a runtime one.

---

## P.7 Capability Enforcement (parse-time)

When a statement references a peripheral address, the parser checks the
registry **before emitting any code**:

```text
holy> poke 0x40020000 0x01;
ERR E001: CAPABILITY_VIOLATION - Peripheral token not claimed

holy> cap_claim GPIOA;
CAP CLAIMED GPIOA id=0

holy> poke 0x40020000 0x01;
OK
```

- SRAM (`0x2000xxxx`), flash (`0x08xxxxxx`), and unmapped addresses need no
  capability.
- `SUPERUSER` bypasses every check but logs every write to the audit ring.
- Function bodies are checked at DEFINITION time too.

---

## P.8 Limits (hard constants from the source)

| Limit | Value | Error when exceeded |
|-------|-------|---------------------|
| Input line length | 128 bytes | bytes silently dropped |
| Token stream | 128 words | `ERR STREAM FULL` |
| Symbol table | 32 slots | `ERR SYMBOL TABLE FULL` |
| Live functions | 2 | `ERR FN TABLE FULL` |
| Fn body size | 32 words | `ERR STREAM FULL` |
| Name length | 16 chars | `ERR NAME TOO LONG` |
| Operand stack | 64 words | silent drop (never panics) |

---

## P.9 Line Editing

| Key | Action |
|-----|--------|
| Backspace / DEL | erase last character |
| Ctrl-U | kill entire line |
| Ctrl-C | cancel input, fresh prompt |
| Enter | submit line for evaluation |

---

## P.10 Comments

There are **no comments in the language**. The lexer treats every byte as
potential syntax; a `//` would lex as two division operators. For annotated
hardware sequences, keep your notes on the host side and paste plain commands.

---

## P.11 Reading the Output

```text
holy> let x = 42;
x = 0x0000002A (42)
```

Every printed value is `0x` + 8 hex digits, followed by the decimal in
parentheses. Errors print as `ERR <REASON>` or `E001/E002: <CATEGORY>`.

---

That is the whole language. Now go drive some hardware.
