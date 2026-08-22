//! Fixed-size capability bitfield registry.
//!
//! One bit per hardware resource, resident in SRAM at `__capreg_base`
//! (SRAM + 0x1000) via the `.capability_registry` link section — matching
//! the address contract in docs/CHAPTER_02.
//!
//! All operations are O(1). Atomics are used instead of the doc's plain
//! read-modify-write so acquire/release stay correct even if an interrupt
//! handler races the REPL; on single-core silicon this compiles to simple
//! load/store-with-reservation sequences.

use core::sync::atomic::{AtomicU32, Ordering};

/// Total tracked resources (8 words x 32 bits).
pub const MAX_RESOURCES: usize = 256;

const WORDS: usize = MAX_RESOURCES / 32;

// ---------------------------------------------------------------------------
// Capability identifiers (one per hardware resource)
// ---------------------------------------------------------------------------

/// Hardware resource identifiers. Values match the bit positions in
/// [`REGISTRY_BITS`] and the indices used by [`tokens::resolve_name`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum CapId {
    GpioA = 0,
    GpioB = 1,
    Uart0 = 2,
    Spi0 = 3,
    I2c0 = 4,
    Timer0 = 5,
    Dma0 = 6,
    SuperUser = 31,
}

/// O(1) physical address → capability ID resolution.
///
/// Returns `None` for addresses that fall outside any peripheral region
/// (SRAM, flash, etc.) — those are unrestricted and require no capability.
///
/// SuperUser addresses map to `Some(CapId::SuperUser)` so the caller
/// can check whether the SuperUser token is active.
#[inline(always)]
pub fn addr_to_cap_id(addr: u32) -> Option<CapId> {
    #[cfg(target_arch = "arm")]
    {
        arm_addr_to_cap(addr)
    }
    #[cfg(target_arch = "riscv32")]
    {
        riscv_addr_to_cap(addr)
    }
}

// ARM Cortex-M peripheral address ranges (STM32F405)
#[cfg(target_arch = "arm")]
#[inline(always)]
fn arm_addr_to_cap(addr: u32) -> Option<CapId> {
    match addr {
        0x4002_0000..=0x4002_03FF => Some(CapId::GpioA),
        0x4002_0400..=0x4002_07FF => Some(CapId::GpioB),
        0x4001_1000..=0x4001_13FF => Some(CapId::Uart0),
        0x4001_3000..=0x4001_33FF => Some(CapId::Spi0),
        0x4001_5400..=0x4001_57FF => Some(CapId::I2c0),
        0x4000_0000..=0x4000_03FF => Some(CapId::Timer0),
        0x4000_2000..=0x4000_23FF => Some(CapId::Dma0),
        _ => None,
    }
}

// RISC-V SiFive FE310 peripheral address ranges
#[cfg(target_arch = "riscv32")]
#[inline(always)]
fn riscv_addr_to_cap(addr: u32) -> Option<CapId> {
    match addr {
        0x1001_2000..=0x1001_2FFF => Some(CapId::GpioA),
        0x1001_3000..=0x1001_3FFF => Some(CapId::Uart0),
        0x1001_4000..=0x1001_4FFF => Some(CapId::Spi0),
        0x1002_0000..=0x1002_0FFF => Some(CapId::I2c0),
        0x1001_5000..=0x1001_5FFF => Some(CapId::Timer0),
        0x1000_0000..=0x1000_0FFF => Some(CapId::Dma0),
        _ => None,
    }
}

/// Check whether an address falls within any claimed capability, or is
/// unrestricted (SRAM / flash / unmapped). Returns `Ok(())` if access
/// is permitted, `Err(cap_id)` if the peripheral is not claimed.
///
/// SuperUser bypass is evaluated first. Fail-closed boundaries reject unmapped MMIO.
#[inline(always)]
pub fn check_access(addr: u32) -> Result<(), CapId> {
    if is_superuser_active() {
        return Ok(());
    }

    if let Some(cap_id) = addr_to_cap_id(addr) {
        if !is_claimed(cap_id as usize) {
            return Err(cap_id);
        }
        Ok(())
    } else {
        #[cfg(target_arch = "arm")]
        let is_ram_flash = matches!(addr, 0x0800_0000..=0x080F_FFFF | 0x2000_0000..=0x2001_C000);
        #[cfg(target_arch = "riscv32")]
        let is_ram_flash = matches!(addr, 0x2000_0000..=0x2000_FFFF | 0x8000_0000..=0x8000_FFFF);

        if is_ram_flash {
            Ok(())
        } else {
            Err(CapId::SuperUser)
        }
    }
}

/// Returns true when the SuperUser capability is currently claimed.
#[inline(always)]
pub fn is_superuser_active() -> bool {
    !available(CapId::SuperUser as usize)
}

// ---------------------------------------------------------------------------
// Bitfield registry
// ---------------------------------------------------------------------------

/// Capability availability bitmap. Bit set = resource claimed.
///
/// Wrapped in a struct to carry `#[repr(align(4))]` (repr attributes do
/// not apply to statics directly).
#[repr(C, align(4))]
pub struct RegistryBits(pub [AtomicU32; WORDS]);

#[used]
#[link_section = ".capability_registry"]
pub static REGISTRY_BITS: RegistryBits = RegistryBits([const { AtomicU32::new(0) }; WORDS]);

/// Returns true when no owner holds `resource_id` (single-bit lookup).
#[inline(always)]
pub fn available(resource_id: usize) -> bool {
    let word = resource_id / 32;
    let bit = resource_id % 32;
    // SAFETY-free: bounds enforced by modulo against a compile-time-sized
    // array; index < WORDS always holds for resource_id < MAX_RESOURCES.
    match REGISTRY_BITS.0.get(word) {
        Some(w) => w.load(Ordering::Acquire) & (1u32 << bit) == 0,
        None => false,
    }
}

/// Returns true when `resource_id` is claimed (bit is set).
#[inline(always)]
pub fn is_claimed(resource_id: usize) -> bool {
    !available(resource_id)
}

/// Atomically claim `resource_id`. Returns false if already claimed.
///
/// O(1): one fetch-or test-and-set. On loss the bit is left set (it was
/// already set by the winner), so state stays consistent.
#[inline(always)]
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

/// Atomically release `resource_id` (clear its bit). O(1).
#[inline(always)]
pub fn release(resource_id: usize) {
    let word = resource_id / 32;
    let bit = resource_id % 32;
    if let Some(w) = REGISTRY_BITS.0.get(word) {
        w.fetch_and(!(1u32 << bit), Ordering::AcqRel);
    }
}
