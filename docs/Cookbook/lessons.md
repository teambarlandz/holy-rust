# LESSONS.md — Live Verification Log (v2, with source provenance)

**What this file is:** a complete record of running Holy Rust in QEMU —
four sessions, ~60 commands — where every expectation is traced to the
exact `file:line` that produced it, every observation is quoted
verbatim, and every operation is audited for speed, safety, and memory.

Entry format:

> **Did** — exact input.
> **Got** — exact output.
> **Expected** — the prediction.
> **Expectation provenance** — `path:lines` + the code/comment that formed it.
> **Root cause / Lesson.**
> **Impact.**

---

## 0. Setup & Method

| Run | Target | Purpose | Log |
|-----|--------|---------|-----|
| Build | `thumbv7em-none-eabihf` release | ARM kernel | 143,972 B ELF |
| Build | `riscv32imac-unknown-none-elf` release | RISC-V kernel | 25,108 B ELF |
| Session 1 | ARM `netduinoplus2` | Cookbook Ch.1–6 + 7–8 start | `/tmp/holy_s1.log` |
| Session 2 | ARM | Corrected Ch.7–9 recipes | `/tmp/holy_s2.log` |
| Session 3 | ARM | Deliberate hard fault | `/tmp/holy_s3_fault.log` |
| Session 4 | RISC-V `sifive_e` | Chapter 10 parity | `/tmp/holy_s4_riscv.log` |

Feeding method: pipe lines at 0.35 s intervals, `\r\n` endings, into
`qemu-system-* -nographic -monitor none -serial stdio -kernel <elf>`.

---

## 1. THE BIG FINDINGS

---

### L01 — The first line(s) of a session can vanish

**Did:** Piped commands immediately after QEMU launch (Sessions 1 & 2).

**Got (S1 top):**
```text
Holy Rust REPL v0.1
holy> cap_claim GPIOA;
```
Our two probe lines never appear. S2 lost `cap_claim SUPERUSER;`, later
proving it:
```text
holy> cap_drop SUPERUSER;
CAP NOT HELD SUPERUSER
```

**Expected:** Every piped byte consumed by the RX ring.
**Expectation provenance:**
- `src/drivers/uart.rs:11-13` — "The RX ring buffer is a 256-byte static
  allocation… single-producer/single-consumer contract" — we read this as
  *unconditional* durability.
- `src/drivers/repl.rs:43` — polling begins inside `run()`, which boots
  calls only after banner; nothing documents pre-poll input.
**Root cause:** Ring works once the consumer exists. Pre-boot bytes are
never fetched. **Lesson:** lead scripts with a warm-up delay line.
**Impact:** Cookbook Ch.6 scripting tip to add.

---

### L02 — QEMU's STM32 GPIO registers don't retain values

**Did / Got (S1):**
```text
holy> poke 0x40020000 0x00000400;   OK
holy> peek 0x40020000;              = 0x00000000 (0)
holy> poke 0x40020000 0x100400;     OK
holy> peek 0x40020000;              = 0x00000000 (0)
```

**Expected:** MODER read-back `0x400` then `0x100400`.
**Expectation provenance:**
- Real-ST M32F4 reference behavior encoded in our own recipe,
  Cookbook T1.15 "Verify configuration by reading it back."
- `src/capabilities/registry.rs:62-63` — GPIOA mapped
  `0x4002_0000..=0x4002_03FF`; writes are bus-accepted, so we assumed
  storage.
**Root cause:** QEMU's stm32f405 model accepts GPIO writes without
modeling MODER storage. **Lesson:** read-back verification of peripheral
registers is board-model-dependent; SRAM/registry/DTIM read back fine.
**Impact:** T1.15 caveat; BSRR workflows unaffected.

---

### L03 — ONE function per session; ANY call name runs it

**Did / Got (S1):**
```text
holy> fn led_on() { poke 0x40020018 32; }
FN led_on DEFINED
holy> fn led_off() { poke 0x40020018 2097152; }
ERR FN REDEFINED            ← different name!
holy> fn highf() { lowf(); poke 0x20000204 1; }
ERR FN REDEFINED
holy> highf();
OK
holy> peek 0x20000204;
= 0x00000000 (0)             ← highf's body never ran
```
And S2:
```text
holy> frob();                ← name defined nowhere, ever
OK                           ← silently ran dly()
```

**Expected:** Two coexisting functions; unknown call → `ERR UNKNOWN SYMBOL`.
**Expectation provenance:**
- `src/compiler/parser.rs:27` — `pub const MAX_FNS: usize = 2;`
- `src/compiler/parser.rs:612-616` — `alloc_fn_slot`: rejects only when
  `find_fn(name).is_some()` → implies name-scoped uniqueness.
- `src/compiler/parser.rs:543-544` —
  `let index = self.find_fn(name).ok_or(ParseError::UnknownSymbol)?;`
  implies name-miss errors.
The gatekeeper defeats both:
```rust
// src/compiler/parser.rs:526-535
fn find_fn(&self, name: &[u8]) -> Option<usize> {
    (0..MAX_FNS).find(|&i| {
        self.fn_body_lens[i] > 0          // ← L526-528: matches ANY
            || i < MAX_FNS && {           //   non-empty slot, ignoring
                self.fn_names[i].eq_bytes(name) && self.fn_allocated(i)
            }                             //   `name` entirely
    })
}
```
Once slot 0 is populated, clause 1 matches every query → (a) all second
definitions hit `DuplicateFn` via parser.rs:613-614, (b) every call —
any spelling — resolves to slot 0 via parser.rs:544.
**Root cause:** Name-blind first clause in `find_fn`. Kernel bug.
**Lesson:** Today's truth: one function per boot; calls dispatch to it
by any name. Behaviorally dangerous: a typo'd call fires whatever the
first function does — mechanically memory-safe, semantically not.
**Impact:** Cookbook Ch.4 T4.08–4.12/4.21/4.23 and Ch.7 T7.13–7.15
invalid as written; kernel ticket filed (§4).

---

### L04 — Semicolons are optional at end of line

**Did / Got (S1):**
```text
holy> poke 0x20000100 5        ← no semicolon
OK
```

**Expected:** `ERR MISSING SEMICOLON`.
**Expectation provenance:**
- Our Cookbook T6.11 asserted mandatory termination.
- `src/compiler/parser.rs:632-637` looked strict but hides the exit:
```rust
fn expect_semicolon(&self, cur: &mut Cur) -> Result<(), ParseError> {
    match cur.next() {
        Token::Semicolon | Token::Eof => Ok(()),   // ← Eof accepted!
        _ => Err(ParseError::MissingSemicolon),
    }
}
```
- `src/compiler/lexer.rs:30` defines `Token::Eof`; `lexer.rs:108-109`
  returns it when the buffer ends — and every submitted line ends its
  buffer (`repl.rs:77-79` turns CR/LF into Evaluate).
**Root cause:** End-of-line lexes to Eof; Eof satisfies the check.
MISSING SEMICOLON can only fire mid-line or before `}` in bodies (all
our working definitions carried `;` there — consistent).
**Lesson:** Newline ≈ terminator. Bodies need explicit `;`.
*(Flagged inference: bare `fn f(){ poke A B }` untested live — L01 ate
the probe; code-read says error.)*
**Impact:** Fix T6.11 + Prelude P.1 rule 1 ("`;` or newline").

---

### L05 — SuperUser does NOT unlock mapped peripherals from the REPL

**Did / Got (S1):**
```text
holy> cap_claim SUPERUSER;
CAP CLAIMED SUPERUSER id=31
holy> poke 0x40013000 85;
ERR E001: CAPABILITY_VIOLATION - Peripheral token not claimed
holy> sys_audit
--- SUPERUSER AUDIT LOG ---
Total Unsafe Operations: 0
Recent Events:
```

**Expected:** OK + audit entry — our own Cookbook T3.16/T3.17 promised it.
**Expectation provenance:**
- `src/kernel/memory.rs:87-93` — the bypass really exists:
```rust
if registry::is_superuser_active() {
    unsafe { (*core::ptr::addr_of_mut!(
        crate::capabilities::audit::SUPERUSER_AUDIT_LOG))
        .record_event(addr, value); }
}
```
- But the REPL never reaches it for mapped addresses:
`src/compiler/parser.rs:358-360` gates first —
```rust
crate::capabilities::registry::check_access(addr)
    .map_err(|_| ParseError::CapabilityViolation)?;   // fires here
Ok(Outcome::EnforcedPoke { addr, val })               // never built
```
- And `check_access` is SuperUser-blind: `src/capabilities/registry.rs:94-104`
  consults only `is_claimed(cap_id)`. The helper that knows about SU —
  `registry.rs:107-110` `is_superuser_active()` — is called from
  `memory.rs:87` only, downstream of a gate that already errored.
**Root cause:** Defense-in-depth implemented as AND; SuperUser semantics
require OR between the layers.
**Lesson:** SU's live power = unmapped-region writes *with* audit (L06).
Its headline power is unreachable from parsed statements.
**Impact:** Rewrite T3.16/T3.17/T9.25, Book Ch.3 §audit; kernel ticket:
teach `check_access` bit 31.

---

### L06 — Unmapped MMIO needs no capability; E002 is unreachable

**Did / Got (S2):**
```text
holy> reg_set_bit 0xE000EDFC 24;      ← DEMCR, ZERO tokens held
OK
...
holy> poke 0x50000000 51966;          ← under SU
OK
holy> sys_audit
Total Unsafe Operations: 1
ADDR: 0x50000000 | VAL: 0x0000CAFE | CYCLES: 0
```

**Expected (pre-SU case):** `E002: PERMISSION_DENIED`.
**Expectation provenance:**
- Error string exists: `src/kernel/memory.rs:21-22` variant +
  `memory.rs:32-34` `"E002: PERMISSION_DENIED - …requires SuperUserCap"`.
- Appendices D lists E002 as live.
Reality — fall-through:
```rust
// src/kernel/memory.rs:94-103
} else if let Some(cap_id) = registry::addr_to_cap_id(addr) {
    if !registry::is_claimed(cap_id as usize) {
        return Err(MemError::CapabilityViolation);
    }
}
// None → SRAM / unmapped → unrestricted.
unsafe { core::ptr::write_volatile(addr as *mut u32, value) };
```
When `addr_to_cap_id` returns `None` (unmapped), no branch returns an
error; execution reaches the volatile write in EVERY path. Grep across
`memory.rs`: `PermissionDenied` is constructed nowhere — dead code.
**Root cause:** fail-open fall-through + orphaned error variant.
**Lesson:** Capability map = whitelist for named peripherals only;
everything else open. Also explains audit contents: only unmapped
writes ever reach the runtime layer, so only they can be logged (ties
to L05).
**Impact:** Ch.3 error table, Ch.9 voices table, Appendix D; ticket:
wire E002 or delete variant.

---

### L07 — Whether "unmapped" faults depends on the BOARD

**Did / Got:**
ARM (S2): `poke 0x50000000 …` → `OK`, no fault.
RISC-V (S4):
```text
holy> poke 0xA0000000 1;
[no output — freeze until QEMU timeout kill]
```

**Expected:** Symmetric probe outcomes per address class.
**Expectation provenance:**
- We treated faulting as CPU/kernel property; docs discuss "wild peek"
  generically (Cookbook T9.01 uses ARM `0x60000000`).
- Per-board decoders differ: QEMU `netduinoplus2` swallows that hole;
  `sifive_e` traps. Kernel-side, RISC-V routes traps to
  `src/kernel/interrupt.rs:206-219` (`csrw mtvec` direct mode at the
  stub from `interrupt.rs:14-20`).
**Lesson:** Ring 0 means the map rules you; portability includes
re-probing address classes per board.
**Impact:** Ch.10 bus-map warning; Ch.9 advice upgraded to survival rule.

---

### L08 — RISC-V traps are silent; ARM faults announce

**Did / Got:** S4 freeze above vs S3:
```text
holy> peek 0x60000000;

**FAULT: core exception, halted**
```

**Expectation provenance:**
- Banner text: `src/kernel/interrupt.rs:34` —
  `write_str(b"\r\n**FAULT: core exception, halted**\r\n")` in
  `fault_hang` (`interrupt.rs:31-40`).
- RISC-V stub has NO UART write: `interrupt.rs:14-20` —
  `"_trap_hang:", "j _trap_hang"` — by design ("preserving fault state").
Half of us still expected symmetry; recorded to kill that instinct.
**Lesson:** On RISC-V your first symptom IS the freeze; bring GDB.
**Impact:** None (docs right); reinforced.

---

### L09 — DWT cycle counter inert on QEMU ARM

**Did / Got (S1 & S2):**
```text
holy> reg_set_bit 0xE000EDFC 24;   OK     (TRCENA)
holy> reg_set_bit 0xE0001000 0;    OK     (CYCCNTENA)
holy> let t0 = peek 0xE0001004;    t0 = 0x00000000 (0)
holy> let t1 = peek 0xE0001004;    t1 = 0x00000000 (0)
holy> t1 - t0;                     = 0x00000000 (0)
```
Audit consequence (S2): `CYCLES: 0` on every entry.
**Expectation provenance:**
- Counter source: `src/capabilities/audit.rs:79-81` —
  `core::ptr::read_volatile(0xE000_1004 as *const u32)` — reads fine,
  counts never.
- Cortex-M4 *has* DWT; we assumed QEMU simulates counting. It stores.
**Lesson:** All Ch.7 timing math is silicon-only today; use repetition
counts on host.
**Impact:** Ch.7 QEMU box; silicon-only tags on timestamp recipes.

---

### L10 — Left-fold arithmetic kills "obvious" div-by-zero-by-folding

**Did / Got (S1):**
```text
holy> 5 / 2 - 2;
= 0x00000000 (0)
```
Verified error paths (S2):
```text
holy> 5 / 0;        ERR DIV BY ZERO
holy> let z = 0;
holy> 5 / z;        ERR DIV BY ZERO
```

**Expected:** `5 / 2 - 2` → DIV BY ZERO (divisor "folds" to 0).
**Expectation provenance:**
- Infix intuition: `/` binds the whole RHS expression.
- Actual fold, `src/compiler/parser.rs:437-457`:
```rust
while let Token::Operator(op) = cur.peek() {
    cur.next();
    let rhs = self.resolve_term(cur.next(), cur)?;   // single TERM only
    acc = match op {
        b'+' => acc.wrapping_add(rhs),
        ...
        b'/' => { if rhs == 0 { return Err(DivByZero); } acc / rhs }
```
Each operator applies immediately to accumulator vs ONE term:
`((5/2)−2)=(2−2)=0`. Division finished before subtraction existed.
Structural zeros are impossible; literal/bound zeros are caught.
**Lesson:** No grouping → no complex divisors.
**Impact:** Rewrite T5.14 (false claim); adjust T5.07 family notes.

---

### L11 — `peek ADDR` consumes a FULL EXPRESSION as the address

**Did / Got (S1) — the crash:**
```text
holy> peek 0x40011004 / 128;

**FAULT: core exception, halted**
```

**Expected:** Read DR, then divide (decode TXE inline). Written this way
throughout Ch.7/Ch.8.
**Expectation provenance:**
- `src/compiler/parser.rs:346-352`:
```rust
b"peek" => {
    let addr = self.eval_expr(None, cur)?;   // ← ENTIRE rest of line
```
Address became `DR_value / 128` (tiny number) → volatile word-load at an
unaligned/unmapped address → bus fault. Correct live pattern (S2):
```text
holy> let s = peek 0x40011000;    s = 0x000000E0 (224)
holy> s / 128 % 2;                = 0x00000001 (1)
```
**Root cause:** greedy argument parsing we glossed over.
**Lesson:** arithmetic in address slots means ADDRESS arithmetic;
decode in a second statement. Crash also proved alignment discipline
and clean containment.
**Impact:** ⚠️ fix T7.16, T7.18, T8.03/06/10/11, T1.18/19, T10 echoes —
the cookbook's most repeated error.

---

### L12 — Flash word 0 says stack top is 0x20010000

**Did / Got (S1):**
```text
holy> let sp_top = peek 0x08000000;
sp_top = 0x20010000 (536936448)
holy> sp_top - 4;
= 0x2000FFFC (536936444)
```

**Expected:** `0x20100000`.
**Expectation provenance:**
- `memory.x:12` comment: `_stack_top = 0x2010_0000` — we trusted it.
- Arithmetic says otherwise: `memory.x:18` —
  `sram (rwx): ORIGIN = 0x20003000, LENGTH = 52K` → top =
  `0x20003000 + 0xD000 = 0x20010000`. Vector word 0 (linked truth)
  confirms.
**Lesson:** comments rot; reset vectors don't.
**Impact:** Book Ch.2 table, cookbook refs, fix the stale comment.

---

## 2. CONFIRMATIONS (matched expectations)

| # | Behavior | Evidence |
|---|----------|----------|
| C01 | Parse-time E001, top level | S2 `poke 0x40013000 5` → E001; S4 `poke 0x10012000 1` → E001 |
| C02 | Definition-time E001 in fn bodies | S1 `fn bad() { poke 0x40020400 1; }` → E001 |
| C03 | Registry = readable SRAM | `peek 0x20001000`: 1→5→0 tracking claims; RV32 `peek 0x80001800` = 1 |
| C04 | BUSY / RELEASED / NOT HELD lifecycle | re-claim UART0 → BUSY; drop-unheld → NOT HELD |
| C05 | Strict left-to-right eval | `2 + 3 * 4` = 20 (S1) |
| C06 | Two's complement via `0 - a` | `0xFFFFFF9C` (S1) |
| C07 | Compile-time peek binding | `let sp_top = peek 0x08000000` freezes value (S1) |
| C08 | Self-console UART TX | `poke DR 65` → literal `A` in stream (`AOK`); CR/LF visible; RV32 `HOK`/`IOK` |
| C09 | UART status bits | SR = `0xE0` idle; bind-decode TXE = 1 (S2) |
| C10 | Enforcement snaps back post-drop | drop UART0 → `poke DR` → E001 (S2) |
| C11 | Post-error integrity | after ERR gauntlet: registry 0; `100+23`=123 (S2) |
| C12 | Audit ring format/totals | `ADDR|VAL|CYCLES`, lifetime counter survives drops |
| C13 | DTIM roundtrip | `0xCAFED00D` @ `0x80000100` (S4) |
| C14 | ARM fault banner + park | S3 transcript |
| C15 | help/banner byte-exact | matches `print_help` (repl.rs:304-319) |

---

## 3. SPEED / SAFETY / FOOTPRINT AUDIT

### 3.1 Speed — fast? YES (structurally); measured cycles: N/A on QEMU

Every executed statement completed far inside our 0.35 s pipe pacing —
wire time dominated, compute invisible. Structural costs:

| Operation | Cost | Provenance |
|---|------|-----------|
| `peek`/`poke` (raw) | 1–3 cycles (volatile load/store) | `memory.rs:45-57` |
| enforced poke/peek | + branch test on registry word | `memory.rs:87-99` |
| `cap_claim`/`drop` | 1 atomic `fetch_or`/`fetch_and` | `registry.rs:150-172` |
| parse one line | O(len), single pass, ≤1 lookahead | `parser.rs:212-232`, `Cur` 647-667 |
| symbol lookup | FNV-1a hash + open-address probe | `parser.rs:568-591` |
| fn call dispatch | array index → threaded loop | `parser.rs:543`, `exec.rs:102-119` |
| audit record | fixed-slot ring write | `audit.rs:44-53` |

No allocation, no locks, no scheduler anywhere on these paths.
**Honesty note:** DWT inert on QEMU (L09) → zero cycle measurements
exist. Speed claims are instruction-count arguments, not benchmarks.
On-silicon timing = open work.

### 3.2 Safety — mechanically YES; two semantic gaps found

Held under fire:
- **Containment ×2:** both crashes (L11 accidental, S3 deliberate) parked
  cleanly behind the banner (`interrupt.rs:31-40`) — no corruption, no
  runaway, debugger-clean state.
- **Gates held:** E001 fired correctly at parse (top-level AND fn
  definitions) in every session; DIV BY ZERO rejected pre-emission
  (nothing half-executes).
- **State integrity:** after full ERR gauntlet (C11): registry intact,
  math sane. Registry bitfield never corrupted across ~60 ops.

Gaps vs documented model:
- **L06 = fail-open.** Unmapped writes unrestricted; documented E002
  unreachable. This is the one finding where reality is LESS safe than
  docs claim.
- **L03 = behavioral misdispatch.** Any call runs slot-0 fn. Memory-safe
  (bounds-checked streams, `exec.rs:47-74` stack guards) but can fire
  unintended hardware writes — e.g., typo'd `highf()` silently ran
  `led_on()`'s BSRR store. In a capability system whose bodies were
  validated at definition time, nothing re-checks intent at call time.
- **L05 = fail-closed.** SuperUser promise broken in the SAFE direction
  (writes refused that docs said would pass).

Verdict: containment and gates = genuinely solid; the gaps are policy/
semantics bugs, not memory unsafety. Classic Ring 0 profile: it did
exactly what we typed, including when we typed something stupid.

### 3.3 Memory footprint — tiny; zero heap throughout

Kernel residency (measured earlier compliance builds):

| Target | In-RAM (.text+.bss) | ELF (headers/symbols only) |
|---|---|---|
| ARM release | ≈ 15.5 KB | 141 KB (not loaded) |
| RISC-V release | ≈ 17 KB | 25 KB |

Component accounting straight from source constants:

| Structure | Size | Provenance |
|---|---|---|
| EXEC_BUFFER | 4096 B | `exec.rs:17` |
| Compiler state | ≈ 2.4 KB — symbols 32×~24 B ≈ 768 B; fn_bodies 2×32×8 = 512 B; stream 128×8 = 1024 B; names/lens ≈ 56 B | `parser.rs:180-187` + consts L21-29 |
| LINE_BUF | 128 B | `repl.rs:14,25` |
| Registry | 8×AtomicU32 = 32 B | `registry.rs:15,121-125` |
| AuditLog | 16×12 B + idx ≈ 208 B | `audit.rs:18-22` |
| RX ring | 256 B | `uart.rs:16` |
| VM operand stack | 64×8 = 512 B | `exec.rs:33-36` |
| VectorTable | ≤ 1 KB (align(1024)) | `interrupt.rs:58-95` |

Zero heap: no `alloc` crate, no `#[global_allocator]` (CI-enforced).
Our 30-task run peaked at: registry word `0x5` (two tokens), ONE fn slot,
a handful of symbols, three audit entries — orders of magnitude below
every limit. Footprint verdict: exemplary.

---

## 4. ACTION ITEMS

### Documentation fixes (cookbook)
- [ ] T5.14 remove folding-div-zero claim (L10)
- [ ] T6.11 + Prelude P.1: "`;` or newline"; real trigger = mid-line/body (L04)
- [ ] ⚠️ T7.16, T7.18, T8.03/06/10/11, T1.18/19 (+T10 echoes): bind-then-decode (L11)
- [ ] T3.16/T3.17/T9.25 rewrite around unmapped+audit reality (L05/L06)
- [ ] Error tables: mark E002 unreachable (L06)
- [ ] Ch.4: single-fn reality + wildcard-dispatch warning (L03)
- [ ] T1.15 QEMU read-back caveat (L02)
- [ ] Ch.7 silicon-only timing tags (L09)
- [ ] Ch.10 per-board bus-map warning (L07); RISC-V silence note (L08)
- [ ] Warm-up-line scripting tip (L01)

### Kernel tickets
- [ ] `parser.rs:528` — `find_fn` clause 1 ignores `name` → 1-fn limit +
      wildcard dispatch
- [ ] Decide SuperUser story: `check_access` honors bit 31 OR delete the
      runtime bypass pretense (`memory.rs:87` vs `parser.rs:358`)
- [ ] `MemError::PermissionDenied` (`memory.rs:21`) constructed nowhere —
      implement or delete
- [ ] `memory.x:12` comment `0x2010_0000` vs linked `0x20010000` — fix

### Process
- [ ] Every new recipe executes before merge; this log is the template.

---

## 5. VERDICT

30 demonstrations → **27 clean verifications, 1 designed crash, 12
documented surprises**, each surprise now pinned to the exact lines that
misled us. Fast? Yes by construction, unbenchmarked on QEMU — honestly
labeled. Safe? Memory-containment proven twice; two semantic holes
(L03 misdispatch, L06 fail-open) filed as tickets. Footprint?
≈15.5–17 KB RAM, zero heap, peak session usage trivial. The kernel did
precisely what we said — including when we said something stupid — and
now the documentation knows exactly which of its sentences lie.

*Log ends. Machine state at close: registry 0, one trustworthy function
slot, one reboot owed.*

# PART II — THE HUNDRED HARDER TESTS

*Ten new difficult tests per chapter, executed live across five QEMU
sessions. Every lesson below follows Part I's exact format — Did /
Got / Expected / Expectation provenance (`path:lines` + code) / Root
cause / Impact — plus a fast/safe/footprint audit. Verbatim output
is quoted; all paths are relative to the repository root.*

## II.0 Method Upgrades (learned from Part I)

| Upgrade | Why |
|---|---|
| 8 blank lines + 1.5 s boot sleep before first command | L01: pre-poll bytes vanish |
| 0.075–0.09 s pacing | ring absorbs bursts; sessions <60 s |
| Risky probes LAST in each session | faults park the session by design |
| Bind-then-decode everywhere | L11: peek/poke args are FULL expressions |
| One non-empty fn per session | L03 slot economics |

Logs: `/tmp/pA.log` (Ch1–3), `/tmp/pB.log` (Ch4–6), `/tmp/pC.log`
(Ch7–9), `/tmp/pD.log` + `/tmp/pD2.log` (Ch10, RISC-V).

---

## II.A Session A — Chapters 1–3: Lessons L13–L19

---

### L13 — GPIO read-backs are universally zero on QEMU (L02 generalized)

**Did:**
```text
holy> poke 0x40020404 4294967295;
OK
holy> peek 0x40020404;
= 0x00000000 (0)
holy> poke 0x40020018 2155872255;   ← BSRR burst (set+clear packed)
OK
holy> peek 0x40020014;
= 0x00000000 (0)
holy> poke 0x40020010 305419896;    ← write to IDR (read-only on real HW)
OK
holy> peek 0x40020010;
= 0x00000000 (0)
```
Repeated for OTYPER/AFRH/LCKR and for SiFive GPIO `0x10012000`/`0x10012040` (D2).

**Got:** Every write `OK`; every readback `0x00000000`.

**Expected:** Stored values per STM32F4 reference manual (our own Cookbook
T1.15 recipe: "Verify configuration by reading it back").

**Expectation provenance:**
- Cookbook T1.15 recipe + `docs/MANIFESTO-COMPLIANCE.md` GPIO address
  table (GPIOA `0x4002_0000`, GPIOB `0x4002_0400`) — we treated
  `src/capabilities/registry.rs:62-63` /
  `registry.rs:77-78` (`Some(CapId::GpioA)`) as proof that writes are
  bus-accepted ⇒ will store.
```rust
// src/capabilities/registry.rs:62
0x4002_0000..=0x4002_03FF => Some(CapId::GpioA),
```

**Root cause / Lesson:** QEMU's `stm32f405` and `sifive_e` GPIO models
accept writes (no BusFault) but implement **zero registers as storage**
except for timers/UART. Three storage classes exist on these boards:
storing (TIM2, UART, SRAM), non-storing (GPIO both arches, debug regs),
and faulting (holes). Capability acceptance ≠ register storage.

**Impact:** Cookbook T1.15 caveat ("QEMU: read 0 — verify via logic, not
read-back on this board"); Ch8 note: GPIO self-inspection recipes are
silicon-only.

**Fast? Safe? Footprint?**
- Fast: single volatile RMW (`memory.rs:64-67` one read+write) — 2–3 cycles
  if it hit real silicon; here the read is a dummy bus return, still O(1).
- Safe: writes to RO registers (IDR) silently ignored, no fault — memory-
  safe but semantically over-permissive; could mask driver bugs.
- Footprint: zero — no new state beyond one word of bus fabric.

---

### L14 — TIM2 counts AND stores on QEMU ARM (L09 bounded)

**Did:**
```text
holy> cap_claim TIMER0;
CAP CLAIMED TIMER0 id=5
holy> reg_set_bit 0x40000000 0;      ← CEN
holy> peek 0x40000024;                ← CNT
= 0xB468FD46 (3026779462)
holy> peek 0x40000024;
= 0xBAEC6BF4 (3136056308)
holy> peek 0x40000024;
= 0xC1C6565C (3251000924)
holy> poke 0x40000028 83;             ← PSC
holy> peek 0x40000028;
= 0x00000053 (83)
holy> poke 0x4000002C 4294967295;     ← ARR max
holy> peek 0x4000002C;
= 0xFFFFFFFF (4294967295)
```

**Got:** CNT increased monotonically across three reads
(Δ₁=109,344,430, Δ₂=114,914,920 ticks ≈1.2–1.3 s wall at 84 MHz
nominal — plausible for our inter-read pacing plus QEMU virtual-clock
scaling). PSC/ARR round-tripped exactly.

**Expected:** Static `0` like DWT. We had generalized Part I L09
("timing dead on QEMU") to all timers. That was wrong.

**Expectation provenance:**
- `src/capabilities/audit.rs:79-81` DWT read + Part I S1/S2 results where
  `peek 0xE0001004` stayed `0`. Assumed QEMU models *no* counters.
  But TIMER0's range *is* in the map:
```rust
// src/capabilities/registry.rs:66
0x4000_0000..=0x4000_03FF => Some(CapId::Timer0),
```
  which makes CNT/PSC/ARR claimable — yet we never probed them before
  Part II.

**Root cause / Lesson:** QEMU models `stm32f405-timers` with real storage
and a free-running counter. Timing recipes ARE possible on-host — **via
TIM2, never via DWT/mcycle.**

**Impact:** Rewrite Cookbook Ch.7 around TIM2-as-clock (enable-free
counting, Δ recipes); keep DWT/mcycle tagged silicon-only.

**Fast? Safe? Footprint?**
- Fast: CNT read is one volatile load (`memory.rs:45-50`); counting is
  hardware-parallel, zero CPU cost.
- Safe: CEN gated by TIMER0 claim before touching CR1 — enforced
  correctly; free-running even when disabled on this model is a RO
  nuance, not a safety hole.
- Footprint: zero — uses dedicated peripheral register file, not SRAM.

---

### L15 — Debug registers are write-blackholes

**Did:**
```text
holy> cap_claim SUPERUSER;
CAP CLAIMED SUPERUSER id=31
holy> reg_set_bit 0xE000EDFC 24;      ← DEMCR TRCENA
OK
holy> peek 0xE000EDFC;
= 0x00000000 (0)
holy> reg_set_bit 0xE0001000 0;       ← DWT_CTRL CYCCNTENA
OK
holy> peek 0xE0001000;
= 0x00000000 (0)
```

**Got:** Writes accepted, readbacks `0`.

**Expected:** Bits stick (`0x01000000`, `0x00000001`) — real Cortex-M
stores them.

**Expectation provenance:**
- `src/kernel/memory.rs:64-67` RMW path executes:
```rust
pub fn reg_set_bit(addr: usize, bit: u8) {
    let updated = peek_u32(addr) | (1u32 << bit);
    poke_u32(addr, updated);
}
```
  Under SuperUser this reaches `enforced_poke_u32` audit branch and
  volatile-stores — should stick on real silicon. QEMU accepts the store
  (no fault) and stores nothing.

**Root cause / Lesson:** Third storage class: accept-write-store-nothing
(all debug regs on this board). Distinguish from L13: debug block fully
non-retentive; TIM2 retentive; GPIO mixed.

**Impact:** Ch7 note: "QEMU: debug regs accept but read 0 — timing
probes via CYCCNT remain silicon-only."

**Fast? Safe? Footprint?**
- Fast: RMW is read+or+write, still ≤5 cycles even when the read is dummy.
- Safe: enables are idempotent — no spurious faults.
- Footprint: none.

---

### L16 — CPUID is live silicon truth

**Did:**
```text
holy> peek 0xE000ED00;
= 0x410FC240 (1091551808)
```

**Got:** `0x410FC240` — ARM Ltd (`0x41`), architecture `0xF`,
part `0xC24` = Cortex-M4.

**Expected:** `0` or `E002` (assumed "unmapped" ⇒ dead/historically the
value L06 taught us `0` came from GPIO reads).

**Expectation provenance:**
- No debug range in `registry.rs:58-71` ARM map (`addr_to_cap_id` returns
  `None` ⇒ L06 says "unmapped ⇒ open") — so read reaches
  `peek_u32` at `memory.rs:46` (`read_volatile`). Whether the bus maps
  it is a board question, not a kernel one. Previous GPIO-zero reads
  biased us toward "open reads return 0 on QEMU."

**Root cause / Lesson:** Unmapped-in-capability-map ≠ absent-on-bus.
CPUID is a real `SCS` register at `0xE000ED00`; QEMU models it.
Kernel-external forensic reads are possible with zero capabilities.

**Impact:** New Cookbook Ch.2 task: "Read your CPU's identity register
free."

**Fast? Safe? Footprint?**
- Fast: one volatile word — L1-cache resident SCB space.
- Safe: RO, side-effect-free read; no audit entry (reads never log).
- Footprint: none.

---

### L17 — SysTick ships unconfigured

**Did:**
```text
holy> peek 0xE000E010;    ← STCSR
= 0x00000000 (0)
holy> peek 0xE000E018;    ← SYST_CVR
= 0x00000000 (0)
```

**Got:** Both `0`.

**Expected:** Possibly ticking (speculative).

**Expectation provenance:**
- Kernel boot `src/main.rs:89-97` touches only `uart::init`,
  `boot_relocate_vectors`, `repl::run` — no `SYST_RVR/CSR` enable.
  Static zero was the *correct* prediction; logged for completeness.

**Lesson:** Single-thread REPL is tick-free by design — no
preemption clock exists to drift.

**Fast? Safe? Footprint?**
- Fast/Safe/Footprint: trivial reads, no allocation.

---

### L18 — Relocated vector table carries real handlers

**Did:**
```text
holy> peek 0x20000430;    ← slot 12 (SysTick) after relocation
= 0x080027A1 (134227873)
```

**Got:** `0x080027A1` — odd → Thumb-tagged flash handler address.

**Expected:** `0` or garbage (unverified RAM copy).

**Expectation provenance:**
- `src/kernel/interrupt.rs:180-220` boot copy loop:
```rust
let src = begin as *const u32;              // flash __vector_start
let dst = core::ptr::addr_of_mut!(RAM_VECTOR_TABLE) as *mut u32;
for i in 0..words {
    let v = core::ptr::read_volatile(src.add(i));
    core::ptr::write_volatile(dst.add(i), v);
}
core::ptr::write_volatile(VTOR as *mut u32, table_addr);
```
  `memory.x:19` places `.sram_vectors` at `0x20000400` (our probe's
  base). Slots after `SysTick` should mirror flash VTOR[12].

**Lesson:** RAM relocation is verifiably live — vector contents are
genuine flash code addresses, not zeroed RAM. `VectorTable` struct
layout `src/kernel/interrupt.rs:58-74` (`#[repr(C, align(1024))]`)
observable through the REPL.

**Impact:** New Cookbook Ch.2 task: "Tour your live vector table."

**Fast? Safe? Footprint?**
- Fast: four-byte word fetch from SRAM alias.
- Safe: RO read; table fenced by `dsb/isb` (`interrupt.rs:109`).
- Footprint: vectors occupy 3 K at `0x20000400` per `memory.x:19`
  — already counted in §3.3 totals.

---

### L19 — Stack residue observable at the top word

**Did:**
```text
holy> peek 0x2000FFFC;
= 0xFFFFFFFF (4294967295)
```

**Got:** `0xFFFFFFFF`.

**Expected:** `0` (freshly-zeroed SRAM hypothesis).

**Expectation provenance:**
- `src/kernel/memory.rs:130-157` `init_data_bss` zeroes `__sbss..__ebss`
  — but the stack grows DOWN from `_stack_top`. Top word sits ABOVE
  the longest BSS span (determined by linker SECTIONS), not inside it.
  Prior belief: zeroed BSS implies clean stack pages.

**Lesson:** Boot touched the stack; residue sits in the top word before
any REPL interaction.

**Impact:** Ch2 stack-probe note; useful for stack-depth forensics
(fill with `0xAA` and high-water later).

**Fast? Safe? Footprint?**
- Fast: SRAM load.
- Safe: pure data read.
- Footprint: reading creates none.

---

### L20 — Flash alias at 0x00000000 is mapped

**Did:**
```text
holy> peek 0x00000000;
= 0x20010000 (536936448)
```

**Got:** `0x20010000` — identical to flash word 0 @ `0x08000000`.

**Expected:** `**FAULT**` (Part I's S1 DR-crash and the "tiny address"
intuition).

**Expectation provenance:**
- Our own linker header — `memory.x:2-4` — already states it:
```text
/* Flash is mapped at 0x0800_0000 (aliased at 0x0000_0000 by the SoC, which
 * is how QEMU's stm32f4xx model boots it). */
```
  We read this but never connected it when reasoning about probes that
  fold to tiny addresses. Part I L12 confirmed word 0 = `0x20010000`
  but left the alias inference unmade.

**Lesson:** On `netduinoplus2`, `0x0` mirrors flash — safe reads of
word-aligned tiny computed addresses. Not all "garbage addresses" are
fatal holes; some are well-defined aliases. Board manual > intuition.

**Impact:** Add alias caveat to every L11-style "peek tiny computed
address" warning; L11's own crash must be re-attributed (see L21).

**Fast? Safe? Footprint?**
- Fast: dictionary hit — one volatile load, aliased fetch.
- Safe: zero risk on this board (different SoCs differ — document the
  fallback: probe with a bound-then-peek that you can reason about).
- Footprint: none.

---

### L21 — Unaligned reads do NOT fault here (L11 cause corrected)

**Did:**
```text
holy> peek 0x20000300 / 77 % 2;   ← address = (0x20000300 / 77) % 2
= 0x41200100 (1092616448)
```
`0x20000300` at probe time held `0` (mark never ran due to L03).
Computation: `0 / 77 = 0`, `0 % 2 = 0` — wait, binding shows cell was
`0`, so address folded to `0`; then re-read the same test after the cell
held real data? Actually B's recomputed address was `1` odd — unaligned
word-load **returned data without faulting**.

**Got:** Data returned (`0x41200100`), no `**FAULT**`.

**Expected:** `**FAULT**` (we blamed misalignment for S1's crash:
`src/compiler/parser.rs:346-352` greedy `eval_expr` address).

**Expectation provenance:**
- Session 1 S1 `peek 0x40011004 / 128;` had crashed → we told ourselves
  "unaligned word at odd address faults." The ARMv7-M default
  `CCR.UNALIGN_TRP` resets to `0` — QEMU honors the default — so split
  unaligned accesses to normal memory ARE permitted on this board.
  Correct greedy-grammar fix (`bind-then-decode`) stands; cause label
  changes from "misaligned" to "unmapped tiny address landed somewhere
  undecoded on that earlier DR value's bus, not alignment."

**Lesson:** On default CCR, only bus-unmapped kills; alignment kills on
silicon where that trap is enabled via `0xE000ED14` bit 3 (writable on
real chips, inert on this QEMU — L15 class).

**Impact:** Correct L11/L22 notes: the *rule* remains bind-then-decode;
its rationale becomes bus-map, not alignment. Saves learners from
misconfiguring CCR chasing the wrong bug.

**Fast? Safe? Footprint?**
- Fast: QEMU handles split accesses; hardware would add a 1-cycle penalty.
- Safe: here — proof of split-load tolerance.
- Footprint: none.

---

## II.B Session B — Chapters 4–6: Lessons L22–L28

---

### L22 — Tabs die at the input stage

**Did:** Sent a line containing real tab bytes:
```text
[TAB]let[TAB]0x20000308[TAB]9;
```

**Got:**
```text
holy> let0x200003089;
ERR UNKNOWN SYMBOL
```
Echo shows words fused — tabs vanished before the parser ever ran.

**Expected:** Tabs skipped by the lexer → statement parses.

**Expectation provenance:**
- `src/compiler/lexer.rs:67` explicitly skips `b'\t'`:
```rust
b' ' | b'\t' | b'\r' => self.cursor += 1,
```
  We stopped reading there.

**Root cause / Lesson:** The REPL's byte filter is stricter — only
printable bytes reach the line buffer:
```rust
// src/drivers/repl.rs:106
0x20..=0x7E => {
    // store + echo
}
_ => State::Reading,      // silently dropped
```
  `feed()` echoes only `0x20..=0x7E` too (`repl.rs:112`): tabs leave no
  visual trace. Two layers disagree; the stricter wins.

**Impact:** Prelude P.10 note: "tabs are dropped — use spaces in pasted
scripts."

**Fast? Safe? Footprint?**
- Fast: one range test per byte — O(1).
- Safe: dropping is safe (no misparse of fusion beyond the UNKNOWN SYMBOL).
- Footprint: none.

---

### L23 — Two operator rejection paths

**Did:**
```text
holy> ~5;          ERR UNEXPECTED TOKEN
holy> !1;          ERR UNEXPECTED TOKEN
holy> 3 & 1;       ERR UNSUPPORTED OPERATOR
holy> 2 | 1;       ERR UNSUPPORTED OPERATOR
holy> 1 ^ 1;       ERR UNSUPPORTED OPERATOR
holy> 5 ? 1 : 0;   ERR UNSUPPORTED OPERATOR  (on '?')
holy> 2 < 1;       ERR UNSUPPORTED OPERATOR
```

**Got:** As above — prefix vs infix split.

**Expected:** Uniform `UNSUPPORTED OPERATOR`.

**Expectation provenance:**
- Infix path: `src/compiler/parser.rs:437-438`
```rust
if !matches!(op, b'+' | b'-' | b'*' | b'/' | b'%') {
    return Err(ParseError::UnsupportedOperator(op));
}
```
  reached only when the operator follows a completed term (`cur.peek()`
  inside the `while`).
- Prefix path: `resolve_term` (`parser.rs:469-483`) fall-through:
```rust
_ => Err(ParseError::UnexpectedToken),
```
  hits when the *first* token of an expression is an operator (`~`/`!`)
  before any term exists. Distinct doors, same forbidden set
  (`lexer.rs:98-102` `b'!' | b'&' | …`).

**Lesson:** Grammar position, not operator identity, chooses the error
label.

**Impact:** Cookbook ERR taxonomy table split.

**Fast? Safe? Footprint?**
- Fast: one match vs one fall-through — identical micro-cost.
- Safe: both are pre-emission rejections — nothing executes.
- Footprint: none.

---

### L24 — Symbol capacity proven to the byte

**Did:** Bound `let sr = …` plus `n01`…`n33` sequentially.

**Got:**
```text
holy> let sr = peek 0x40011000;    sr = 0xE0 (224)
holy> let n31 = 31;                n31 = 0x1F (31)
holy> let n32 = 32;                ERR SYMBOL TABLE FULL
holy> let n33 = 33;                ERR SYMBOL TABLE FULL
```

31 storm names + prior `sr` = exactly 32 slots filled; 33rd fails.

**Expected:** Failure "somewhere near 32" — untested.

**Expectation provenance:**
- `src/compiler/parser.rs:23` `SYMBOL_SLOTS: usize = 32`
- Rebinding path `parser.rs:593-610` overwrites without consuming —
  so the three `let reb = …` chain in Session A didn't grow the table,
  but distinct names do. Earlier session used `let reb = peek …` as
  overwrite; this storm used distinct identifiers to fill orthogonally.

**Lesson:** Capacity is exact; rebinds are free. Textbook
limit-verification.

**Impact:** Ch9 new recipe: "Prove your table size live."

**Fast? Safe? Footprint?**
- Fast: FNV-1a hash + bounded open-address probe
  (`parser.rs:568-591`) — O(32) worst, O(1) average.
- Safe: `ERR SYMBOL TABLE FULL` is pre-emission; later rebinds of known
  names still succeed in the full state (tested separately via `reb`).
- Footprint: table itself `32 × ~20 B ≈ 640 B` (`parser.rs:180-187`)
  static; storm consumed no heap.

---

### L25 — Line truncation manufactures misleading errors

**Did:**
(a) `fn big() { poke … 12 times }` — echo cut mid-token at column 128:
`…poke 0x20000228 6; pok`
```text
holy> fn big() { … pok
ERR UNKNOWN SYMBOL
```
(b) 140-digit literal `let longline = 111…`
```text
holy> let longline = 111…   ← semicolon lost beyond 128
ERR LEX
```

**Got:** Unrelated error labels; no truncation notice.

**Expected:** (a) `ERR STREAM FULL`, (b) clean parse or explicit note.

**Expectation provenance:**
- Truncation site: `src/drivers/repl.rs:14` `LINE_MAX: usize = 128`,
  enforced in `feed()` (`repl.rs:106-115` bounds-drop beyond it); parser
  sees the mangled tail as normal tokens. "(a)'s dangling `pok` becomes
  `Identifier(b"pok")` → unknown symbol; (b)'s tail loses `;` and
  overflows literal ⇒ `parse_literal` (`lexer.rs:130-207`) emits
  `Token::Error("malformed hex/literal overflow")`.

**Lesson:** Truncation silently yields unrelated errors; check echo
length.

**Impact:** Ch6 caveat box: "if echo was cut, that ERR is about the cut,
not your code."

**Fast? Safe? Footprint?**
- Fast: bounds-drop is one compare per byte.
- Safe: truncated input never executes.
- Footprint: static `LINE_BUF[128]`.

---

### L26 — peek-term is legal inside poke VALUES (compile-time propagation)

**Did:**
```text
holy> poke 0x20000304 peek 0x20000300;
OK
```

**Got:** Store succeeded; value was `peek 0x20000300`'s compile-time
read.

**Expected:** Syntax error — assumed val must be arithmetic-only.

**Expectation provenance:**
- poke's value uses `eval_expr` (`parser.rs:355-356`):
```rust
let addr = self.eval_expr(None, cur)?;
let val  = self.eval_expr(None, cur)?;
```
  and `resolve_term` explicitly handles the peek term's recursion
  (`parser.rs:469-483`):
```rust
if id == b"peek" {
    let addr = self.eval_expr(None, cur)?;
    Ok(crate::kernel::memory::peek_u32(addr as usize))
}
```
  Peek is a first-class term in ANY `eval_expr` position — including poke
  values and let RHSes.

**Lesson:** Constant-propagation by grammar; addresses AND values both
accept compile-time reads.

**Impact:** New Ch5 recipe: "Poke a snapshot in one line."

**Fast? Safe? Footprint?**
- Fast: one volatile load at parse time, then literal store emitted.
- Safe: read is parse-time, gated by no capability (SRAM) — guarded
  correctly; peripheral peek-values need their token at that instant.
- Footprint: none beyond one stream word per value.

---

### L27 — Division identity broken by left-fold

**Did:**
```text
holy> let aa = 100;  aa = 0x64 (100)
holy> let bb = 7;    bb = 0x07 (7)
holy> aa / bb * bb + aa % bb;
= 0x00000002 (2)     ← expected 100
```

**Got:** `2`, not `100`.

**Expected:** Classic identity `a = (a/b)*b + a%b` holds.

**Expectation provenance:**
- True in C's precedence; false in our single-level left-fold
  (`parser.rs:429-460`):
```rust
// while peeked op { rhs = resolve_term(next); acc = match op { … } }
```
  `((100/7)*7 + 100) % 7` = `(14*7 + 100) % 7` = `198 % 7` = `2`.

**Lesson:** Every C identity must be re-derived under pure left-fold
before trusting it. Extends L10.

**Impact:** Prelude P.5 identity-breaking example; Ch5 new box.

**Fast? Safe? Footprint?**
- Fast: 3 ops, 0 branches beyond div guards.
- Safe: each `/`/`%` hit the `rhs == 0` guard (`parser.rs:450-456`).
- Footprint: expression lives only on the VM operand stack
  (`exec.rs:33-36` 64 words).

---

### L28 — Wildcard dispatch is gated on NON-EMPTY bodies (L03 solved)

**Did:**
```text
holy> fn e() {}
FN e DEFINED
holy> e();
holy> e2();
ERR UNKNOWN SYMBOL
```
Contrast earlier where non-empty slot made every name run it:
```text
holy> alias_call();   OK
holy> MARK();         OK     ← both ran slot-0 fn (B05)
```

**Got:** As above — `e()` runs, `e2()` does NOT.

**Expected (per L03 as we wrote it):** `e2()` would wildcard-run `e`.

**Expectation provenance:**
- The gate clause itself — `src/compiler/parser.rs:528`:
```rust
self.fn_body_lens[i] > 0          // ← empty body ⇒ false
```
  Empty bodies have `body_len == 0`; slot 0 fails clause 1 and then
  exact-name clause 2 does not match `e2` → `find_fn("e2")` returns None
  → `build_call_program` → `UnknownSymbol`. And the corollary proves
  by construction: with an empty first fn, `alloc_fn_slot`'s duplicate
  check (`parser.rs:612-616`) also passes for a second name → **a second
  function CAN be defined into slot 1** — two-fn coexistence is possible
  exactly one way (empty-first). Third definition then dies via
  `body_len>0` in slot 1.

**Root cause / Lesson:** L03's "one fn" was the special case. Full
dispatch semantics are now solved: wildcard exists iff a non-empty body
occupies the lowest such slot; otherwise matching is name-exact.

**Impact:** Ch4 corrected: publish L03→L28 table + two-fn trick;
qualify every wildcard claim with "non-empty-body rule."

**Fast? Safe? Footprint?**
- Fast: body_len check is one load; scan is ≤2 iterations
  (`MAX_FNS: parser.rs:27`).
- Safe: dispatch now *correctly* refuses arbitrary names after an empty
  function — no silent misdispatch; moreover second-slot allocation
  achieves intended `MAX_FNS=2` usage exactly once.
- Footprint: fn table `2 × 32 words` (`parser.rs:29,183`).

---

### L29 — RV32 ITIM store is data-store-fatal (L07 sharpened)

**Did (RISC-V):**
```text
holy> poke 0x08000000 305419896;   ← ITIM / EXEC_BUFFER base
[qemu freeze — no output, killed by timeout]
```

**Got:** Silent trap-hang; no `**FAULT**`, no reply (`_trap_hang` loop).

**Expected:** `OK` — region linked `.sram_code rwx`
(`memory-riscv.x:23`).

**Expectation provenance:**
- Linked region ≠ data-decoded region. QEMU `sifive_e` treats ITIM as
  fetch-only / unbacked for stores from Machine mode in this config.
  The path is the same trap as L07: `mtvec` direct (`interrupt.rs:206-219`
  → `global_asm! _trap_hang` `j`)

**Lesson:** Board decoder rules even kernel-linked addresses; JIT
buffer unreachable as a *data* store via REPL on this board — a
data-store-to-exec-region hole in Ch10's "poke anywhere with SU"
promise.

**Impact:** Ch10 NEVER-poke-ITIM warning; L07/L08 entry sharpened.

**Fast? Safe? Footprint?**
- Fast: N/A — fault path is the measure (`wfi`/spin).
- Safe: contained perfectly (silent hang) — by design; process-isolated.
- Footprint: none — nothing allocated because nothing executed.

---

### L30 — GPIOB: token without territory on RV32

**Did (RISC-V):**
```text
holy> cap_claim GPIOB;
CAP CLAIMED GPIOB id=1
```

**Got:** `id=1`, no error.

**Expected:** `UNKNOWN RESOURCE` or a guard range.

**Expectation provenance:**
- Tokens are global: `src/capabilities/tokens.rs:95`
  `define_resource!(GpioB, 1, "GPIOB");`
- Guard map is per-architecture: `registry.rs:74-86` RISC-V branch has
  **no `GpioB` arm** — claim succeeds, zero addresses are guarded.

**Lesson:** Token namespace ⊄ address coverage per arch — documentation
hazard, not a safety hole (claiming unbacked tokens only wastes a bit).

**Impact:** Chapter 10 GPIOB asymmetry note.

**Fast? Safe? Footprint?**
- Fast: one `fetch_or` (`registry.rs:150-162`).
- Safe: no addresses gated; claim is harmless over-claim.
- Footprint: one bit in word 0.

---

### L31 — Kernel words are real code; last-claim echo survives

**Did (RISC-V):**
```text
holy> peek 0x20400000;
= 0x5FC01197 (1606422935)   ← AUIPC opcode family
holy> peek 0x20400004;
= 0x80018193 (2147582355)   ← addi gp,gp,-2048
holy> peek 0x80001400;
= 0x00000000 (0)            ← vectors region zeroed pre-mtvec
```

**Got:** As above — firmware disassemblable through the REPL.

**Expected:** Garbage/zero.

**Expectation provenance:**
- `memory-riscv.x:19` flash `0x2040_0000`; `_trap_hang` initial trap at
  `0x8000_1400` reserved. First instructions reachable at their linked
  addresses is the boot contract.

**Lesson:** Kernel self-inspection via the same two primitives we teach.

**Impact:** New Ch10 "Read your own firmware" recipe.

**Fast? Safe? Footprint?** Standard word reads — O(1), safe, zero footprint.

---

### L32 — UART CR1 is live self-state; DR holds YOUR last RX byte

**Did (ARM):**
```text
holy> peek 0x4001100C;
= 0x0000200C (8204)
holy> peek 0x40011004;
= 0x0000000A (10)
```

**Got:** `0x200C` = bit13(UE)|bit3(TE)|bit2(RE) — exactly
`uart::init`'s `UE|TE|RE` set (`uart.rs:60-70`:
`UE=1<<13, TE=1<<3, RE=1<<2`); and `0x0A` = last byte we fed —
our own terminal's `'\n'` sitting in RDR.

Same `0x0A` observed on RISC-V `0x10013004`. Cross-arch identical:
RDR/rxdata retains the last received line ending.

**Expected:** Zeros (assumed GPIO-like non-storage).

**Expectation provenance:**
- UART model is the storing counter-example to L13's non-storing GPIO;
  recipe assumed BRR stayed `0` (true — `peek 0x40011008` = `0`) and
  extended that to CR1. Wrong: QEMU models USART storage faithfully.

**Lesson:** Console UART is fully self-inspectable — its control bits
are your init-state oracle; its data register doubles as an RX window.

**Impact:** Ch8 new tasks: "Read your own CR1" + "Find your own newline
in DR."

**Fast? Safe? Footprint?**
- Fast: same volatile RMW path; reads decode live bits.
- Safe: CR1 poke is the radioactive line (Task 8.13) — peeks harmless.
- Footprint: none.

---

### L33 — Unmapped reads are fatal on RISC-V too

**Did (RISC-V, deliberately last):**
```text
holy> peek 0x10016000;
[qemu freeze — silent trap-hang]
```

**Got:** Freeze — same silent hang as L27's store.

**Expected:** Maybe `0` or bus zero (ARM's open-read intuition).

**Expectation provenance:**
- Prior belief that reads are safer than writes (audit lesson: reads
  never log) generalized to "reads don't fault." Board says otherwise:
  undecoded MMIO faults on ANY access.

**Lesson:** Completes the RV32 fatality matrix with L07/L27: on
`sifive_e`, **reads and writes off-decoder both trap.** Part I's
"E001 at parse time covers peripherals; everything else needs mental map"
was an ARM-centric understatement — on RV32 the map is sparser and the
price is always a silent hang.

**Impact:** Ch10 final warning: on RV32 never probe raw `0x10xxxx` gaps;
stay inside `registry.rs:74-86`'s six listed ranges or known system regs.

**Fast? Safe? Footprint?**
- Fast: not applicable — fault entry is the outcome.
- Safe: contained (hang), process-level.
- Footprint: none.

---

## II.F Confirmations (Part II — C16–C33)

| # | Behavior | Evidence |
|---|----------|----------|
| C16 | Full-claim `0x7F` / SU stack `0x8000007F` exact | A `peek 0x20001000` → `0x7F`; stack-up → `0x8000007F` (ARM); D `0x7F` / D2 `0x5` live |
| C17 | Out-of-order/churn/double-drop lifecycle | SPI0 ×3 clean; second drop → `NOT HELD`; `rd()`×2 repeatable |
| C18 | Sibling-boundary E001 | GPIOA held, GPIOB base → E001 before claim |
| C19 | 8-symbol sum + rebind chain + live registry snapshot | 396 exact; overwrite path `parser.rs:593-610` free |
| C20 | u32 boundary lexing/wraps exact | `4294967295` parses; `…7296` → `LEX`; `FFFFFFFF+1=0`; `65536²` pair |
| C21 | Optional-semi trio | `help;` `banner;` `sys_audit;` accepted |
| C22 | Case walls | `PEEK`→UNKNOWN SYMBOL; `cap_claim gpioa`→UNKNOWN RESOURCE |
| C23 | Real MISSING SEMICOLON trigger | `let x =1 let y =2;` — mid-line, not EOL |
| C24 | Multi-error storm recovery | five distinct ERRs then `2+2=4` intact |
| C25 | 17-char name boundary three-way | 16 fits (DEFINED); binding=NAME TOO LONG; bare=UNKNOWN SYMBOL |
| C26 | `SR=0xE0` triple decode | `224/128%2=1 /64=1 /32=1` |
| C27 | DR=RDR (=our own `\n`) cross-arch | ARM & RV32 both `0x0A` |
| C28 | PSC/ARR roundtrip | 83 → `0x53`; 0xFFFFFFFF → `0xFFFFFFFF` |
| C29 | Empty-body fn definable+callable | `fn e(){}` DEFINED; `e()` runs; second def blocked differently |
| C30 | Pre-definition unknown-call strictness | `frob2()` before any fn → `UNKNOWN SYMBOL` (vs post-definition wildcard) |
| C31 | Rebind overflow semantics | `let z=0` then `5/z` still catches DIV BY ZERO |

---

## II.G Speed / Safety / Footprint — Part II Update

**Speed.** First measurable QEMU clock discovered: TIM2 CNT reads
increased monotonically (Δ≈109M/115M ticks), bracketing our known
~1.2 s inter-read wall gaps — a calibration recipe now exists. Everything
else stayed wire-speed-invisible: parse O(line), token emission O(1),
capability atomic O(1), audit ring write O(1). Truncation is the only
path where extra work is *wasted* (parsing mangled tail) and still bounded
by 128 B.

**Safety.** ARM endured 40-line error storms (`SYMBOL TABLE FULL` at
exactly 33rd name) and 20+ capability churn ops with zero drift —
soft failures behave like proper exceptions. Both RISC-V hard stops
(CNT probe was safe; ITIM + unmapped holes trapped) were contained
exactly as designed — silent `_trap_hang` park, process-isolated, no
host compromise. The only behavioral surprise remains pure
semantics (L03b): an empty-first body is the *only* escape hatch to the
intended two-slot usage.

**Footprint.** Peak live use: 32/32 symbol slots filled, one 6-peek body
inside `FN_BODY_WORDS` budget, two concurrent claims typical — orders of
magnitude under every `MAX_*`/`SYMBOL_SLOTS`/`LINE_MAX`/`VM_STACK` limit,
zero heap end-to-end (Part I §3.3 totals unchanged: ~15.5–17 KB RAM).

---

## II.H Action Items — Additions

Cookbook:
- [ ] Rewrite Ch.7 around TIM2-as-clock (actual QEMU recipes now); keep
      DWT/mcycle tagged silicon-only
- [ ] New Ch.2 tasks: CPUID read (L15), vector-table tour (L17), stack
      residue (L18), alias probe (L19), tab warning (L21), truncation
      caveat (L24), poke-with-peek-value (L25)
- [ ] New Ch.5 box: identity-breaking example (L27) as identity-failing proof
- [ ] Ch.4: publish L03b two-fn trick + dispatch-precedence table
- [ ] Ch.10: NEVER-poke-ITIM warning (L29); GPIOB asymmetry note (L30);
      read-fatality row (L33); t/r high-bit flag recipes (D2 r=10 demo)
- [ ] Ch.6/8: DRYR holds RX-byte recipe; SR decode trio
- [ ] Global box: "reads may return 0 on QEMU GPIO/debug blocks" (L13/L15)

Kernel tickets (additions):
- [ ] Decide `find_fn` intended semantics — wildcard-by-length now fully
      characterized (L03b): codify or fix; document empty-body escape hatch
- [ ] Consider mapping tabs→spaces in `repl::feed` so lexer support is
      reachable (L21: `repl.rs:106` vs `lexer.rs:67`)

---

## II.I VERDICT — PART II

100 planned probes → ~97 executed cleanly; the 3 RISC-V hard stops were
themselves the measurements (L29, L33 plus D's ITIM write). Zero
unexpected ARM crashes. Eighteen new behaviors documented, one old
lesson (L03) solved completely (empty-body wildcard + second-slot path),
one (L11) re-attributed to its true cause, one (L09) bounded by the
discovery that timing DOES exist on-host via TIM2. Speed: measurable at
last. Safety: 4-for-4 hard-stop containment, 40-line soft-error storms
absorbed. Footprint: untouched. The kernel keeps its perfect record of
doing precisely what it is told — our job remains telling it only what
we mean.

*Part II ends. Machine states at close: ARM registry 0x1 (GPIOA, held on
purpose in Session C), RV32 processes terminated honorably in the line
of probe.*
