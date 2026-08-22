# Chapter 3: The Capability Safety Engine

---

## 3.1 Why Capability Safety Matters

In a bare-metal Ring 0 system with no MMU, every address is reachable. A misplaced `poke` can reconfigure a clock tree or corrupt DMA descriptors. Traditional solutions (MPU regions, MMU page tables) add complexity and are often unavailable on small cores.

Holy Rust provides O(1) operations, zero allocation, two-layer enforcement, and linear ownership via `!Copy` + `!Clone` tokens. Cost: 32 bytes of SRAM for the registry.

## 3.2 The O(1) Atomic Bitfield Registry

The registry is 256 bits, 8 `AtomicU32` words in SRAM:

```rust
pub const MAX_RESOURCES: usize = 256;
const WORDS: usize = MAX_RESOURCES / 32;

#[repr(C, align(4))]
pub struct RegistryBits(pub [AtomicU32; WORDS]);

#[used]
#[link_section = ".capability_registry"]
pub static REGISTRY_BITS: RegistryBits =
    RegistryBits([const { AtomicU32::new(0) }; WORDS]);
```

Placed at `SRAM + 0x1000`. Each bit: 0 = available, 1 = claimed.

## 3.3 The `CapId` Enum

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum CapId {
    GpioA = 0, GpioB = 1, Uart0 = 2, Spi0 = 3,
    I2c0 = 4, Timer0 = 5, Dma0 = 6, SuperUser = 31,
}
```

## 3.4 `addr_to_cap_id()`: Per-Architecture Address Resolution

```rust
#[inline(always)]
pub fn addr_to_cap_id(addr: u32) -> Option<CapId> {
    #[cfg(target_arch = "arm")]  { arm_addr_to_cap(addr) }
    #[cfg(target_arch = "riscv32")] { riscv_addr_to_cap(addr) }
}
```

ARM ranges: `0x4002_0000..=0x4002_03FF` = GpioA, `0x4002_0400..=0x4002_07FF` = GpioB, `0x4001_1000..=0x4001_13FF` = Uart0, `0x4001_3000..=0x4001_33FF` = Spi0, `0x4001_5400..=0x4001_57FF` = I2c0, `0x4000_0000..=0x4000_03FF` = Timer0, `0x4000_2000..=0x4000_23FF` = Dma0.

RISC-V ranges: `0x1001_2000..=0x1001_2FFF` = GpioA, `0x1001_3000..=0x1001_3FFF` = Uart0, `0x1001_4000..=0x1001_4FFF` = Spi0, `0x1002_0000..=0x1002_0FFF` = I2c0, `0x1001_5000..=0x1001_5FFF` = Timer0, `0x1000_0000..=0x1000_0FFF` = Dma0.

`_ => None` returns no capability for SRAM, Flash, and unmapped addresses.

## 3.5 `check_access()`: Compile-Time Enforcement

```rust
pub fn check_access(addr: u32) -> Result<(), CapId> {
    if let Some(cap_id) = addr_to_cap_id(addr) {
        if !is_claimed(cap_id as usize) {
            return Err(cap_id);
        }
    }
    Ok(())
}
```

The parser calls this for `poke`/`peek`/`reg_set_bit`/`reg_clr_bit`. Unclaimed peripherals produce `ParseError::CapabilityViolation` --- the program never executes. Same check applies inside `fn` body compilation (`src/compiler/parser.rs:358`).

## 3.6 `acquire()`: Atomic Bit-Test-and-Set

```rust
pub fn acquire(resource_id: usize) -> bool {
    let word = resource_id / 32;
    let bit = resource_id % 32;
    match REGISTRY_BITS.0.get(word) {
        Some(w) => {
            let mask = 1u32 << bit;
            let prev = w.fetch_or(mask, Ordering::AcqRel);
            prev & mask == 0
        }
        None => false,
    }
}
```

`fetch_or` atomically ORs the mask. Bit 0 -> 1 = claimed, returns `true`. `Ordering::AcqRel` ensures correct ordering even if an interrupt races the REPL.

## 3.7 `release()`: Atomic Bit-Clear

```rust
pub fn release(resource_id: usize) {
    let word = resource_id / 32;
    let bit = resource_id % 32;
    if let Some(w) = REGISTRY_BITS.0.get(word) {
        w.fetch_and(!(1u32 << bit), Ordering::AcqRel);
    }
}
```

## 3.8 `is_claimed()` / `available()`: Single-Bit Load

```rust
pub fn available(resource_id: usize) -> bool {
    let word = resource_id / 32;
    let bit = resource_id % 32;
    match REGISTRY_BITS.0.get(word) {
        Some(w) => w.load(Ordering::Acquire) & (1u32 << bit) == 0,
        None => false,
    }
}
pub fn is_claimed(resource_id: usize) -> bool { !available(resource_id) }
```

## 3.9 The Linear Token System

```rust
pub struct Cap<T: HardwareResource> {
    id: u16,
    _phantom: PhantomData<T>,
}
```

Not `Copy`, not `Clone`. A **linear (affine) token**: moved or dropped, never duplicated. No `Drop` impl exists --- releasing hardware is an explicit act (`drop_cap`), not implicit.

## 3.10 The `HardwareResource` Trait and Resource Definitions

```rust
pub trait HardwareResource {
    const RESOURCE_ID: u16;
    const NAME: &'static str;
}

macro_rules! define_resource {
    ($name:ident, $id:expr, $label:expr) => {
        pub struct $name;
        impl HardwareResource for $name {
            const RESOURCE_ID: u16 = $id;
            const NAME: &'static str = $label;
        }
    };
}

define_resource!(GpioA,        0,  "GPIOA");
define_resource!(GpioB,        1,  "GPIOB");
define_resource!(Uart0,        2,  "UART0");
define_resource!(Spi0,         3,  "SPI0");
define_resource!(I2c0,         4,  "I2C0");
define_resource!(Timer0,       5,  "TIMER0");
define_resource!(Dma0,         6,  "DMA0");
define_resource!(SuperUserCap, 31, "SUPERUSER");
```

`resolve_name()` maps REPL strings to resource IDs.

## 3.11 `claim` / `drop_cap` / `steal`

**`claim`** returns `Some(Cap<T>)` on success, `None` if already claimed.

**`drop_cap`** consumes the token and clears the registry bit.

**`steal`** is an `unsafe` bypass for early boot code. The bit is marked claimed so later claims fail.

## 3.12 The SuperUser Audit Ring Buffer

```rust
pub struct AuditEntry {
    pub addr: u32,
    pub val: u32,
    pub timestamp_cycles: u32,
}
pub struct AuditLog {
    buffer: [AuditEntry; 16],
    head: usize,
    count: usize,
}
```

**16 entries x 12 bytes = 192 bytes SRAM.** `record_event` stores address, value, and cycle count. The ring wraps when full.

## 3.13 `get_cycle_count()`: Architecture-Specific Cycle Counter

**ARM:** `core::ptr::read_volatile(0xE000_1004 as *const u32)` --- DWT->CYCCNT.

**RISC-V:** `csrr mcycle` --- single instruction.

## 3.14 The `sys_audit` REPL Command

Dumps the audit log:

```text
holy> sys_audit;
--- SUPERUSER AUDIT LOG ---
Total Unsafe Operations: 2
Recent Events:
ADDR: 0x40020014 | VAL: 0x00000020 | CYCLES: 892014
ADDR: 0x40020014 | VAL: 0x00200000 | CYCLES: 892187
```

## 3.15 Defense-in-Depth: Two Layers of Enforcement

**Layer 1 (Compile-Time):** The parser calls `registry::check_access(addr)`. Unclaimed peripheral addresses produce `ParseError::CapabilityViolation` before any code is emitted.

**Layer 2 (Runtime):** `enforced_poke_u32` re-checks at execution time. Stale compiled programs from when the capability *was* claimed are still caught.

## 3.16 Real REPL Sessions

**Unclaimed poke -> E001:**

```text
holy> poke 0x40020014 32;
E001: CAPABILITY_VIOLATION - Peripheral token not claimed
```

**Claim -> poke -> OK:**

```text
holy> cap_claim GPIOA;
CAP CLAIMED GPIOA id=0
holy> poke 0x40020014 32;
OK
```

**Double claim -> BUSY:**

```text
holy> cap_claim GPIOA;
CAP CLAIMED GPIOA id=0
holy> cap_claim GPIOA;
CAP BUSY GPIOA
```

**Drop -> re-claim:**

```text
holy> cap_claim GPIOA;
CAP CLAIMED GPIOA id=0
holy> cap_drop GPIOA;
CAP RELEASED GPIOA
holy> cap_claim GPIOA;
CAP CLAIMED GPIOA id=0
```

**SuperUser bypass with audit:**

```text
holy> cap_claim SUPERUSER;
CAP CLAIMED SUPERUSER id=31
holy> poke 0x40020014 32;
OK
holy> poke 0x40020014 2097152;
OK
holy> sys_audit;
--- SUPERUSER AUDIT LOG ---
Total Unsafe Operations: 2
Recent Events:
ADDR: 0x40020014 | VAL: 0x00000020 | CYCLES: 892014
ADDR: 0x40020014 | VAL: 0x00200000 | CYCLES: 892187
```

---

*Next chapter: The Streaming Compiler --- single-pass parsing, threaded execution, and the on-chip JIT.*
