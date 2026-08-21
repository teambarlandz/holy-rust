# CHAPTER 04: RING 0 KERNEL

## Single-Address-Space Layout

### Elimination of Memory Management Units

Holy Rust discards page tables, Translation Lookaside Buffers (TLB), and Memory
Management Units (MMU). The system operates on a Single Address Space Operating
System (SASOS) model, where virtual memory equals physical memory (1:1 identity
mapping). This eliminates the entire class of MMU-related overhead and non-determinism.

### Identity Address Space Layout (Physical Memory = Virtual Memory)

```text
0x0000_0000 ┌────────────────────────────────────────┐
            │ Vector Table & Core Exception Vectors  │
0x0000_0400 ├────────────────────────────────────────┤
            │ Holy Rust Core Runtime & Flash Storage │
0x2000_0000 ├────────────────────────────────────────┤
            │ SRAM: Static Capabilities & System RAM │
0x2000_4000 ├────────────────────────────────────────┤
            │ SRAM: Dynamic JIT Execution Buffers    │
0x4000_0000 ├────────────────────────────────────────┤
            │ Memory-Mapped I/O Peripherals (MMIO)   │
0xFFFF_FFFF └────────────────────────────────────────┘
```

### Identity Mapping Guarantees

- **Deterministic Timing**: Eliminates TLB misses and non-deterministic page-fault
  handling loops, guaranteeing microsecond-accurate real-time execution bounds
  (O(1) memory access latency). Every physical address maps to itself; there is
  no translation step.

- **Direct Bus Access**: High-speed peripherals (e.g., DMA engine, SPI controllers,
  hardware crypto accelerators) can read and write directly to JIT-allocated data
  buffers in SRAM without pointer translation or buffer copying. The DMA engine
  programs physical addresses directly; the CPU uses the same physical addresses.

- **Compact Footprint**: Removing page table metadata reclaims hundreds of kilobytes
  of SRAM. A typical Linux page table structure for 4 KB pages over 1 GB of address
  space requires several megabytes of page table entries. Holy Rust on 64 KB SRAM
  requires zero page table entries.

```rust
// Example: Linker script memory map for Holy Rust
// memory.x

MEMORY
{
    flash (rx) : ORIGIN = 0x0800_0000, LENGTH = 128K
    sram (rwx) : ORIGIN = 0x2000_0000, LENGTH = 64K
    vector_table (rwx) : ORIGIN = 0x2000_0400, LENGTH = 1K
}
```

## Interrupt Routing & Trampolines

### Elimination of Kernel Interrupt Dispatch Pipeline

In standard operating systems, a hardware interrupt causes the CPU to enter kernel
space, save userland context, evaluate the device driver stack, process the interrupt
service routine (ISR), schedule thread rescheduling, and perform a reverse context
switch back to Ring 3. This pipeline typically incurs 200-500 CPU cycle latency.

Holy Rust replaces this pipeline by routing hardware interrupt vectors directly
into JIT-compiled execution routines using C-ABI trampoline blocks ("Thunk" pattern).
The total interrupt dispatch latency is less than 12 CPU cycles.

### Interrupt Handling Sequence

```text
[ Hardware IRQ Signal ]
            │
            ▼ (CPU Vector Lookup ~12 cycles)
[ SRAM Vector Table Slot ]
            │
            ▼
[ C-ABI Trampoline Thunk ] ──► (Clears Interrupt Pending Flag)
            │
            ▼
[ JIT-Compiled Execution Token ] ──► (Executes Task Code in Ring 0)
```

### Interrupt Vector Table Configuration

Microcontrollers (ARM Cortex-M, RISC-V PLIC/CLIC) resolve hardware interrupt
handlers via a vector table pointer in memory. At boot, Holy Rust initializes a
RAM-backed Vector Table to allow dynamic vector overwrites during shell sessions.

#### ARM Cortex-M Vector Relocation

On ARM Cortex-M architectures, the Vector Table Offset Register (VTOR) is configured
to point directly to an aligned SRAM array. This allows the vector table to be
modified at runtime during shell sessions.

```rust
// Static vector table aligned in SRAM
#[link_section = ".sram_vectors"]
pub static mut VECTOR_TABLE: [Option<fn()>; 256] = [None; 256];

/// Relocate the vector table to a new SRAM address
pub unsafe fn relocate_vector_table(table_address: usize) {
    // Write SRAM Vector Table address to VTOR register (0xE000_ED08)
    let vtor = 0xE000_ED08 as *mut usize;
    core::ptr::write_volatile(vtor, table_address);
}
```

#### RISC-V Trap Vector Configuration

On RISC-V architectures, the mtvec (Machine Trap-Vector Base Address) control and
status register is set to Vectored Mode (Mode bits = 01).

```rust
/// Configure RISC-V machine trap vectors in vectored mode
pub unsafe fn configure_riscv_interrupts(table_base_address: usize) {
    // Set base address with Vectored Mode flag (Bit 0 = 1)
    // Mode: 01 = Direct Vector Mode
    let mtvec_val = (table_base_address & !0x3) | 0x1;
    core::arch::asm!(
        "csrw mtvec, {reg}",
        reg = in(reg) mtvec_val,
    );
}
```

### Trampoline Architecture (The C-ABI Thunk Interface)

Because a hardware interrupt controller executes raw CPU instructions without passing
context state or environment pointers, JIT-compiled closures cannot be attached directly
to raw vector addresses. Holy Rust uses static C-ABI trampoline functions to bridge
low-level silicon interrupts to JIT execution routines.

```rust
// Storage for dynamic execution token pointers in SRAM
static mut INTERRUPT_SLOTS: [Option<fn()>; 64] = [None; 64];

/// Low-level C-ABI Trampoline Entry Point
///
/// This function is called directly from the vector table when an interrupt fires.
/// It performs the minimal necessary operations:
/// 1. Acknowledge the interrupt (clear pending flag)
/// 2. Dispatch to the registered JIT execution token if present
/// 3. Return - no context save/restore needed (shared Ring 0 address space)
#[no_mangle]
pub extern "C" fn generic_irq_trampoline_ch16() {
    // 1. Hardware Acknowledge: Clear the pending bit on the peripheral register
    unsafe {
        let pending_reg = 0x4001_0004 as *mut u32;
        core::ptr::write_volatile(
            pending_reg,
            core::ptr::read_volatile(pending_reg) & !(1 << 16)
        );
    }

    // 2. Direct Call: Dispatch the registered execution token if present
    unsafe {
        if let Some(handler) = INTERRUPT_SLOTS[16] {
            // No context save/restore needed - shared Ring 0 address space
            handler();
        }
    }
}
```

### System Performance Comparison

| Attribute                | Standard OS (Linux / RTOS) | Holy Rust (Ring 0 SASOS) |
|---|---|---|
| **Execution Privilege** | Ring 3 (User) / Ring 0 (Kernel) | Ring 0 Only |
| **Address Translation** | MMU Page Tables (4 KB Pages) | 1:1 Identity Mapping |
| **Hardware Access Cost** | ~100--1,000 CPU Cycles (Syscall) | 0--1 CPU Cycles (Direct MMIO) |
| **Interrupt Dispatch Latency** | ~200--500 CPU Cycles | < 12 CPU Cycles (Hardware Push + Thunk) |
| **Context Switching Overhead** | High (Register Dumps + Page Swap) | Zero (Shared Context) |
| **Real-Time Guarantees** | Scheduler-dependent (nondeterministic) | O(1) deterministic (no scheduler) |