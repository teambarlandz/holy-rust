/* Holy Rust — ARM Cortex-M4F memory map (STM32F405 / QEMU netduinoplus2).
 *
 * Flash is mapped at 0x0800_0000 (aliased at 0x0000_0000 by the SoC, which
 * is how QEMU's stm32f4xx model boots it). The 64K SRAM at 0x2000_0000 is
 * carved into non-overlapping regions (explicit mappings keep lld
 * deterministic under --gc-sections):
 *
 *   0x2000_0400  vectors    relocatable trap/handler slots (doc ch.4)
 *   0x2000_1000  registry   O(1) capability bitfield (doc ch.2)
 *   0x2000_2000  sram_code  JIT execution buffer, 4K (memory.x original)
 *   0x2000_3000  sram       .data / .bss / stack; stack descends from
 *                           _stack_top = 0x2010_0000
 */

MEMORY
{
    flash (rx)     : ORIGIN = 0x08000000, LENGTH = 128K
    sram (rwx)     : ORIGIN = 0x20003000, LENGTH = 52K
    vectors (rw)   : ORIGIN = 0x20000400, LENGTH = 3K
    registry (rw)  : ORIGIN = 0x20001000, LENGTH = 256
    sram_code (rwx): ORIGIN = 0x20002000, LENGTH = 4K
}

INCLUDE memory-layout.x
