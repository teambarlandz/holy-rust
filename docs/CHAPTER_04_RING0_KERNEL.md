# CHAPTER 04: RING 0 KERNEL

## 4.1 Ring 0 Execution Model
Traditional operating systems divide execution into privilege rings: Ring 3 for untrusted user applications and Ring 0 for the privileged kernel. Crossing this boundary requires hardware-enforced context switches (via system call instructions like syscall or svc). These switches invalidate CPU pipeline states, swap stack pointers, and incur a mandatory performance tax of 100 to 1,000 CPU cycles per invocation.

### Traditional OS Architecture (Multi-Ring / Syscall Tax)
```text
┌──────────────────────────────────────────────────────────┐
│ Userland Application (Ring 3)                            │
└────────────────────────────┬─────────────────────────────┘
                              │ Hardware Trap (syscall / svc)
                              ▼ ~100-1,000 CPU Clock Cycles
┌──────────────────────────────────────────────────────────┐
│ Kernel & Drivers (Ring 0)                                │
└──────────────────────────────────────────────────────────┘
```

### Holy Rust Architecture (Single-Ring / Direct Execution)
```text
┌──────────────────────────────────────────────────────────┐
│ Ring 0: User Script ──► JIT Engine ──► Memory / Silicon  │
│ Execution Latency: 0-1 CPU Cycles (Direct Memory Write)  │
└──────────────────────────────────────────────────────────┘
```

Holy Rust completely eliminates Ring 3. The entire environment—the live REPL, the capability verifier, the streaming JIT compiler, and all user-submitted code—executes exclusively within Ring 0.

#### 1. Eliminating the Privilege Boundary
Because memory safety and peripheral access rights are mathematically proven by the capability engine before execution, hardware-enforced privilege rings are redundant. User-submitted code cannot manufacture unauthorized memory handles or access unassigned peripheral registers.

#### 2. Zero-Call-Overhead Systems Programming
Hardware access in Holy Rust is a direct register mutation or inline pointer dereference. Invoking a driver operation translates to a standard function call, executing in a single CPU cycle rather than triggering a hardware interrupt or kernel trap.

## 4.2 Linear Physical Memory Mapping
Holy Rust discards page tables, Translation Lookaside Buffers (TLB), and Memory Management Units (MMU). The system operates on a Single Address Space Operating System (SASOS) model, where virtual memory equals physical memory (1:1 identity mapping).

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
- **Deterministic Timing**: Eliminates TLB misses and non-deterministic page-fault handling loops, guaranteeing microsecond-accurate real-time execution bounds (O(1) memory access latency).
- **Direct Bus Access**: High-speed peripherals (e.g., DMA engine, SPI controllers, hardware crypto accelerators) can read and write directly to JIT-allocated data buffers in SRAM without pointer translation or buffer copying.
- **Compact Footprint**: Removing page table metadata reclaims hundreds of kilobytes of SRAM, allowing Holy Rust to run on resource-constrained microcontrollers with as little as 32 KB of total memory.

## 4.3 Direct Hardware Interrupt Routing
In standard operating systems, a hardware interrupt causes the CPU to enter kernel space, save userland context, evaluate the device driver stack, process the interrupt service routine (ISR), schedule thread rescheduling, and perform a reverse context switch back to Ring 3.

Holy Rust replaces this pipeline by routing hardware interrupt vectors directly into JIT-compiled execution routines using C-ABI trampoline blocks ("Thunk" pattern).

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
Microcontrollers (ARM Cortex-M, RISC-V PLIC/CLIC) resolve hardware interrupt handlers via a vector table pointer in memory. At boot, Holy Rust initializes a RAM-backed Vector Table to allow dynamic vector overwrites during shell sessions.

#### ARM Cortex-M Vector Relocation
On ARM Cortex-M architectures, the Vector Table Offset Register (VTOR) is configured to point directly to an aligned SRAM array.

```rust
// Static vector table aligned in SRAM
#[link_section = ".sram_vectors"]
pub static mut VECTOR_TABLE: [option<fn()>; 256] = [None; 256];

pub unsafe fn relocate_vector_table(table_address: usize) {
    // Write SRAM Vector Table address to VTOR register (0xE000_ED08)
    let vtor = 0xE000_ED08 as *mut usize;
    core::ptr::write_volatile(vtor, table_address);
}
```

#### RISC-V Trap Vector Configuration
On RISC-V architectures, the mtvec (Machine Trap-Vector Base Address) control and status register is set to Vectored Mode (Mode bits = 01).

```rust
pub unsafe fn configure_riscv_interrupts(table_base_address: usize) {
    // Set base address with Vectored Mode flag (Bit 0 = 1)
    let mtvec_val = (table_base_address & !0x3) | 0x1;
    core::arch::asm!(
        "csrw mtvec, {reg}",
        reg = in(reg) mtvec_val,
    );
}
```

## 4.4 Trampoline Architecture (The C-ABI Thunk Interface)
Because a hardware interrupt controller executes raw CPU instructions without passing context state or environment pointers, JIT-compiled closures cannot be attached directly to raw vector addresses. Holy Rust uses static C-ABI trampoline functions to bridge low-level silicon interrupts to JIT execution routines.

```rust
// Storage for dynamic execution token pointers in SRAM
static mut INTERRUPT_SLOTS: [Option<fn()>; 64] = [None; 64];

// Low-level C-ABI Trampoline Entry Point
#[no_mangle]
pub extern "C" fn generic_irq_trampoline_ch16() {
    // 1. Hardware Acknowledge: Clear the pending bit on the peripheral register
    unsafe {
        let pending_reg = 0x4001_0004 as *mut u32;
        core::ptr::write_volatile(pending_reg, core::ptr::read_volatile(pending_reg) & !(1 << 16));
    }

    // 2. Direct Call: Dispatch the registered execution token if present
    unsafe {
        if let Some(handler) = INTERRUPT_SLOTS[16] {
            handler();
        }
    }
}
```

### System Performance Comparison

| Attribute                | Standard OS (Linux / RTOS) | Holy Rust (Ring 0 SASOS) |
|---|---|---|
| Execution Privilege | Ring 3 (User) / Ring 0 (Kernel) | Ring 0 Only |
| Address Translation | MMU Page Tables (4 KB Pages) | 1:1 Identity Mapping |
| Hardware Access Cost | ~100--1,000 CPU Cycles (Syscall) | 0--1 CPU Cycles (Direct MMIO) |
| Interrupt Dispatch Latency | ~200--500 CPU Cycles | < 12 CPU Cycles (Hardware Push + Thunk) |
| Context Switching Overhead | High (Register Dumps + Page Swap) | Zero (Shared Context) |