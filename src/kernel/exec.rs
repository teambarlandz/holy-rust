//! Executable SRAM buffer and threaded-code dispatch engine.
//!
//! Two execution paths live here:
//!
//! 1. **Threaded micro-primitives** (`run_threaded_stream`): the SRAM buffer
//!    holds an array of usize words — function pointers into Flash-resident
//!    primitives interleaved with literal arguments. This is the fast path
//!    (~100 us compile) used by the REPL.
//! 2. **Native SRAM code** (`exec_buffer_entry`): emitters write real
//!    machine code (Thumb-2 / RV32I) into [`EXEC_BUFFER`] and control is
//!    transferred with a direct call.

use crate::compiler::primitives::MicroPrimitive;

/// Size of the executable SRAM region reserved by `.sram_code` in the
/// linker script.
pub const EXEC_BUFFER_SIZE: usize = 4096;

/// Writable + executable JIT buffer placed at `__sram_code_base`
/// (SRAM + 0x2000) via the `.sram_code` link section.
///
/// Wrapped in a struct to carry `#[repr(align(4))]` (repr attributes do
/// not apply to statics directly). `#[used]` + linker `KEEP()` guarantee
/// the buffer survives even when no live code path references it yet —
/// the region must exist in the final image for JIT execution.
#[repr(C, align(4))]
pub struct ExecBuffer(pub [u8; EXEC_BUFFER_SIZE]);

#[used]
#[link_section = ".sram_code"]
pub static mut EXEC_BUFFER: ExecBuffer = ExecBuffer([0; EXEC_BUFFER_SIZE]);

/// Depth of the threaded-execution operand stack.
pub const VM_STACK_SIZE: usize = 64;

static mut VM_STACK: [usize; VM_STACK_SIZE] = [0; VM_STACK_SIZE];
static mut VM_SP: usize = 0;

/// Push a value onto the threaded-execution operand stack.
///
/// Returns `false` when the stack is full; the value is dropped. The hot
/// loop must never panic, so overflow is a silent, deterministic drop.
///
/// # Safety
/// Single-threaded Ring 0 dispatch only; no reentrancy (interrupt handlers
/// must not run threaded streams).
pub unsafe fn vm_push(value: usize) -> bool {
    let sp = VM_SP;
    if sp >= VM_STACK_SIZE {
        return false;
    }
    // SAFETY: sp < VM_STACK_SIZE enforced above; single-threaded access.
    unsafe {
        (*core::ptr::addr_of_mut!(VM_STACK))[sp] = value;
    }
    VM_SP = sp + 1;
    true
}

/// Pop a value from the threaded-execution operand stack.
///
/// Returns 0 when empty (deterministic underflow, no panic).
///
/// # Safety
/// See vm_push.
pub unsafe fn vm_pop() -> usize {
    let sp = VM_SP;
    if sp == 0 {
        return 0;
    }
    VM_SP = sp - 1;
    // SAFETY: index was occupied (sp > 0); single-threaded access.
    unsafe { (*core::ptr::addr_of_mut!(VM_STACK))[VM_SP] }
}

/// Reset the operand stack pointer between REPL evaluations.
pub fn vm_reset() {
    // SAFETY: plain usize store; single-threaded REPL context.
    unsafe { VM_SP = 0 };
}

/// Current operand stack depth (diagnostics).
pub fn vm_depth() -> usize {
    // SAFETY: plain usize load; single-threaded REPL context.
    unsafe { VM_SP }
}

/// Execute a threaded token stream.
///
/// `ip` points at an array of usize words. Each step reads one word,
/// interprets it as a `MicroPrimitive` function pointer, and calls it with
/// the IP just past that word. The primitive consumes any inline arguments
/// it needs and returns the next IP. Dispatch ends when a primitive returns
/// null (see `halt_prim`).
///
/// # Safety
/// - `ip` must point to a valid, fully-initialized token stream whose words
///   are either null or valid `MicroPrimitive` code pointers (the parser
///   guarantees this layout).
/// - No interrupt handler may run a competing stream concurrently
///   (single-threaded REPL contract).
pub unsafe fn run_threaded_stream(mut ip: *const usize) {
    while !ip.is_null() {
        // Volatile fetch: the stream may have been written moments ago by
        // the emitter into SRAM; keep ordering explicit.
        let word = core::ptr::read_volatile(ip);
        if word == 0 {
            // Defensive stop on a zeroed slot (corrupt/overrun stream).
            break;
        }
        ip = ip.add(1);
        // SAFETY: word came from a parser-built stream; non-null words are
        // function pointers produced by casting MicroPrimitive fns to usize
        // (Thumb bit included automatically on ARM). usize and fn-pointer
        // have identical layout on both supported targets.
        let prim: MicroPrimitive = core::mem::transmute(word);
        ip = prim(ip);
    }
}

/// Cast the base of [`EXEC_BUFFER`] into a callable function pointer.
///
/// Used after a native emitter has written machine code into the buffer.
///
/// # Safety
/// The buffer must contain valid machine code for the running architecture
/// before the returned function is called. Calling garbage = Ring 0 fault
/// by definition; capability discipline lives one layer up.
pub unsafe fn exec_buffer_entry() -> fn() -> u32 {
    let base = core::ptr::addr_of!(EXEC_BUFFER) as *const u8 as usize;
    #[cfg(target_arch = "arm")]
    // ARM Thumb state requires the LSB set on interworking branch targets.
    // SAFETY: SRAM addresses are halfword-aligned; setting bit 0 cannot
    // corrupt a valid aligned address, only tags the instruction-set state.
    let base = base | 1;
    // SAFETY: usize <-> fn pointer transmute is layout-identical here; the
    // Thumb-bit concern is handled above for ARM.
    core::mem::transmute::<usize, fn() -> u32>(base)
}

/// Roadmap M2 helper: jump to a function whose body resides in SRAM.
///
/// # Safety
/// `func` must actually point at executable code (e.g. a value produced by
/// [`exec_buffer_entry`]).
pub unsafe fn jump_to_sram(func: fn() -> u32) -> u32 {
    func()
}

// ---------------------------------------------------------------------------
// Pipeline fence barriers (doc ch.4 Gap #4)
// ---------------------------------------------------------------------------

/// Flush the instruction cache / pipeline so recently-written SRAM code
/// is visible to the instruction fetch unit.
///
/// On ARM Cortex-M this is DSB + ISB (data synchronization barrier then
/// instruction synchronization barrier).  On RISC-V this is `fence.i`
/// (instruction-fetch fence).
///
/// Must be called after writing machine code into [`EXEC_BUFFER`] and
/// before jumping to it.
///
/// # Safety
/// Caller must ensure no data writes to EXEC_BUFFER are still in flight.
#[inline(always)]
pub unsafe fn flush_instruction_cache() {
    #[cfg(target_arch = "arm")]
    unsafe {
        core::arch::asm!("dsb", "isb", options(nostack));
    }

    #[cfg(target_arch = "riscv32")]
    unsafe {
        core::arch::asm!("fence.i", options(nostack));
    }
}

/// Execute machine code from [`EXEC_BUFFER`] at the given byte offset.
///
/// Flushes the instruction pipeline, then transmutes the buffer pointer
/// into a callable function and invokes it.
///
/// # Safety
/// - `offset` must be within [`EXEC_BUFFER_SIZE`].
/// - The buffer must contain valid, architecture-correct machine code at
///   `offset`.
/// - Calling this with garbage code = guaranteed Ring 0 fault.
pub unsafe fn execute_sram_buffer(offset: usize) -> u32 {
    flush_instruction_cache();

    let base = core::ptr::addr_of!(EXEC_BUFFER) as *const u8 as usize + offset;

    #[cfg(target_arch = "arm")]
    // SAFETY: SRAM addresses are halfword-aligned; setting bit 0 tags
    // Thumb instruction-set state without corrupting the address.
    let func_ptr: extern "C" fn() -> u32 = core::mem::transmute((base | 1) as *const ());

    #[cfg(target_arch = "riscv32")]
    let func_ptr: extern "C" fn() -> u32 = core::mem::transmute(base as *const ());

    func_ptr()
}
