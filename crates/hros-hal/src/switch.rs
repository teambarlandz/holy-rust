//! ContextSwitch — 43-cycle deterministic switch (AXIS-1.md, BENCHMARK.md).
//! Invariant Φ: S × T_old × T_new → S' bijection. No TLB flush, no mutex.

/// Deterministic context switch mechanics. Every impl must uphold
/// `TOTAL_CYCLES == 43` at 168 MHz (0.51 µs ±0 jitter, SASA → no TLB flush).
pub trait ContextSwitch: Sized {
    /// Total bounded cycles for a full switch (incl. HW auto-stack).
    const TOTAL_CYCLES: usize = 43;
    /// HW auto-stack cycles (xPSR/PC/LR/R12/R3-R0).
    const CYCLES_AUTO_STACK: usize = 12;
    /// SW callee-save push (R4-R11).
    const CYCLES_MANUAL_PUSH: usize = 8;
    /// Scheduler pointer bump.
    const CYCLES_SCHED: usize = 3;
    /// SW pop + auto-unstack
    const CYCLES_RESTORE: usize = 20; // 8 + 12

    /// Callee-saved register block. Repr C for asm `stm/ldm` layout parity.
    type Frame: Copy;

    /// Save `R4-R11` onto `*sp` (SP descending full). Returns new SP.
    /// # Safety: `sp` points inside `[SP_limit,SP_base]` of `T_old`.
    unsafe fn save_callee(sp: *mut u8) -> *mut u8;

    /// Restore `R4-R11` from `*sp`. Returns new SP.
    /// # Safety: `sp` points at a frame written by `save_callee`.
    unsafe fn restore_callee(sp: *const u8) -> *const u8;

    /// Atomically advance circular/priority queue head. Pure `AtomicUsize::CAS` (no mutex).
    /// Returns next task index. Must be 3 cycles worst-case.
    fn next_task(current: usize, len: usize) -> usize;

    /// Full switch: save `old_sp` → slot `old`, load `new` slot → `sp`, restore.
    /// # Safety: single-core critical section or CAS-protected; both frames valid.
    unsafe fn switch(current_sp: *mut *mut u8, next_sp: *const u8);
}
