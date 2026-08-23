//! ExecutionBuffer — I-Cache fence, barriers & opcode emission (AXIS-4.md, FORWARD.md).
//! Invariant: peek/poke aliasing EXEC_BUFFER must not desync I-cache; emission is volatile + bounded.

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum EmitError {
    Overflow,
    BadRegister,
    Unaligned,
}

/// Executable SRAM buffer: write → fence → call. Size = 4096 (kernel/exec.rs).
pub trait ExecutionBuffer: Sized {
    const SIZE: usize = 4096;
    const ALIGN: usize = 4;

    /// Base pointer of the RWX region (linker `.sram_code`).
    fn base() -> *mut u8;
    /// Bytes written so far (cursor).
    fn len(&self) -> usize;
    /// Remaining capacity.
    fn remaining(&self) -> usize {
        Self::SIZE - self.len()
    }

    /// Emit 16-bit halfword (Thumb-2) — volatile store, bounds-checked.
    /// # Safety: caller owns buffer (single-owner contract).
    unsafe fn emit16(&mut self, hw: u16) -> Result<(), EmitError>;

    /// Emit 32-bit word (RV32I / ARM32) — volatile store.
    /// # Safety: same.
    unsafe fn emit32(&mut self, word: u32) -> Result<(), EmitError>;

    /// Data Synchronization Barrier + Instruction Synchronization.
    /// ARM: `dsb; isb`, RISC-V: `fence.i`, x86: `mfence`+`clflush`.
    /// # Safety: must follow last emit, before any `call`.
    unsafe fn flush_icache(&self);

    /// Cast buffer base (+ `offset`) to `fn()->u32` and call (with `base|1` Thumb fix on ARM).
    /// # Safety: `offset` < SIZE, buffer holds valid ISA for `target_arch`.
    unsafe fn call(&self, offset: usize) -> u32;

    /// Convenience: emit `ret` (`BX LR` / `JALR x0,0(ra)`).
    unsafe fn emit_ret(&mut self) -> Result<(), EmitError>;
}
