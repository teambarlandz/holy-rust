# Chapter 2: Memory-Mapped I/O & Volatile Primitives

---

## 2.1 What MMIO Is

Microcontrollers map every hardware register --- GPIO pins, UART baud rates, timers, DMA channels --- into the same flat 32-bit address space as SRAM and Flash. This is **Memory-Mapped I/O (MMIO)**. A 32-bit write to `0x40020000` sends bits directly to the GPIOA output pins. There is no special instruction; the CPU uses the same `load`/`store` as any memory access.

The address map is fixed by silicon. On STM32F405, `0x40020000`-`0x400203FF` is GPIOA, `0x40020400`-`0x400207FF` is GPIOB, `0x40011000`-`0x400113FF` is USART1. On RISC-V (SiFive FE310), `0x10012000`-`0x10012FFF` is GPIO, `0x10013000`-`0x10013FFF` is UART0.

Holy Rust resolves addresses via `addr_to_cap_id()` (`src/capabilities/registry.rs:46`), which performs a `match` against these architecture-specific ranges.

## 2.2 The Two Volatile Primitives

### `peek_u32`

```rust
#[inline(always)]
pub fn peek_u32(addr: usize) -> u32 {
    unsafe { core::ptr::read_volatile(addr as *const u32) }
}
```

Reads a 32-bit value from a physical address. `read_volatile` prevents the compiler from optimizing away or reordering the read. The CPU issues a single load instruction (1-3 cycles).

### `poke_u32`

```rust
#[inline(always)]
pub fn poke_u32(addr: usize, value: u32) {
    unsafe { core::ptr::write_volatile(addr as *mut u32, value) }
}
```

Writes a 32-bit value. Same volatile guarantee. A single store instruction, 1-3 cycles. `#[inline(always)]` ensures zero function-call overhead.

## 2.3 Under the Hood

| Primitive | Rust Intrinsic | ARM Instruction | Cycles |
|---|---|---|---|
| `peek_u32` | `core::ptr::read_volatile` | `LDR Rn, [Rm]` | 1-3 |
| `poke_u32` | `core::ptr::write_volatile` | `STR Rn, [Rm]` | 1-3 |

The safety contract: Ring 0 single-address-space system; every address is directly reachable physical memory or MMIO. Unmapped addresses trigger a HardFault. No silent corruption.

## 2.4 Read-Modify-Write: `reg_set_bit` and `reg_clr_bit`

Most peripheral registers require read-modify-write to change a single bit without clobbering the rest:

```rust
pub fn reg_set_bit(addr: usize, bit: u8) {
    let updated = peek_u32(addr) | (1u32 << bit);
    poke_u32(addr, updated);
}
pub fn reg_clr_bit(addr: usize, bit: u8) {
    let updated = peek_u32(addr) & !(1u32 << bit);
    poke_u32(addr, updated);
}
```

`reg_set_bit` uses OR to set the target bit; `reg_clr_bit` uses AND with an inverted mask to clear it. Cost: one load, one ALU op, one store. O(1).

## 2.5 Real STM32F4 Register Programming

### Enable the GPIOA Clock

```text
holy> cap_claim RCC;
CAP CLAIMED RCC id=0
holy> reg_set_bit 0x40023830 0;
OK
```

`0x40023830` is RCC AHB1ENR. Bit 0 enables GPIOA's clock.

### Set PA5 as Output

```text
holy> cap_claim GPIOA;
CAP CLAIMED GPIOA id=0
holy> reg_set_bit 0x40020000 10;
OK
```

`0x40020000` is GPIOA MODER. Bits [11:10] control pin 5. Setting bit 10 makes it `01` = output.

### Set PA5 High

```text
holy> poke 0x40020014 32;
OK
```

`0x40020014` is GPIOA BSRR. `32 = 1 << 5` sets pin 5 high. The LED turns on.

### Set PA5 Low

```text
holy> poke 0x40020014 2097152;
OK
```

`2097152 = 1 << 21` clears pin 5 via the upper half of BSRR. The LED turns off.

## 2.6 The Capability Enforcement Layer

The **enforced** variants gate every access through the capability registry:

```rust
pub fn enforced_poke_u32(addr: u32, value: u32) -> Result<(), MemError> {
    if registry::is_superuser_active() {
        // SuperUser bypass: log to audit ring buffer.
        unsafe {
            (*core::ptr::addr_of_mut!(
                crate::capabilities::audit::SUPERUSER_AUDIT_LOG
            )).record_event(addr, value);
        }
    } else if let Some(cap_id) = registry::addr_to_cap_id(addr) {
        if !registry::is_claimed(cap_id as usize) {
            return Err(MemError::CapabilityViolation);
        }
    }
    // None => SRAM/unmapped => unrestricted.
    unsafe { core::ptr::write_volatile(addr as *mut u32, value) };
    Ok(())
}
```

The two-layer defense:

1. **Compile-time**: The parser calls `registry::check_access(addr)` at parse time. Unclaimed peripherals produce `ParseError::CapabilityViolation` --- the program never executes.
2. **Runtime**: `enforced_poke_u32` re-checks before the volatile access. Catches stale compiled programs executed under a different capability state.

## 2.7 SRAM Addresses Pass Through Freely

The `addr_to_cap_id` function returns `None` for addresses outside any peripheral range (SRAM, Flash, unmapped). When `None`, the enforced variants skip all capability checks. This is correct: reading/writing SRAM is a fundamental kernel operation.

## 2.8 SuperUserCap Bypass and Audit Logging

The SuperUser token (bit 31) bypasses all peripheral checks. Every write is logged to the audit ring buffer: 16 entries, 12 bytes each = 192 bytes SRAM. Each entry records address, value, and cycle count. Reads are not logged (side-effect-free); only writes are audited.

```text
holy> sys_audit;
--- SUPERUSER AUDIT LOG ---
Total Unsafe Operations: 2
Recent Events:
ADDR: 0x40020014 | VAL: 0x00000020 | CYCLES: 1245032
ADDR: 0x40020014 | VAL: 0x00200000 | CYCLES: 1245187
```

## 2.9 The `MemError` Enum

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemError {
    CapabilityViolation,   // E001: peripheral token not claimed
    PermissionDenied,      // E002: unmapped MMIO without SuperUserCap
}
```

## 2.10 Real REPL Session: Poke Without Capability

```text
holy> poke 0x40020014 32;
E001: CAPABILITY_VIOLATION - Peripheral token not claimed
holy> cap_claim GPIOA;
CAP CLAIMED GPIOA id=0
holy> poke 0x40020014 32;
OK
holy> poke 0x40020014 2097152;
OK
holy> cap_drop GPIOA;
CAP RELEASED GPIOA
holy> poke 0x40020014 32;
E001: CAPABILITY_VIOLATION - Peripheral token not claimed
```

1. **`poke 0x40020014 32`** --- `addr_to_cap_id` returns `Some(GpioA)`, `is_claimed(0)` is `false`. E001.
2. **`cap_claim GPIOA`** --- `acquire(0)` does `fetch_or` on bit 0. Transitions 0 -> 1.
3. **`poke 0x40020014 32`** --- `is_claimed(0)` is `true`. Volatile write proceeds. LED on.
4. **`cap_drop GPIOA`** --- `release(0)` does `fetch_and` to clear bit 0.
5. **`poke 0x40020014 32`** --- Bit is clear again. E001 fires.

The entire cycle completes in microseconds over UART. Hardware is safe by construction.

---

*Next chapter: The Capability Safety Engine --- O(1) atomic bitfields, linear tokens, and the compile-time enforcement layer.*
