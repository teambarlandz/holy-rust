//! InterruptController — direct physical vector dispatch (<12 cycles).
//! Peripheral voltage/MSI-X → NVIC/GIC/APIC → VTOR/mtvec → SRAM handler.

/// Physical interrupt controller — no kernel IRQ thread, no IOMMU.
pub trait InterruptController: Sized {
    /// Slots in SRAM vector table (HR-OS: 256 raw, 32 typed IRQs).
    const SLOTS: usize = 32;
    /// Max dispatch latency IRQ→ISR first insn.
    const MAX_LATENCY_CYCLES: usize = 12;

    /// Relocate CPU vector base to `table` (ARM VTOR=0xE000ED08, RISC-V mtvec).
    /// # Safety: run once before enabling IRQs; table 1024-B aligned (ARM).
    unsafe fn relocate(table: *const u8);

    /// Read pending IRQ number (e.g., `ICSR &0x1FF` / `IAR`).
    fn pending() -> Option<usize>;

    /// Install `handler` at `slot` (None = disable). Atomic + DSB/ISB or fence.i.
    /// # Safety: handler is `extern "C" fn()` with interrupt ABI; lives ≥ slot lifetime.
    unsafe fn attach(slot: usize, handler: Option<unsafe extern "C" fn()>);

    /// Acknowledge & clear pending bit `slot` on peripheral status reg.
    /// # Safety: MMIO address derived from `slot` is valid.
    unsafe fn ack(slot: usize);

    /// Returns true if `slot` is an NMI (cannot be masked, WDT path).
    fn is_nmi(slot: usize) -> bool;
}
