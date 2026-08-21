//! HOLY RUST — Ring 0 boot sequence and binary entry point.
//!
//! Boot flow (Roadmap M2):
//! 1. Reset vector: per-architecture stack setup, `.data`/`.bss` init.
//! 2. UART bring-up (115200-style console over QEMU serial).
//! 3. Vector table relocation into SRAM (VTOR on ARM / mtvec on RISC-V).
//! 4. Banner + REPL loop (never returns).

#![no_std]
#![no_main]

use holy_rust::{drivers, kernel};

/// Panic handler: report through the UART console, then park.
///
/// SAFETY-free by construction: this is the process-wide panic sink; it
/// must not itself panic, so every operation here is infallible I/O.
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    drivers::uart::write_str(b"\nPANIC: ");
    if let Some(msg) = info.message().as_str() {
        drivers::uart::write_str(msg.as_bytes());
    } else {
        drivers::uart::write_str(b"(no message)");
    }
    drivers::uart::write_str(b"\n");
    loop {
        // Wait-for-interrupt parks the core; nothing re-enables IRQs here,
        // so this is an efficient halt on both architectures.
        // SAFETY: wfi has no operands or side effects beyond parking.
        unsafe {
            #[cfg(target_arch = "arm")]
            core::arch::asm!("wfi");
            #[cfg(target_arch = "riscv32")]
            core::arch::asm!("wfi");
        }
    }
}

// ---------------------------------------------------------------------------
// Reset entry
// ---------------------------------------------------------------------------

/// ARM Cortex-M reset handler. The CPU loads SP from vector word 0 before
/// entering here, so plain Rust code can run immediately.
#[cfg(target_arch = "arm")]
#[no_mangle]
pub extern "C" fn Reset() -> ! {
    unsafe {
        // SAFETY: first code after reset; linker symbols bound the sections
        // and alignment is guaranteed by memory-layout.x.
        kernel::memory::init_data_bss();
    }
    boot()
}

/// RISC-V reset entry: naked so no prologue touches SP before we set it.
///
/// # Safety
///
/// May only run as the CPU reset vector: it assumes no valid SP exists yet
/// and establishes gp/sp from linker symbols before anything else runs.
#[cfg(target_arch = "riscv32")]
#[unsafe(naked)]
#[no_mangle]
pub unsafe extern "C" fn Reset() -> ! {
    // SAFETY: first instructions on the core; establishes gp/sp then
    // tail-calls the C-level boot continuation.
    core::arch::naked_asm!(
        ".option push",
        ".option norelax",
        "la gp, __global_pointer$",
        "la sp, _stack_top",
        ".option pop",
        "tail rust_boot_riscv",
    )
}

/// RISC-V C-level boot continuation (SP/gp valid from here on).
#[cfg(target_arch = "riscv32")]
#[no_mangle]
unsafe extern "C" fn rust_boot_riscv() -> ! {
    // SAFETY: see ARM Reset — first code after stack setup.
    kernel::memory::init_data_bss();
    boot()
}

/// Shared boot sequence after basic runtime init.
fn boot() -> ! {
    drivers::uart::init();
    drivers::uart::write_str(kernel::BANNER);
    unsafe {
        // SAFETY: runs once during boot before any interrupt source is
        // enabled; see kernel::interrupt for per-arch details.
        kernel::interrupt::boot_relocate_vectors();
    }
    drivers::repl::run()
}
