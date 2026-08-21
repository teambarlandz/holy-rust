//! Threaded micro-primitives: the Flash-resident execution atoms.
//!
//! The parser emits arrays of `usize` words — function pointers to these
//! primitives interleaved with inline literal arguments (direct threading,
//! doc ch.3). Every primitive shares the C-ABI-ish signature
//! `fn(ip: *const usize) -> *const usize`: it consumes its arguments from
//! the token stream via `ip` and returns the next instruction pointer.

use crate::kernel::exec::{vm_pop, vm_push};

/// Micro-primitive signature: takes IP past the opcode word, returns next IP.
pub type MicroPrimitive = unsafe fn(ip: *const usize) -> *const usize;

/// Push the following word as a literal operand.
///
/// # Safety
///
/// `ip` must point at an inline-argument slot of a parser-built token
/// stream; volatile read keeps the freshly-written SRAM fetch explicit.
pub unsafe fn lit_prim(ip: *const usize) -> *const usize {
    let v = core::ptr::read_volatile(ip);
    vm_push(v);
    ip.add(1)
}

/// Pop `addr`, read `*addr`, push the value.
///
/// Pure stack semantics: the address arrives on the operand stack (pushed
/// by [`lit_prim`]), keeping every primitive argument-free and the token
/// stream uniform RPN.
///
/// # Safety
///
/// `ip` must point at a valid opcode slot of a parser-built stream; `addr`
/// popped from the operand stack must be a readable word address.
pub unsafe fn load_reg_prim(ip: *const usize) -> *const usize {
    let addr = vm_pop();
    let value = core::ptr::read_volatile(addr as *const usize);
    vm_push(value);
    ip
}

/// Pop `value`, pop `addr`, write `*addr = value`.
///
/// # Safety
///
/// `ip` must point at a valid opcode slot of a parser-built stream; `addr`
/// popped from the operand stack must be a writable word address.
pub unsafe fn write_reg_prim(ip: *const usize) -> *const usize {
    let value = vm_pop();
    let addr = vm_pop();
    core::ptr::write_volatile(addr as *mut usize, value);
    ip
}

/// Pop b, pop a, push `a + b`.
///
/// # Safety
///
/// `ip` must point at a valid opcode slot of a parser-built stream.
pub unsafe fn add_prim(ip: *const usize) -> *const usize {
    let b = vm_pop();
    let a = vm_pop();
    vm_push(a.wrapping_add(b));
    ip
}

/// Pop b, pop a, push `a - b`.
///
/// # Safety
///
/// `ip` must point at a valid opcode slot of a parser-built stream.
pub unsafe fn sub_prim(ip: *const usize) -> *const usize {
    let b = vm_pop();
    let a = vm_pop();
    vm_push(a.wrapping_sub(b));
    ip
}

/// Pop b, pop a, push `a * b`.
///
/// # Safety
///
/// `ip` must point at a valid opcode slot of a parser-built stream.
pub unsafe fn mul_prim(ip: *const usize) -> *const usize {
    let b = vm_pop();
    let a = vm_pop();
    vm_push(a.wrapping_mul(b));
    ip
}

/// Pop b, pop a, push `a / b` (0 when b == 0 — deterministic, no traps in
/// Ring 0 threaded mode).
///
/// # Safety
///
/// `ip` must point at a valid opcode slot of a parser-built stream.
pub unsafe fn div_prim(ip: *const usize) -> *const usize {
    let b = vm_pop();
    let a = vm_pop();
    let q = a.checked_div(b).unwrap_or(0);
    vm_push(q);
    ip
}

/// Terminate dispatch: returning null stops `run_threaded_stream`.
///
/// # Safety
///
/// Safe on any `ip`; the argument is ignored.
pub unsafe fn halt_prim(_ip: *const usize) -> *const usize {
    core::ptr::null()
}
