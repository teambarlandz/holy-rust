/* Holy Rust — shared section layout, RISC-V variant.
 *
 * INCLUDED by memory-riscv.x, which defines the MEMORY regions: flash,
 * sram, vectors, registry, sram_code.
 *
 * No .isr_vector section: RISC-V has no hardware vector table at the flash
 * base, and QEMU's sifive_e boot ROM jumps directly to ORIGIN(flash), so
 * the Reset entry code must be the first thing there.
 */

ENTRY(Reset)

_stack_top = ORIGIN(sram) + LENGTH(sram);

SECTIONS
{
    .text : ALIGN(4)
    {
        *(.text .text.*)
        *(.rodata .rodata.*)
        *(.srodata .srodata.*)
        . = ALIGN(4);
        __etext = .;
    } > flash

    /* Relocatable trap/handler slot array (mtvec target once vectored
     * dispatch is configured). */
    .sram_vectors (NOLOAD) : ALIGN(4)
    {
        KEEP(*(.sram_vectors))
    } > vectors

    /* O(1) capability bitfield registry. */
    .capability_registry (NOLOAD) : ALIGN(4)
    {
        KEEP(*(.capability_registry))
    } > registry

    /* JIT execution buffer (writable + executable, ITIM).
     * NOLOAD keeps the section out of the ELF file; build.rs runs
     * objcopy to grant the PT_LOAD segment covering this address range
     * execute permission (LLD infers RW from the input sections). */
    .sram_code (NOLOAD) : ALIGN(4)
    {
        KEEP(*(.sram_code))
    } > sram_code

    .data : ALIGN(4)
    {
        __sdata = .;
        *(.data .data.*)
        *(.sdata .sdata.*)
        . = ALIGN(4);
        __edata = .;
    } > sram AT > flash
    __sidata = LOADADDR(.data);

    .bss (NOLOAD) : ALIGN(4)
    {
        __sbss = .;
        *(.bss .bss.*)
        *(.sbss .sbss.*)
        *(COMMON)
        . = ALIGN(4);
        __ebss = .;
    } > sram

    /* Small-data relaxation anchor (initialized in startup). */
    __global_pointer$ = __sdata + 0x800;

    /DISCARD/ :
    {
        *(.eh_frame*)
        *(.comment*)
    }
}
