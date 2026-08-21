//! Direct volatile memory access primitives.
//!
//! These are the kernel-side `peek`/`poke` family used by the REPL and by
//! boot code. All accesses are volatile so they are never elided or
//! reordered by the optimizer — MMIO semantics.
//!
//! The `enforced_*` variants gate every MMIO access through the O(1)
//! capability registry (doc ch.2).  SRAM / flash addresses return `None`
//! from `addr_to_cap_id` and pass through without a capability check.
//! SuperUserCap bypasses all peripheral checks but every write is logged
//! to the audit ring buffer.

use crate::capabilities::registry;

/// Error returned by enforced memory operations when a capability
/// violation is detected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemError {
    /// Peripheral token not claimed (E001).
    CapabilityViolation,
    /// Unmapped MMIO access without SuperUserCap (E002).
    PermissionDenied,
}

impl MemError {
    /// Human-readable error prefix for UART output.
    pub fn as_bytes(&self) -> &'static [u8] {
        match self {
            MemError::CapabilityViolation => {
                b"E001: CAPABILITY_VIOLATION - Peripheral token not claimed"
            }
            MemError::PermissionDenied => {
                b"E002: PERMISSION_DENIED - Unmapped MMIO access requires SuperUserCap"
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Raw (unchecked) access — used by boot code and the UART driver.
// ---------------------------------------------------------------------------

/// Read a 32-bit value from a physical address (1-3 cycles).
#[inline(always)]
pub fn peek_u32(addr: usize) -> u32 {
    // SAFETY: Ring 0 single-address-space system: every address is directly
    // reachable physical memory or MMIO. Volatile read of an aligned u32;
    // alignment is the caller's contract, matching raw hardware access.
    unsafe { core::ptr::read_volatile(addr as *const u32) }
}

/// Write a 32-bit value to a physical address (1-3 cycles).
#[inline(always)]
pub fn poke_u32(addr: usize, value: u32) {
    // SAFETY: see peek_u32; volatile write to caller-provided address.
    unsafe { core::ptr::write_volatile(addr as *mut u32, value) }
}

/// Set bit `bit` at memory-mapped register address `addr`.
///
/// Note: implemented as a volatile read-modify-write. The reference doc
/// sketched a plain `write(1 << bit)`, which would clobber every other bit
/// in the register; RMW preserves them at identical O(1) cost.
pub fn reg_set_bit(addr: usize, bit: u8) {
    let updated = peek_u32(addr) | (1u32 << bit);
    poke_u32(addr, updated);
}

/// Clear bit `bit` at memory-mapped register address `addr`.
pub fn reg_clr_bit(addr: usize, bit: u8) {
    let updated = peek_u32(addr) & !(1u32 << bit);
    poke_u32(addr, updated);
}

// ---------------------------------------------------------------------------
// Capability-enforced access (doc ch.2 mandatory enforcement)
// ---------------------------------------------------------------------------

/// Capability-guarded poke: checks the address against the SRAM
/// capability registry before performing the write.
///
/// - Peripheral addresses require the matching capability to be claimed.
/// - SRAM / flash / unmapped addresses pass through freely.
/// - SuperUserCap bypasses all checks but logs to the audit ring.
#[inline(always)]
pub fn enforced_poke_u32(addr: u32, value: u32) -> Result<(), MemError> {
    if registry::is_superuser_active() {
        // SuperUser bypass: write anything, but record in the audit log.
        // SAFETY: single-threaded REPL path; audit log is not reentrant.
        unsafe {
            (*core::ptr::addr_of_mut!(crate::capabilities::audit::SUPERUSER_AUDIT_LOG))
                .record_event(addr, value);
        }
    } else if let Some(cap_id) = registry::addr_to_cap_id(addr) {
        // Peripheral address — require the matching capability.
        if !registry::is_claimed(cap_id as usize) {
            return Err(MemError::CapabilityViolation);
        }
    }
    // None → SRAM / unmapped → unrestricted.
    // SAFETY: capability verified above; volatile write to Ring 0 address.
    unsafe { core::ptr::write_volatile(addr as *mut u32, value) };
    Ok(())
}

/// Capability-guarded peek: checks the address against the SRAM
/// capability registry before performing the read.
#[inline(always)]
pub fn enforced_peek_u32(addr: u32) -> Result<u32, MemError> {
    if registry::is_superuser_active() {
        // SuperUser bypass: read is allowed (reads are side-effect-free
        // from a safety perspective; only writes need audit logging).
    } else if let Some(cap_id) = registry::addr_to_cap_id(addr) {
        if !registry::is_claimed(cap_id as usize) {
            return Err(MemError::CapabilityViolation);
        }
    }
    // SAFETY: capability verified above; volatile read from Ring 0 address.
    Ok(unsafe { core::ptr::read_volatile(addr as *const u32) })
}

/// Boot-time initialization: copy `.data` from its flash load address
/// (`__sidata`) to its SRAM virtual address, then zero `.bss`.
///
/// # Safety
/// Must run exactly once, first thing after reset entry, before any Rust
/// code touches initialized or zeroed statics. Requires the linker symbols
/// defined in `memory-layout.x` and word-aligned sections (guaranteed by
/// the ALIGN(4) directives there).
pub unsafe fn init_data_bss() {
    extern "C" {
        static mut __sidata: u32;
        static mut __sdata: u32;
        static mut __edata: u32;
        static mut __sbss: u32;
        static mut __ebss: u32;
    }

    let mut src = core::ptr::addr_of!(__sidata);
    let mut dst = core::ptr::addr_of_mut!(__sdata);
    let data_end = core::ptr::addr_of_mut!(__edata);
    while dst < data_end {
        // SAFETY: both pointers derive from linker symbols bounding the
        // .data section; volatile keeps the copy loop observable to the
        // compiler despite no local reads of the destination.
        core::ptr::write_volatile(dst, core::ptr::read_volatile(src));
        src = src.add(1);
        dst = dst.add(1);
    }

    let mut z = core::ptr::addr_of_mut!(__sbss);
    let bss_end = core::ptr::addr_of_mut!(__ebss);
    while z < bss_end {
        // SAFETY: bounds guaranteed by __sbss/__ebss linker symbols.
        core::ptr::write_volatile(z, 0);
        z = z.add(1);
    }
}
