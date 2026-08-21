//! Native code generation from threaded token streams.
//!
//! Compiles a small subset of [`StreamProgram`] layouts into real machine
//! code (Thumb-2 / RV32I) and executes via [`exec_buffer_entry`]. This is
//! the Milestone 4 "JIT" path — for expressions too simple to justify the
//! threaded dispatch overhead.
//!
//! # Limitations (v1)
//!
//! The compiler uses a **two-register** strategy (accumulator + scratch)
//! which handles all single-level binary expressions. Nested expressions
//! like `2 + 3 * 4` fall back to the threaded interpreter; only streams
//! matching the pattern `lit [op lit]* [load|store] halt` are compiled.
//!
//! # Register allocation
//!
//! | Target  | Accumulator | Scratch | Return |
//! |---------|-------------|---------|--------|
//! | ARM     | r0          | r1      | r0     |
//! | RISC-V  | a0 (x10)    | a1 (x11)| a0     |

#[cfg(not(target_arch = "riscv32"))]
use crate::compiler::emitter::TargetEmitter;
#[cfg(not(target_arch = "riscv32"))]
use crate::compiler::primitives;
#[cfg(not(target_arch = "riscv32"))]
use crate::kernel::exec::{exec_buffer_entry, jump_to_sram};

/// Target-specific accumulator and scratch register numbers.
#[cfg(target_arch = "arm")]
#[cfg(not(target_arch = "riscv32"))]
mod regs {
    pub const ACC: u8 = 0; // r0
    pub const SCRATCH: u8 = 1; // r1
}

/// Compile a threaded token stream into native code in EXEC_BUFFER and
/// execute it.
///
/// Returns `Some(result)` when the stream yields a value (the return
/// register holds it), or `None` for side-effect-only streams (poke, etc.).
///
/// Returns `Err(())` when the stream is too complex for the two-register
/// compiler — the caller should fall back to `run_threaded_stream`.
///
/// # Safety
/// Overwrites EXEC_BUFFER (single-owner contract). The stream must be
/// valid and fully-initialized. Interrupt handlers must not touch
/// EXEC_BUFFER concurrently.
#[allow(clippy::result_unit_err)]
pub unsafe fn compile_and_run(
    stream: &[usize; crate::compiler::parser::MAX_STREAM_WORDS],
    len: usize,
    yields_value: bool,
) -> Result<Option<u32>, ()> {
    // RISC-V: LLD emits the ITIM section in a RW-only PT_LOAD segment;
    // QEMU's sifive_e enforces execute-permission on PT_LOAD, so native
    // codegen faults.  Fall back to threaded until the ELF is patched
    // (see build.rs post-link step, or switch to GNU ld).
    #[cfg(target_arch = "riscv32")]
    {
        let _ = (stream, len, yields_value);
        Err(())
    }

    #[cfg(not(target_arch = "riscv32"))]
    {
        // Quick complexity check: scan for illegal patterns.
        if !is_compilable(stream, len) {
            return Err(());
        }

        #[cfg(target_arch = "arm")]
        let mut em = unsafe { crate::compiler::emitter::Thumb2Emitter::into_exec_buffer() };
        #[cfg(target_arch = "riscv32")]
        let mut em = unsafe { crate::compiler::emitter::Riscv32Emitter::into_exec_buffer() };

        let mut ip = 0;
        let mut acc_loaded = false;

        while ip < len {
            let word = stream[ip];
            ip += 1;

            if word == 0 {
                break; // defensive stop
            }

            if word == word_of(primitives::halt_prim) {
                break;
            }

            if word == word_of(primitives::lit_prim) {
                if ip >= len {
                    return Err(());
                }
                let val = stream[ip] as u32;
                ip += 1;

                // Look ahead to decide which register receives this literal.
                // Peek/poke need the address in SCRATCH; binary ops need the
                // first operand in ACC.
                let next = if ip < len { stream[ip] } else { 0 };
                let addr_context = next == word_of(primitives::load_reg_prim)
                    || next == word_of(primitives::write_reg_prim);

                if addr_context {
                    // Address goes to SCRATCH (load/store read from there).
                    em.emit_mov_imm(regs::SCRATCH, val).map_err(|_| ())?;
                } else if !acc_loaded {
                    em.emit_mov_imm(regs::ACC, val).map_err(|_| ())?;
                    acc_loaded = true;
                } else {
                    em.emit_mov_imm(regs::SCRATCH, val).map_err(|_| ())?;
                }
            } else if word == word_of(primitives::add_prim) {
                em.emit_add(regs::ACC, regs::ACC, regs::SCRATCH)
                    .map_err(|_| ())?;
                acc_loaded = true;
            } else if word == word_of(primitives::sub_prim) {
                em.emit_sub(regs::ACC, regs::ACC, regs::SCRATCH)
                    .map_err(|_| ())?;
                acc_loaded = true;
            } else if word == word_of(primitives::mul_prim) {
                em.emit_mul(regs::ACC, regs::ACC, regs::SCRATCH)
                    .map_err(|_| ())?;
                acc_loaded = true;
            } else if word == word_of(primitives::div_prim) {
                em.emit_div(regs::ACC, regs::ACC, regs::SCRATCH)
                    .map_err(|_| ())?;
                acc_loaded = true;
            } else if word == word_of(primitives::load_reg_prim) {
                // SCRATCH holds the address; load *SCRATCH → ACC.
                em.emit_load_u32(regs::ACC, regs::SCRATCH).map_err(|_| ())?;
                acc_loaded = true;
            } else if word == word_of(primitives::write_reg_prim) {
                // ACC holds the address (from first lit), SCRATCH holds the
                // value (from second lit).  Store SCRATCH → *ACC.
                em.emit_store_u32(regs::SCRATCH, regs::ACC)
                    .map_err(|_| ())?;
            } else {
                // Unknown primitive — not compilable.
                return Err(());
            }
        }

        // Ensure there's a return instruction.
        em.emit_ret().map_err(|_| ())?;

        // Execute.
        let entry = exec_buffer_entry();
        let result = jump_to_sram(entry);

        if yields_value {
            Ok(Some(result))
        } else {
            Ok(None)
        }
    }
}

/// Check whether a threaded stream matches the compilable pattern:
/// `lit [op lit]* [load|store] halt` using only known primitives.
#[cfg(not(target_arch = "riscv32"))]
fn is_compilable(stream: &[usize; crate::compiler::parser::MAX_STREAM_WORDS], len: usize) -> bool {
    let lit_w = word_of(primitives::lit_prim);
    let halt_w = word_of(primitives::halt_prim);
    let add_w = word_of(primitives::add_prim);
    let sub_w = word_of(primitives::sub_prim);
    let mul_w = word_of(primitives::mul_prim);
    let div_w = word_of(primitives::div_prim);
    let load_w = word_of(primitives::load_reg_prim);
    let write_w = word_of(primitives::write_reg_prim);

    let mut ip = 0;
    let mut lit_seen = false;

    while ip < len {
        let w = stream[ip];
        if w == 0 || w == halt_w {
            return true;
        }
        if w == lit_w {
            ip += 2; // skip lit + its argument
            lit_seen = true;
            continue;
        }
        if w == add_w || w == sub_w || w == mul_w || w == div_w || w == load_w || w == write_w {
            lit_seen = true; // operation after at least one lit
            ip += 1;
            continue;
        }
        return false; // unknown word
    }
    lit_seen
}

/// Cast a [`MicroPrimitive`] function pointer to `usize` for comparison.
#[cfg(not(target_arch = "riscv32"))]
fn word_of(f: primitives::MicroPrimitive) -> usize {
    f as usize
}
