//! Direct volatile memory access primitives.
//!
//! These are the kernel-side `peek`/`poke` family used by the REPL and by
//! boot code. All accesses are volatile so they are never elided or
//! reordered by the optimizer — MMIO semantics.

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
