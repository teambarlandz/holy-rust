//! Dynamic vector table relocation and C-ABI interrupt trampolines.
//!
//! Holy Rust routes hardware interrupts straight into SRAM-resident handler
//! slots (< 12 cycle dispatch target, doc ch.4). At boot the flash vector
//! table is mirrored into SRAM and the CPU's vector base register is
//! repointed there, so REPL sessions can overwrite handlers at runtime.
//!
//! The typed [`VectorTable`] struct provides `attach_jit_irq()` for
//! atomically wiring an IRQ slot to a JIT-compiled function address.

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

// ---------------------------------------------------------------------------
// Typed relocatable vector table (doc ch.4)
// ---------------------------------------------------------------------------

/// Number of dispatch slots in the SRAM vector table.
pub const VECTOR_SLOTS: usize = 256;

/// Typed, relocatable vector table in SRAM.
///
/// Laid out exactly as the ARM Cortex-M hardware expects: SP, Reset, NMI,
/// HardFault, MemManage, BusFault, UsageFault, reserved, SVCall, DebugMon,
/// reserved, PendSV, SysTick, then 32 external IRQ slots.
///
/// `irq_handlers` are `Option<extern "C" fn()>` — `None` means "no handler
/// installed".  The low-level assembly trampoline reads the raw word and
/// dispatches only when non-zero.
#[repr(C, align(1024))]
pub struct VectorTable {
    pub initial_sp: u32,
    pub reset_handler: unsafe extern "C" fn() -> !,
    pub nmi_handler: unsafe extern "C" fn(),
    pub hard_fault: unsafe extern "C" fn(),
    pub mem_manage: unsafe extern "C" fn(),
    pub bus_fault: unsafe extern "C" fn(),
    pub usage_fault: unsafe extern "C" fn(),
    pub reserved: [u32; 4],
    pub sv_call: unsafe extern "C" fn(),
    pub debug_mon: unsafe extern "C" fn(),
    pub reserved2: u32,
    pub pend_sv: unsafe extern "C" fn(),
    pub sys_tick: unsafe extern "C" fn(),
    pub irq_handlers: [Option<unsafe extern "C" fn()>; 32],
}

/// SRAM-resident vector table placed at `__sram_vectors_base`
/// (SRAM + 0x400) via the `.sram_vectors` link section.
#[used]
#[link_section = ".sram_vectors"]
pub static mut RAM_VECTOR_TABLE: VectorTable = VectorTable {
    initial_sp: 0,
    reset_handler: default_reset,
    nmi_handler: default_handler,
    hard_fault: default_handler,
    mem_manage: default_handler,
    bus_fault: default_handler,
    usage_fault: default_handler,
    reserved: [0; 4],
    sv_call: default_handler,
    debug_mon: default_handler,
    reserved2: 0,
    pend_sv: default_handler,
    sys_tick: default_handler,
    irq_handlers: [None; 32],
};

/// Relocate the CPU's vector base to [`RAM_VECTOR_TABLE`].
///
/// # Safety
/// Must run once during boot, before any interrupt source is enabled.
pub unsafe fn relocate_vector_table() {
    #[cfg(target_arch = "arm")]
    {
        let vtor = 0xE000_ED08 as *mut u32;
        let table_addr = &raw const RAM_VECTOR_TABLE as u32;
        // SAFETY: VTOR is a standard Cortex-M system register; the table
        // is 1024-byte aligned (repr(align(1024))), satisfying VTOR alignment.
        core::ptr::write_volatile(vtor, table_addr);
        core::arch::asm!("dsb", "isb", options(nostack));
    }
}

/// Atomically attach a JIT-compiled function to IRQ slot `irq_index`.
///
/// The handler must have C-ABI linkage (`extern "C"`) and follow the
/// interrupt contract: no stack frame setup, direct register access only.
///
/// # Safety
/// - `irq_index` must be < 32.
/// - `jit_fn` must be a valid interrupt handler (e.g. code in EXEC_BUFFER
///   or a Flash-resident trampoline).
pub unsafe fn attach_jit_irq(irq_index: usize, jit_fn: extern "C" fn()) {
    if irq_index < 32 {
        // SAFETY: irq_index < 32 enforced above; single-threaded REPL.
        unsafe {
            RAM_VECTOR_TABLE.irq_handlers[irq_index] = Some(jit_fn);
        }
        #[cfg(target_arch = "arm")]
        unsafe {
            core::arch::asm!("dsb", "isb", options(nostack));
        }
    }
}

/// Default reset handler (busy loop; the real Reset is in main.rs).
unsafe extern "C" fn default_reset() -> ! {
    loop {
        #[cfg(target_arch = "arm")]
        unsafe {
            core::arch::asm!("wfi")
        }
        #[cfg(target_arch = "riscv32")]
        unsafe {
            core::arch::asm!("wfi")
        }
    }
}

/// Default handler for unregistered exceptions (busy loop).
unsafe extern "C" fn default_handler() {
    loop {
        #[cfg(target_arch = "arm")]
        unsafe {
            core::arch::asm!("wfi")
        }
        #[cfg(target_arch = "riscv32")]
        unsafe {
            core::arch::asm!("wfi")
        }
    }
}

// ---------------------------------------------------------------------------
// Legacy API removed — all code now uses typed RAM_VECTOR_TABLE above.
// ---------------------------------------------------------------------------

/// ARM Cortex-M Vector Table Offset Register.
#[cfg(target_arch = "arm")]
const VTOR: usize = 0xE000_ED08;

/// Boot-time vector relocation (Roadmap M2).
///
/// - ARM: mirrors the flash vector entries into [`RAM_VECTOR_TABLE`] and
///   points VTOR at it. Exceptions now dispatch from SRAM.
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

        // Copy flash vector table words into the typed RAM table.
        // The typed struct starts at the same offset as the raw table,
        // so we can write through a raw pointer cast.
        let dst = core::ptr::addr_of_mut!(RAM_VECTOR_TABLE) as *mut u32;
        for i in 0..words {
            let v = core::ptr::read_volatile(src.add(i));
            core::ptr::write_volatile(dst.add(i), v);
        }
        let table_addr = core::ptr::addr_of!(RAM_VECTOR_TABLE) as u32;
        // SAFETY: VTOR is a standard Cortex-M system register; the table is
        // 1024-byte aligned (repr(align(1024))), satisfying VTOR alignment.
        core::ptr::write_volatile(VTOR as *mut u32, table_addr);
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
    // SAFETY: slot 16 is within bounds; handler None means "none". A Some
    // value was installed via attach_jit_irq and is a valid fn pointer.
    unsafe {
        if let Some(handler) = RAM_VECTOR_TABLE.irq_handlers[16] {
            handler();
        }
    }
}
