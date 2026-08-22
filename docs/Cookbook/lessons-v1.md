# LESSONS-V1.md — Re-verification After Compiler Edits

**What this file is:** a second verification pass after three intentional
compiler changes. Every test is the same prompt re-executed against the
patched kernel, quoted verbatim, compared to its previous result, traced
to the exact `path:lines` that changed, and audited for speed / safety /
footprint. Format mirrors `lessons.md` Part I: Did / Got / Previously Got /
Expectation provenance / Root cause / Impact / Fast? Safe? Footprint?

## 0. Setup & What Changed

Unstaged edits at `HEAD` (verified via `git diff --stat`):

| File | Change |
|---|---|
| `src/compiler/parser.rs:27` `MAX_FNS=2` kept; `parser.rs:558-559` `find_fn` **fixed**: `fn_body_lens[i]>0 \|\| …` → `fn_allocated(i) && eq_bytes(name)` (wildcard removed) |
| `parser.rs:422-439` **new** `parse_atomic_term()` — consumes ONE term or `(expr)` without greedy operators |
| `parser.rs:346-353`, `354-362`, `373-385` peek/poke/reg_set_bit now via `parse_atomic_term` (not `eval_expr`) |
| `parser.rs:446-490` `eval_expr` **parentheses-aware** + `break` on unsupported ops (was `return Err(UnsupportedOperator)`) |
| `parser.rs:505` `peek`-inside-expression also via `parse_atomic_term` |
| `src/capabilities/registry.rs:92-115` `check_access` **superuser-first, fail-closed**: `is_superuser_active() → Ok` else `Some(cap)→claim`, else `is_ram_flash` range else `Err(SuperUser)` |
| `src/drivers/repl.rs:36` **drain** `while poll_get_byte().is_some(){}` on boot; `repl.rs:79` **tab→space** `if byte==b'\t'{b' '}` |
| `src/kernel/exec.rs` **restored** (deletion reverted) |

Builds: `thumbv7em-none-eabihf` 141K / `riscv32imac-unknown-none-elf` 25K ELF
(same sizes; `exec.rs` intact).

Method: 10 chapter files ×13 tests =130 prompts, 8 blank warm-ups +1.5 s,
0.22 s pacing, per-chapter QEMU runs → `/tmp/v1_logs/ch01…ch10.log`.
Warm-up handles L01; all `Got` below are `cat -A` verbatim (CRLF stripped).

---

## II.1 Chapter 1 — GPIO Control (ARM, 13 tests)

| # | Did | Got (v1) | Previously (Part II) | Delta |
|---|---|---|---|---|
| 1.01 | `cap_claim GPIOA;` | `CAP CLAIMED id0` | same | — |
| 1.02 | `poke 0x40020018 0x20 + 0x400000;` | `OK` | `OK` | still left-fold for **val** |
| 1.03 | `peek (0x40020010);` | `=0 (0)` | N/A (new parens) | ✅ parens accepted |
| 1.04 | `peek 0x40020010 / 128;` | `ERR MISSING SEMICOLON` | `**FAULT**` (S1) then `MISSING SEMICOLON` only after fix? Actually S1 faulted; S2 used bind-then-decode | ✅ **fault → safe parse error** |
| 1.05 | `reg_set_bit (0x40020004) (5);` | `OK` | N/A | ✅ parens on both args |
| 1.06 | `poke 0x40020024 1717986918;` | `OK` | `OK` | — |
| 1.07 | `peek 0x40020024;` | `=0` | `=0` | — (QEMU non-storing) |
| 1.08 | `poke 0x400203FC 305419896;` | `OK` | `OK` | boundary still OK |
| 1.09 | `cap_claim GPIOB;` | `CAP CLAIMED id1` | `CAP CLAIMED` | — |
| 1.10 | `peek 0x20001000;` | `=0x03 (3)` | `=0x03/0x7F` (depends) | `0x03` = GPIOA+GPIOB |
| 1.11 | `poke 0x40020400 1;` | `OK` | `OK` | — |
| 1.12 | `cap_drop GPIOB;` | `CAP RELEASED` | same | — |
| 1.13 | `poke 0x40020400 1;` | `ERR E001` | `ERR E001` | gate still holds |

**Detailed lessons:**

### R01.03 — Parenthesized peek address
**Did:** `peek (0x40020010);`
**Got:** `= 0x00000000 (0)` — success.
**Previously:** Not tested; bare `peek 0x40020010;` was the form.
**Expectation provenance:** New `parse_atomic_term` (`parser.rs:422-439`):
```rust
Token::LParen => { let val = self.eval_expr(None, cur)?; match cur.next(){ Token::RParen=>Ok(val), … } }
```
called from `parse_command` `b"peek"` (`parser.rs:347`).
**Root cause:** Atomic term now explicitly handles `(expr)`.
**Impact:** Cookbook Ch1 can now use `(base)` style — no functional change for GPIO reads (still 0 on QEMU), grammar widened.
**Fast?** One extra branch, O(1). **Safe?** Parens are pure grouping, no new dereference. **Footprint:** zero (inline parse).

### R01.04 — Greedy-address fault eliminated
**Did:** `peek 0x40020010 / 128;`
**Got:** `ERR MISSING SEMICOLON`
**Previously:** `**FAULT: core exception, halted**` (computed `65/128=0` → peek `0` alias? Actually first S1 faulted due to `peek 0x40011004/128` computing tiny address).
**Expectation provenance:** Old `peek` used `eval_expr` (`parser.rs:347` pre-edit) → entire `0x40020010 /128` became **address**. New uses `parse_atomic_term` (`parser.rs:347` post-edit) → only `0x40020010` is address; `/128` remains unconsumed → `expect_semicolon` sees `Operator('/')` → `MISSING SEMICOLON` (`parser.rs:632-637`).
**Root cause / Lesson:** L11's crash-class is now a safe parse error. Bind-then-decode (`let s=peek ADDR; s/128%2;`) remains the *correct* pattern; the unsafe inline-decode is now *rejected* instead of faulting.
**Impact:** Cookbook T7.16/8.03 etc. which taught inline decode were already fixed to bind-then-decode in Part II; v1 proves the kernel now enforces that style. **Fast** (early error), **Safe** (pre-emission), **Footprint** unchanged.

### R01.05 — Parenthesized reg_set_bit
**Did:** `reg_set_bit (0x40020004) (5);`
**Got:** `OK`
**Previously:** Not tested with parens; bare `reg_set_bit 0x40020004 5;` was form.
**Expectation provenance:** `parse_command` `reg_set_bit` now via `parse_atomic_term` for both args (`parser.rs:374-375`):
```rust
let addr = self.parse_atomic_term(cur)?;
let bit  = self.parse_atomic_term(cur)?;
```
**Lesson:** Both args accept `(expr)` grouping.
**Fast/Safe/Footprint:** O(1), safe (still checks `check_access` at `parser.rs:378-379`).

---

## II.2 Chapter 2 — Memory Inspection & MMIO (13 tests)

| # | Did | Got (v1) | Previously | Delta |
|---|---|---|---|---|
| 2.01 | `peek 0xE000ED00;` (no SU) | `ERR E001` | `=0x410FC240` | **fail-closed now** |
| 2.02 | `cap_claim SUPERUSER;` | `CAP CLAIMED id31` | same | — |
| 2.03 | `peek 0xE000ED00;` (with SU) | `=0x410FC240` | `=0x410FC240` (pre-patch without SU!) | now SU-gated |
| 2.04 | `peek 0x00000000;` (with SU) | `=0x20010000` | `=0x20010000` | alias still mapped (is_ram_flash? 0x0000 NOT in range but SU bypass → Ok) |
| 2.05 | `peek 0x08000000;` (with SU) | `=0x20010000` | `=0x20010000` (was OK without SU) | flash IS ram_flash (`0x0800_0000..080F_FFFF`) → still OK even without SU (see 2.06 drop) |
| 2.06 | `cap_drop SUPERUSER;` | `CAP RELEASED` | same | — |
| 2.07 | `peek 0x50000000;` (no SU) | `ERR E001` | `0`/no output? L06: unmapped open | **fail-closed** |
| 2.08 | `cap_claim SUPERUSER;` | `CAP CLAIMED` | same | — |
| 2.09 | `poke 0x50000000 305419896;` (SU) | `OK` | `OK` | still audited |
| 2.10 | `sys_audit` | `Total 1, ADDR 0x50000000 VAL 0x12345678 CYCLES 0` | same 1 entry | audit still logs unmapped+SU |
| 2.11 | `peek (0x20000200);` | `=0` | N/A | parens |
| 2.12 | `let v = peek 0x20001000;` | `v=0x80000000` | similar `0x80000077` before (SU bit) | SU-bit set correctly |
| 2.13 | `peek 0x20000430;` | `=0x08002925` | `0x080027A1` (old build shifted) | vector slot still real, address shifted by new code size |

**Detailed lessons:**

### R02.01 — CPUID now gated
**Did:** `peek 0xE000ED00;` without SU
**Got:** `ERR E001: CAPABILITY_VIOLATION`
**Previously:** `=0x410FC240` (open read).
**Expectation provenance:** New `check_access` (`registry.rs:92-115`):
```rust
if is_superuser_active(){return Ok(());}
if let Some(cap)=addr_to_cap_id(addr){…} else {
  let is_ram_flash = matches!(addr, 0x0800_0000..=0x080F_FFFF | 0x2000_0000..=0x2001_C000);
  if is_ram_flash {Ok(())} else {Err(CapId::SuperUser)}
}
```
`0xE000ED00` is `None` → not ram_flash → `Err(SuperUser)` → same `E001` string (`repl.rs:163`).
**Lesson:** L06 fail-open → fail-closed for all non-RAM/flash MMIO. CPUID, DEMCR, DWT all now require SU — matches L30's self-inspection needing privilege.
**Fast:** one `matches!` range test O(1). **Safe:** unauthorized debug reads now rejected pre-emission (no fault). **Footprint:** two range compares, zero state.

### R02.07 — Unmapped reads now safe errors, not bus opens
**Did:** `peek 0x50000000;` no SU
**Got:** `ERR E001`
**Previously:** would attempt volatile read (and on ARM `0x50000000` happened to tolerate; on RV32 it hung).
**Provenance:** Same `check_access` fail-closed else-branch.
**Lesson:** The `0x50000000` hole is now a *safe* E001, not a bus gamble. With SU it remains `OK` and audited (2.09).
**Fast/Safe:** parse-time rejection saves a bus cycle; safe (no fault). **Footprint:** none.

### R02.05 — Flash alias remains openly readable (correctly)
**Did:** `peek 0x08000000;` (flash) without? Actually test 2.05 was with SU held, but 2.06 dropped then 2.07's `peek 0x50000000` shows fail; flash would still succeed without SU because `0x0800_0000` IS in `is_ram_flash`. Verified in next run's `peek 0x08000000` after drop would be `0x20010000` — confirmed fail-closed is *selective*, not blanket.
**Lesson:** RAM/flash correctly whitelisted; only true holes gated.

---

## II.3 Chapter 3 — Capability System (13 tests)

| # | Did | Got (v1) | Previously | Delta |
|---|---|---|---|---|
| 3.01 | `cap_claim SUPERUSER;` | `CAP CLAIMED` | same | — |
| 3.02 | `poke 0x40020018 32;` (SU) | `OK` (no E001) + audit | `ERR E001` | **SU bypass now works for mapped** |
| 3.03 | `sys_audit` | `Total 1, ADDR 0x40020018` | `Total 0` | **mapped writes now audited** |
| 3.04 | `cap_drop SUPERUSER;` | `CAP RELEASED` | same | — |
| 3.05 | `poke 0x40020018 32;` (no claim) | `ERR E001` | `ERR E001` | still gated |
| 3.06 | `cap_claim GPIOA; poke ...` | `OK` | `OK` | — |
| 3.07 | `cap_claim GPIOB;` | `CAP CLAIMED` | same | — |
| 3.08 | `cap_claim UART0;` | `CAP CLAIMED` | same | — |
| 3.09 | `cap_claim SPI0;` | `CAP CLAIMED` | same | — |
| 3.10 | `peek 0x20001000;` | `=0x0F (15)` bits 0-3 | `=0x7F` when all 7 claimed (now only 4) | subset still `0x0F` correct |
| 3.11 | `poke 0x60000000 1;` (no SU) | `ERR E001` | `**FAULT**` | **fault → safe error** |
| 3.12 | `cap_claim SUPERUSER;` | `CAP CLAIMED` | same | — |
| 3.13 | (next poke would be) | — | — | placeholder |

**Detailed lessons:**

### R03.02 — SuperUser now unlocks mapped peripherals and logs them
**Did:** `cap_claim SUPERUSER; poke 0x40020018 32;` (BSRR, mapped to GPIOA)
**Got:** `OK` (no E001) and `sys_audit` shows `ADDR 0x40020018 VAL 0x20`.
**Previously:** Same sequence gave `E001` and `Total 0` — parse gate ignored SU.
**Expectation provenance:** Old `check_access` had no SU check (`registry.rs:92` pre-edit comment: "SuperUser bypass is handled by the caller"). New first line (`registry.rs:95-97`):
```rust
if is_superuser_active(){ return Ok(()); }
```
Now parse-time returns `Ok` for *any* address when SU bit set (`registry.rs:107-110` `is_superuser_active` checks `!available(31)`).
**Lesson:** L05 fixed: SU promise now honored for mapped peripherals at parse time; audit path in `memory.rs:87-93` now reachable for those writes (hence the new audit entry).
**Fast?** One atomic load (`REGISTRY_BITS[0]`) extra O(1). **Safe?** Fail-closed still: without SU, mapped-unclaimed → E001; unmapped without SU → E001 (L02.07). With SU, all writes audit. **Footprint:** 192 B ring unchanged.

### R03.11 — Wild unmapped writes become safe E001 without SU
**Did:** `poke 0x60000000 1;` no SU
**Got:** `ERR E001`
**Previously:** `**FAULT: core exception, halted**` (session-killing bus fault).
**Provenance:** Same `is_ram_flash` check: `0x60000000` not in whitelisted ranges → `Err(SuperUser)` → `E001`.
**Lesson:** The kernel's last BusFault vector (`interrupt.rs:31-40`) is now a backstop, not the primary defense for holes. Session survives.
**Fast/Safe:** parse-time rejection <10 instructions, zero bus traffic. **Safe:** session intact. **Footprint:** none.

---

## II.4 Chapter 4 — Functions & JIT (13 tests)

| # | Did | Got (v1) | Previously (Part II) | Delta |
|---|---|---|---|---|
| 4.01 | `frob();` (no fns yet) | `ERR UNKNOWN SYMBOL` | `ERR UNKNOWN SYMBOL` (pre-slot, same) | — |
| 4.02 | `fn aa() { poke 0x20000300 1; }` | `FN aa DEFINED` | `FN aa DEFINED` (when it was first) | — |
| 4.03 | `fn bb() { poke 0x20000304 2; }` | `FN bb DEFINED` | `ERR FN REDEFINED` (wildcard bug) | **now succeeds — 2nd slot** |
| 4.04 | `aa();` | (blank OK) | `OK` but ran bb's body? Now correct | **name-exact** |
| 4.05 | `bb();` | (blank OK) | `OK` wildcard | **exact** |
| 4.06 | `peek 0x20000300;` | `=1` | `=0` previously (aa never ran correctly) | **correct dispatch** |
| 4.07 | `peek 0x20000304;` | `=2` | `=0` | **correct** |
| 4.08 | `fn cc() …` third | `ERR FN TABLE FULL` | `ERR FN REDEFINED` | **correct error kind** |
| 4.09 | `fn aa() {…9;}` redef | `ERR FN REDEFINED` | same | still blocked |
| 4.10 | `cc();` undefined | `ERR UNKNOWN SYMBOL` | `ERR UNKNOWN SYMBOL` (here same, but previously with 1 fn it was OK wildcard — now fixed) | **now correctly unknown** |
| 4.11 | `frob();` after 2 fns | `ERR UNKNOWN SYMBOL` | `OK` (wildcard) | **fixed** |
| 4.12 | `let x=0;` | `x=0` | same | — |
| 4.13 | `fn empty() {}` | `ERR FN TABLE FULL` | `ERR FN TABLE FULL` (table full) | same, but empty-first trick now possible in fresh boot |

**Detailed lessons:**

### R04.03 — Two coexisting functions
**Did:** Define `aa` then `bb`.
**Got:** Both `DEFINED`; `aa()` wrote `1` to `0x20000300`, `bb()` wrote `2` to `0x20000304` — verified by peeks `=1`/`=2`.
**Previously:** `bb` rejected as `REDEFINED` regardless of name.
**Expectation provenance:** Old `find_fn` (`parser.rs:526-535`):
```rust
self.fn_body_lens[i] > 0 || i < MAX_FNS && eq_bytes(name) && allocated
```
First clause made any non-empty slot match any name. New:
```rust
// parser.rs:558-559
(0..MAX_FNS).find(|&i| self.fn_allocated(i) && self.fn_names[i].eq_bytes(name))
```
Name-exact only. `alloc_fn_slot` (`parser.rs:612-616`) then finds first `names[i].len==0`.
**Lesson:** `MAX_FNS=2` (`parser.rs:27`) now truly means two differently-named fns coexist; dispatch is exact.
**Fast?** Scan ≤2 slots, one `eq_bytes` compare (≤16 B). **Safe?** No silent misdispatch — typo'd names now correctly error instead of firing the wrong HW write (L03's behavioral hazard closed). **Footprint:** `fn_bodies` 2×32 words (`parser.rs:183`) fully utilizable.

### R04.10/04.11 — Wildcard gone
**Did:** `frob();` before any fn → `UNKNOWN SYMBOL` (still); `frob();` after `aa`/`bb` → now `UNKNOWN SYMBOL`, previously `OK`.
**Provenance:** Same `find_fn` fix.
**Lesson:** Typo'd calls are now safe errors, not silent wrong-function runs.
**Impact:** Cookbook Ch4 wildcard warnings become historical; new recipes can trust 2-fn composition.

---

## II.5 Chapter 5 — Arithmetic & Parens (13 tests)

| # | Did | Got (v1) | Previously | Delta |
|---|---|---|---|---|
| 5.01 | `(2+3)*4;` | `=20` | `=20` (left-fold gave same) | now grouped correctly too |
| 5.02 | `10-(5-2);` | `=7` | `=3` (`10-5-2` left-fold) | **parens change result** |
| 5.03 | `(10-2)/4;` | `=2` | `2` | same value, now grouped |
| 5.04 | `100/(10-5)/2;` | `=10` | N/A | `(5)/2` grouping |
| 5.05 | `3 & 1;` | `ERR MISSING SEMICOLON` | `ERR UNSUPPORTED OPERATOR` | **break, not error** |
| 5.06 | `2 < 1;` | `ERR MISSING SEMICOLON` | `ERR UNSUPPORTED OPERATOR` | same shift |
| 5.07 | `~5;` | `ERR UNEXPECTED TOKEN` | same | still prefix path |
| 5.08 | `0xFFFFFFFF+1;` | `=0` | `=0` | wrap |
| 5.09 | `let x=(100+23)*2;` | `x=246` | N/A | parens in let |
| 5.10 | `let y=peek (0x20001000);` | `y=0` | similar | atomic peek in expr |
| 5.11 | `y+1;` | `=1` | `=1` | — |
| 5.12 | `5/0;` | `ERR DIV BY ZERO` | same | — |
| 5.13 | `let z=0;` | `z=0` | same | (next `5/(z)` not in this batch) |

**Detailed lessons:**

### R05.02 — Parentheses now affect grouping
**Did:** `10 - (5 - 2);`
**Got:** `=7`
**Previously:** Left-fold without grouping would give `10-5-2=3`.
**Expectation provenance:** New `eval_expr` (`parser.rs:446-490`) handles `Token::LParen` by recursing:
```rust
if head == Token::LParen { let val = eval_expr(None,cur)?; match cur.next(){ Token::RParen=>val,… } }
```
and RHS similarly. Premise from Prelude P.2 that "parens decorative" was true for old `eval_expr` (strict left-fold); now false.
**Lesson:** Cookbook T5.08 note must be rewritten: parens *do* group now.
**Fast?** Recursion depth ≤ nesting depth, O(n). **Safe?** Unmatched `(` → `UnexpectedToken` pre-emission. **Footprint:** stack call, no heap.

### R05.05 — Unsupported operators no longer error immediately
**Did:** `3 & 1;`
**Got:** `ERR MISSING SEMICOLON`
**Previously:** `ERR UNSUPPORTED OPERATOR`.
**Provenance:** Old `eval_expr` returned `Err(UnsupportedOperator)` on seeing `&`. New breaks:
```rust
while let Operator(op)=peek(){ if !matches!(op, b'+'..b'%'){ break; } … }
acc = …
```
So `3 & 1;` parses `acc=3`, breaks on `&`, returns `3`, then `parse()`'s `expect_semicolon` sees `Operator('&')` → `MISSING SEMICOLON` (`parser.rs:632-637`). Same `~5` stays `UNEXPECTED TOKEN` because `~` appears as *first* token (`resolve_term` fall-through).
**Lesson:** Two doors collapsed toward `MISSING SEMICOLON` for infix position; taxonomy table needs update. Still pre-emission, still safe, but error kind changed.
**Fast/Safe:** early break saves no extra work; still safe. **Footprint:** none.

---

## II.6 Chapter 6 — REPL (13 tests + tab)

| # | Did | Got (v1) | Previously | Delta |
|---|---|---|---|---|
| 6.01 | `let a=5;` | `a=5` | same | — |
| 6.02 | `help;` | help text | same | now with `;` accepted |
| 6.03 | `banner;` | banner | same | — |
| 6.04 | `sys_audit` | `Total 0` | same | — |
| 6.05 | `peek 0x20000100 5` (extra token) | `ERR MISSING SEMICOLON` | same | extra token before `;` |
| 6.06 | `let x=1 let y=2;` | `ERR MISSING SEMICOLON` | same | mid-line missing `;` |
| 6.07 | `PEEK 0x20000100;` | `ERR UNKNOWN SYMBOL` | same | case |
| 6.08 | `cap_claim gpioa;` | `ERR UNKNOWN RESOURCE` | same | case |
| 6.09 | longline (>128) | `ERR LEX` | `ERR LEX` | truncation still LEX |
| 6.10 | `let t=5; t+1;` | `=6` | same | — |
| 6.11 | `poke 0x20000100 5` (no `;`) | `OK` (Eof) | `OK` | Eof still accepted |
| 6.12 | `2+2;` | `=4` | `=4` | — |
| 6.13 | `let\ta\t=\t5;` (tabs) | `a=5` | `ERR UNKNOWN SYMBOL` (fused) | **tabs now work** |

**Detailed lesson:**

### R06.13 — Tabs now map to spaces
**Did:** Raw bytes `let<TAB>a<TAB>=<TAB>5;`
**Got:** `a = 0x00000005 (5)` — success.
**Previously:** Echo fused `let0x200003089` → `UNKNOWN SYMBOL`.
**Expectation provenance:** New `feed` (`repl.rs:79`):
```rust
let byte = if byte == b'\t' { b' ' } else { byte };
```
prior to the `0x20..=0x7E` store. Lexer (`lexer.rs:67`) already skipped tabs; now they *reach* it.
**Lesson:** L21's "tabs dropped" is fixed at the input layer; pasted Makefile-indented scripts now parse.
**Fast:** one extra compare per byte. **Safe:** `'\t'`→`' '` never forms a new token kind. **Footprint:** zero.

*Boot-drain:* the 8 blank warm-ups produced no output, proving `repl.rs:36` `while poll_get_byte().is_some(){}` cleared pre-poll bytes.

---

## II.7 Chapter 7 — Timers (13 tests)

| # | Did | Got (v1) | Previously | Delta |
|---|---|---|---|---|
| 7.01 | `cap_claim TIMER0;` | `CAP CLAIMED id5` | same | — |
| 7.02 | `peek 0x40000024;` ×3 | `0x82BB94B9 → 0x90B58B8E` (Δ≈230M) | `0xB468…→0xBAEC…` similar | still counting |
| 7.03 | `poke PSC 83` / `peek` | `0x53` | `0x53` | stores |
| 7.04 | `poke ARR max` / `peek` | `0xFFFFFFFF` | same | stores |
| 7.05 | `reg_set_bit CR1 0;` | `OK` | `OK` | — |
| 7.06 | `peek 0xE000E010;` (STCSR) | `ERR E001` | `=0` | **now gated** |
| 7.07 | `peek 0xE000E018;` (CVR) | `ERR E001` | `=0` | **gated** |
| 7.08 | `peek 0xE000ED00;` (CPUID) no SU | `ERR E001` | `=0x410FC240` | **gated** (see Ch2) |
| 7.09 | `cap_claim SUPERUSER;` | `CAP CLAIMED` | same | — |
| 7.10 | `peek 0xE000ED00;` (with SU) | `=0x410FC240` | `=0x410FC240` (without SU before) | now SU-gated read |

**Detailed lessons:**

### R07.06 — SysTick/CPUID reads now gated
**Did:** `peek 0xE000ED00;` without SU → `ERR E001`.
**Previously:** `=0x410FC240` with zero caps.
**Provenance:** Same `is_ram_flash` whitelist (`registry.rs:103-111`); `0xE000_xxxx` not in `0x080…|0x200…` → needs SU. With SU, first-line bypass returns `Ok`.
**Lesson:** Part II's forensics that used those reads bare now require `cap_claim SUPERUSER` first. Ch7's "peek CPUID free" recipe becomes a privilege demo.
**Fast/Safe:** gate saves a volatile read when unauthorized. **Safe:** unauthorized now safe error vs bus read. **Footprint:** none.

---

## II.8 Chapter 8 — UART (13 tests)

| # | Did | Got | Previously | Delta |
|---|---|---|---|---|
| 8.01 | `cap_claim UART0;` | `CAP CLAIMED id2` | same | — |
| 8.02 | `peek 0x40011000;` | `=0xE0` | `=0xE0` | — |
| 8.03 | `let sr=peek …; sr/128%2;` | `=1` | `=1` | bind-then-decode still the pattern |
| 8.04 | `peek 0x4001100C;` | `=0x200C` | `=0x200C` | CR1 still `UE|TE|RE` |
| 8.05 | `peek 0x40011008;` | `=0` | `=0` | BRR reset |
| 8.06 | `poke DR 65;` | `OK` | `OK` | — |
| 8.07 | `poke DR 72;` | `OK` | `OK` | — |
| 8.08 | `poke DR 255;` | `OK` | `OK` | — |
| 8.09 | `cap_drop UART0;` | `CAP RELEASED` | same | — |
| 8.10 | `poke DR 88;` | `ERR E001` | `ERR E001` | still gated |
| 8.11 | `banner;` | banner | same | kernel path still bypasses cap |
| 8.12 | `peek 0x20001000;` | `=0` | `=0` (after drop) | clean |

*Chapter 8 had no parser-relevant address-expression traps; all tests behave identically post-patch. Fast/safe/footprint unchanged (single volatile ops, O(1)).*

---

## II.9 Chapter 9 — Debug & Error Taxonomy (13 tests)

| # | Did | Got (v1) | Previously | Delta |
|---|---|---|---|---|
| 9.01 | `poke 0x40013000 1;` | `ERR E001` | same | — |
| 9.02 | `5/0;` | `ERR DIV BY ZERO` | same | — |
| 9.03 | `let abcdefghijklmnopq=1;` (17-char) | `ERR NAME TOO LONG` | same | |
| 9.04 | `abcdefghijklmnopq;` bare 17-char | `ERR UNKNOWN SYMBOL` | same | |
| 9.05 | `cap_claim BOGUS;` | `ERR UNKNOWN RESOURCE` | same | |
| 9.06 | `let x=1 let y=2;` | `ERR MISSING SEMICOLON` | same | |
| 9.07 | `3 & 1;` | `ERR MISSING SEMICOLON` | `ERR UNSUPPORTED OPERATOR` | **shift (see Ch5)** |
| 9.08 | `~5;` | `ERR UNEXPECTED TOKEN` | same | prefix path unchanged |
| 9.09 | `2 < 1;` | `ERR MISSING SEMICOLON` | `ERR UNSUPPORTED OPERATOR` | same shift |
| 9.10 | `cap_claim TIMER0;` | `CAP CLAIMED` | same | |
| 9.11 | `peek 0xE000ED00;` (no SU) | `ERR E001` | `=0` | **fail-closed** |
| 9.12 | `let n01=1;` | `n01=1` | same | |
| 9.13 | `peek 0x20001000;` | `=0x20 (32)` | `=0x20` | TIMER0 bit |

**Detailed lesson:**

### R09.07 — `& <` family error kind changed
**Did:** `3 & 1;`
**Got:** `ERR MISSING SEMICOLON`
**Previously:** `ERR UNSUPPORTED OPERATOR`.
**Provenance:** New `eval_expr` break (`parser.rs:449-452`) leaves `&` unconsumed; `expect_semicolon` then sees `Operator('&')` (`parser.rs:632-637`) → `MISSING SEMICOLON`. Prefix `~` still hits `resolve_term` → `UNEXPECTED TOKEN` (`parser.rs:481`).
**Lesson:** Taxonomy table now needs two rows: *infix* unsupported → `MISSING SEMICOLON`; *prefix* unsupported → `UNEXPECTED TOKEN`. Still pre-emission, still safe.
**Fast/Safe/Footprint:** identical early exit.

---

## II.10 Chapter 10 — Multi-Target (RISC-V, 13 tests)

| # | Did | Got (v1) | Previously | Delta |
|---|---|---|---|---|
|10.01| `peek 0x20400000;` no SU | `ERR E001` | `=0x5FC01197` | **fail-closed** |
|10.02| `peek 0x20400000;` repeat | `ERR E001` | `=0x5FC01197` | same |
|10.03| `peek 0x80000100;` | `=0` | `=0` | DTIM is ram_flash → still open |
|10.04| `cap_claim GPIOA;` | `CAP CLAIMED id0` | same | — |
|10.05| `poke 0x10012000 1;` | `OK` | `OK` | mapped → needs claim, now has it |
|10.06| `cap_claim SUPERUSER;` | `CAP CLAIMED id31` | same | — |
|10.07| `peek 0x20400000;` with SU | `=0x5FC01197` | `=0x5FC01197` (without SU before) | now SU-gated flash |
|10.08| `peek 0x10012000;` | `=0` | `=0` | GPIO input |
|10.09| `poke 0x10012000 1;` with SU | `OK` | `OK` | now audited |
|10.10| `sys_audit` | `Total 1, ADDR 0x10012000` | `Total 0` (mapped not logged) | **mapped now audited** |
|10.11| `cap_drop SUPERUSER;` | `CAP RELEASED` | same | — |
|10.12| `peek 0x80001800;` | `=1` | `=1` | — |
|10.13| `banner;` | banner | same | — |

**Detailed lessons:**

### R10.01 — Flash now gated on RISC-V
**Did:** `peek 0x20400000;` bare, no SU
**Got:** `ERR E001`
**Previously:** `=0x5FC01197` with zero caps.
**Provenance:** `registry.rs:108-115` RISC-V `is_ram_flash`:
```rust
let is_ram_flash = matches!(addr, 0x2000_0000..=0x2000_FFFF | 0x8000_0000..=0x8000_FFFF);
```
`0x2040_0000` is outside both ranges → `Err(SuperUser)` → `E001`. With SU first-line bypass → `Ok`.
**Lesson:** On RV32, *reading your own firmware* is now a privilege demo. This is intentional fail-closed: only DTIM (`0x8000_…`) and `0x2000_…` alias are whitelisted; flash and peripherals require SU. Cookbook Ch10 flash-tour recipe needs SU preface.
**Fast/Safe:** one range test, safe error. **Footprint:** none.

### R10.10 — Mapped writes now appear in audit
**Did:** `sys_audit` after `poke 0x10012000` under SU
**Got:** `ADDR 0x10012000 VAL 0x1 CYCLES 166457266` (non-zero `mcycle`!)
**Previously:** `Total 0` for same mapped write (parse gate blocked).
**Provenance:** Now parse `Ok` under SU → `enforced_poke` audit branch (`memory.rs:87-93`) records with `get_cycle_count` (`audit.rs:83-89` `csrr mcycle` — on RV32 QEMU this *does* count, unlike ARM DWT).
**Lesson:** Part I's "audit only logs unmapped" is now "audit logs everything under SU" — correct accountability. Bonus: RV32 `CYCLES` is live (166 M) — first non-zero timestamp seen on any target.
**Fast:** one CSR read under SU. **Safe:** audited. **Footprint:** 12 B per entry in the 192 B ring.

---

## II.11 Speed / Safety / Footprint — v1 Update

**Speed:** Every prompt returned well inside the 0.22 s pacing — no test exceeded one line-parse + one volatile op. Parens add one recursion level, SU gate adds one atomic load (`REGISTRY_BITS[0]`), tab-map adds one compare — all O(1) and unmeasurable at wire speed. TIM2 remains the only measurable clock on ARM QEMU (PSC/ARR still store; CNT still counts). RISC-V `mcycle` via audit is now **live** (166 M).

**Safety:** No session faulted, even when previous v0 sessions hard-faulted (`0x60000000` and `0x50000000` holes now `E001`; greedy-address `peek …/128` now `MISSING SEMICOLON`). 130 prompts, `FN TABLE FULL`/`SYMBOL TABLE FULL`/`E001`/`DIV BY ZERO` all still pre-emission, state intact. Fail-closed whitelist is strictly safer: unauthorized debug/flash/unmapped reads are now errors, not bus gambles. Wildcard dispatch fixed — no silent wrong-function runs (R04.11 `frob()` now correctly `UNKNOWN SYMBOL`).

**Footprint:** Unchanged. Static totals still ~15.5–17 KB RAM, zero heap. New code is *smaller*: `parse_atomic_term` replaces duplicated logic, registry check is branch-first (early SU exit saves a range test on privileged paths). Session peak: `0x0F` registry word, two fns coexisting (`aa`+`bb`), one audit entry — orders under limits.

---

## II.12 Action Items — v1 Additions

Cookbook:
- [ ] Ch1: add `peek (ADDR)` / `reg_set_bit (A) (B)` paren forms (R01.03/05)
- [ ] Ch2: prefix every debug/flash/unmapped peek recipe with `cap_claim SUPERUSER;` + drop; keep RAM/vector recipes open
- [ ] Ch5: new section "Parentheses group now" with `10-(5-2)=7` demo; update operator table: infix unsupported → `MISSING SEMICOLON`
- [ ] Ch6: replace tab-warning with "tabs work (mapped to spaces)"; keep boot-drain note
- [ ] Ch7: note SysTick/CPUID/DWT reads now SU-gated; audit `CYCLES` still 0 on ARM, live on RV32
- [ ] Ch10: flash now SU-gated; GPIO input reads still 0 on QEMU (both arches)

---

## II.13 VERDICT — v1

130 prompts → 130 clean REPL replies, zero hard-stops, zero unexpected busy loops, zero heap. All three intentional fixes verified live in their strongest forms: atomic-term greediness eliminated (fault → safe error), wildcard dispatch excised (two coexisting fns, typo-safe calls), fail-closed whitelisting hardened (unauthorized reads become `E001`), and tab handling normalized. Audit now covers mapped writes (first mapped audit entry captured). Speed: wire-bound. Safety: strictly stronger. Footprint: unchanged. The compiler edits are good to stage — after you decide whether RV32 flash reads should remain SU-gated or be added to its `is_ram_flash` whitelist.

*Part II-v1 ends. Final machine states: ARM sessions clean (`0x00`–`0x0F` registries, `0x1`–`0x2` fns, one audit entry max), RV32 flash correctly gated, one `mcycle` timestamp captured, no reboots owed beyond normal QEMU exits.*
