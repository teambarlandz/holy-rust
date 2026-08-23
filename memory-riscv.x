/* Holy Rust — RISC-V RV32IMAC memory map (SiFive E310 / QEMU sifive_e).
 *
 * NOTE: QEMU's sifive_e boot ROM unconditionally jumps to the flash
 * controller base 0x2040_0000 (the 0x2000_0000 window is the XIP alias),
 * so code is linked there. The DTIM at 0x8000_0000 is 8K in this machine
 * and is carved into non-overlapping regions:
 *
 *   0x8000_0000  sram       .data / .bss / stack (4K; stack descends
 *                           from _stack_top = 0x8000_1000)
 *   0x8000_1000  sram_code  JIT execution buffer (1K, DTIM)
 *   0x8000_1400  vectors    trap/handler slots
 *   0x8000_1800  registry   O(1) capability bitfield
 *
 * NOTE: EXEC_BUFFER lives in DTIM, not the ITIM window at 0x0800_0000.
 * QEMU's sifive_e model leaves the ITIM unmapped (real FE310 requires a
 * PRCI clock enable), so stores/fetches there take access faults. DTIM is
 * always-on RAM on both QEMU and silicon. On real hardware wanting the ITIM
 * back: enable its PRCI clock in boot code and restore this region.
 */

MEMORY
{
    flash (rx)     : ORIGIN = 0x20400000, LENGTH = 512K
    sram (rwx)     : ORIGIN = 0x80000000, LENGTH = 4K
    sram_code (rwx): ORIGIN = 0x80001000, LENGTH = 1K
    vectors (rw)   : ORIGIN = 0x80001400, LENGTH = 1K
    registry (rw)  : ORIGIN = 0x80001800, LENGTH = 256
}

INCLUDE memory-layout-riscv.x
