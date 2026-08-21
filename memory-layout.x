/* Holy Rust — shared section layout.
 *
 * INCLUDED by memory.x (ARM) and memory-riscv.x (RISC-V), which define the
 * MEMORY regions: flash, sram, vectors, registry, sram_code.
 */

ENTRY(Reset)

_stack_top = ORIGIN(sram) + LENGTH(sram);

SECTIONS
{
    /* Hardware vector table. First two words are emitted here directly by
     * the linker: initial stack pointer and Reset entry. NOTE: ELF function
     * symbols for Thumb code already carry the Thumb bit in their value,
     * so plain LONG(Reset) yields an odd (interworking-correct) address;
     * adding +1 here would clear the bit and boot the core in ARM state.
     *
     * The remaining core exceptions (slots 2..15) route to fault_hang so a
     * wild access degrades to a visible UART stop instead of lockup; zero
     * marks architecturally-reserved slots. KEEP(*(.isr_vector)) still
     * follows, letting Rust statics append device IRQ entries. */
    .isr_vector : ALIGN(4)
    {
        __vector_start = .;
        LONG(_stack_top)
        LONG(Reset)
        LONG(fault_hang)        /*  1 NMI */
        LONG(fault_hang)        /*  2 HardFault */
        LONG(fault_hang)        /*  3 MemManage */
        LONG(fault_hang)        /*  4 BusFault */
        LONG(fault_hang)        /*  5 UsageFault */
        LONG(0)                 /*  6 reserved */
        LONG(0)                 /*  7 reserved */
        LONG(0)                 /*  8 reserved */
        LONG(0)                 /*  9 reserved */
        LONG(fault_hang)        /* 10 SVCall */
        LONG(fault_hang)        /* 11 DebugMon */
        LONG(0)                 /* 12 reserved */
        LONG(fault_hang)        /* 13 PendSV */
        LONG(fault_hang)        /* 14 SysTick */
        KEEP(*(.isr_vector))
        __vector_end = .;
    } > flash

    .text : ALIGN(4)
    {
        *(.text .text.*)
        *(.rodata .rodata.*)
        *(.srodata .srodata.*)
        . = ALIGN(4);
        __etext = .;
    } > flash

    /* Relocatable SRAM vector table (VTOR target on ARM). */
    .sram_vectors (NOLOAD) : ALIGN(4)
    {
        KEEP(*(.sram_vectors))
    } > vectors

    /* O(1) capability bitfield registry. */
    .capability_registry (NOLOAD) : ALIGN(4)
    {
        KEEP(*(.capability_registry))
    } > registry

    /* JIT execution buffer (writable + executable). */
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

    /* RISC-V small-data relaxation anchor (initialized in startup). */
    __global_pointer$ = __sdata + 0x800;

    /DISCARD/ :
    {
        *(.eh_frame*)
        *(.comment*)
    }
}
