//! Dynamic vector table relocation and C-ABI interrupt trampolines.
//!
//! Holy Rust routes hardware interrupts straight into SRAM-resident handler
//! slots (< 12 cycle dispatch target, doc ch.4). At boot the flash vector
//! table is mirrored into SRAM and the CPU's vector base register is
//! repointed there, so REPL sessions can overwrite handlers at runtime.

// RISC-V trap hang stub: any unexpected trap before vectored dispatch is
// configured spins here, preserving fault state for a debugger instead of
// jumping through address 0.
#[cfg(target_arch = "riscv32")]
core::arch::global_asm!(
    ".section .text.trap_hang, \"ax\"",
    ".globl _trap_hang",
    "_trap_hang:",
    "j _trap_hang"
);

/// Catch-all core-exception handler (NMI, HardFault, MemManage, BusFault,
/// UsageFault, SVCall, DebugMon, PendSV, SysTick).
///
/// A wild `peek` in Ring 0 *will* fault by design — there is no MMU standing
/// between the user and the machine. This handler makes that failure visible
/// (UART announcement) instead of escalating to a silent lockup.
///
/// Referenced by name from the ARM linker script's `.isr_vector`, so it must
/// stay `#[no_mangle]` and never be inlined away.
#[cfg(target_arch = "arm")]
#[no_mangle]
pub extern "C" fn fault_hang() -> ! {
    crate::drivers::uart::write_str(b"\r\n**FAULT: core exception, halted**\r\n");
    loop {
        // Sleep until a debugger or reset ends the session; wfi keeps the
        // core quiet so a JTAG attach sees stable fault state.
        unsafe { core::arch::asm!("wfi") }
    }
}

/// Number of dispatch slots in the SRAM vector table.
pub const VECTOR_SLOTS: usize = 256;

/// SRAM-resident vector table placed at `__sram_vectors_base`
/// (SRAM + 0x400) via the `.sram_vectors` link section.
///
/// Stored as raw words (not `Option<fn()>`) so slot layout matches what the
/// hardware consumes word-for-word; 0 means "no handler". Wrapped in a
/// struct to carry `#[repr(align(1024))]` for VTOR alignment.
#[repr(C, align(1024))]
pub struct VectorTable(pub [u32; VECTOR_SLOTS]);

#[used]
#[link_section = ".sram_vectors"]
pub static mut VECTOR_TABLE: VectorTable = VectorTable([0; VECTOR_SLOTS]);

/// ARM Cortex-M Vector Table Offset Register.
#[cfg(target_arch = "arm")]
const VTOR: usize = 0xE000_ED08;

/// Install (or remove with `None`) a handler in SRAM vector slot `slot`.
///
/// # Safety
/// `slot` must be < [`VECTOR_SLOTS`]. The handler runs in Ring 0 interrupt
/// context once the vector base is relocated; it must be interrupt-safe.
pub unsafe fn set_handler(slot: usize, handler: Option<fn()>) {
    let word = match handler {
        // Casting fn -> usize preserves the Thumb bit on ARM targets.
        Some(h) => h as usize as u32,
        None => 0,
    };
    // SAFETY: slot < VECTOR_SLOTS is the caller's contract; deref of the
    // static's raw pointer is single-threaded boot/REPL context.
    unsafe {
        (*core::ptr::addr_of_mut!(VECTOR_TABLE.0))[slot] = word;
    }
}

/// Read back the raw handler word installed in `slot` (diagnostics).
pub fn get_handler(slot: usize) -> u32 {
    // SAFETY: bounds-checked read of a plain static table.
    unsafe { (*core::ptr::addr_of!(VECTOR_TABLE.0))[slot % VECTOR_SLOTS] }
}

/// Boot-time vector relocation (Roadmap M2).
///
/// - ARM: mirrors the flash vector entries into [`VECTOR_TABLE`] and points
///   VTOR at it. Exceptions now dispatch from SRAM.
/// - RISC-V: aims `mtvec` (direct mode) at a hang stub so unexpected traps
///   are observable instead of jumping to address 0.
///
/// # Safety
/// Run once during boot before enabling any interrupt sources.
pub unsafe fn boot_relocate_vectors() {
    #[cfg(target_arch = "arm")]
    {
        extern "C" {
            static __vector_start: u32;
            static __vector_end: u32;
        }
        let begin = core::ptr::addr_of!(__vector_start) as usize;
        let end = core::ptr::addr_of!(__vector_end) as usize;
        let words = core::cmp::min((end - begin) / 4, VECTOR_SLOTS);
        let src = begin as *const u32;
        for i in 0..words {
            // SAFETY: src spans the linker-emitted .isr_vector section;
            // dst is the aligned SRAM table with VECTOR_SLOTS capacity.
            let v = core::ptr::read_volatile(src.add(i));
            (*core::ptr::addr_of_mut!(VECTOR_TABLE.0))[i] = v;
        }
        let table_addr = core::ptr::addr_of!(VECTOR_TABLE) as usize;
        // SAFETY: VTOR is a standard Cortex-M system register; the table is
        // 1024-byte aligned (repr(align(1024))), satisfying VTOR alignment.
        core::ptr::write_volatile(VTOR as *mut u32, table_addr as u32);
    }

    #[cfg(target_arch = "riscv32")]
    {
        extern "C" {
            fn _trap_hang();
        }
        // Direct mode (mode bits = 00): all traps land on one PC.
        // SAFETY: mtvec requires 4-byte alignment; the hang stub is a
        // single 4-byte j instruction at an aligned text address.
        let base = (_trap_hang as *const ()) as usize & !0x3;
        core::arch::asm!(
            "csrw mtvec, {reg}",
            reg = in(reg) base,
        );
    }
}

/// Configure RISC-V machine trap vector in vectored mode (base | 0x1).
///
/// Provided per doc ch.4; vectored dispatch needs per-cause asm stubs, so
/// boot uses direct mode instead until IRQ bring-up lands.
///
/// # Safety
/// `table_base_address` must be 4-byte aligned and point at a valid trap
/// vector region.
#[cfg(target_arch = "riscv32")]
pub unsafe fn configure_riscv_interrupts(table_base_address: usize) {
    let mtvec_val = (table_base_address & !0x3) | 0x1;
    // SAFETY: standard CSR write; alignment enforced above.
    core::arch::asm!(
        "csrw mtvec, {reg}",
        reg = in(reg) mtvec_val,
    );
}

/// Low-level C-ABI trampoline entry point (doc ch.4 reference thunk).
///
/// Called directly from a vector table slot when IRQ line 16 fires:
/// acknowledges the peripheral pending flag, then dispatches to whatever
/// execution token is registered in slot 16 — no context save/restore,
/// shared Ring 0 address space.
#[no_mangle]
pub extern "C" fn generic_irq_trampoline_ch16() {
    // 1. Hardware acknowledge: clear pending bit 16 on the example
    //    peripheral's status register.
    // SAFETY: fixed MMIO address from the platform contract; volatile RMW
    // clears exactly bit 16.
    unsafe {
        let pending_reg = 0x4001_0004 as *mut u32;
        core::ptr::write_volatile(
            pending_reg,
            core::ptr::read_volatile(pending_reg) & !(1 << 16),
        );
    }

    // 2. Direct call: dispatch the registered execution token if present.
    // SAFETY: slot read is bounded; handler word 0 means "none". A nonzero
    // word was installed via set_handler and is a valid fn pointer.
    unsafe {
        let word = (*core::ptr::addr_of!(VECTOR_TABLE.0))[16];
        if word != 0 {
            let handler: fn() = core::mem::transmute(word as usize);
            handler();
        }
    }
}
