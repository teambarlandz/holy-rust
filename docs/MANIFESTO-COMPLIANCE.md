================================================================================
HOLY RUST: MANIFESTO COMPLIANCE ACTION PLAN & TECHNICAL SPECIFICATION
================================================================================
Goal: Transition Holy Rust from a proof-of-concept into a fully enforced,
100% compliant Ring-0 bare-metal interactive operating environment.

This document contains explicit, step-by-step actionable code and configuration
modifications to close all remaining architectural gaps across both ARM Cortex-M
and RISC-V targets.

--------------------------------------------------------------------------------
TABLE OF CONTENTS
--------------------------------------------------------------------------------
1. MANDATORY CAPABILITY ENFORCEMENT (Closing Gap #1)
2. SUPERUSER AUDIT LOG WIRING (Closing Gap #2)
3. DYNAMIC INTERRUPT ROUTING TO JIT CLOSURES (Closing Gap #3)
4. RISC-V EXECUTION PERMISSION & CACHE FENCING (Closing Gap #4)
5. ARM BINARY SIZE REDUCTION TO < 64 KB (Closing Gap #5)
6. PARSER INTEGRATION & END-TO-END VERIFICATION

================================================================================
1. MANDATORY CAPABILITY ENFORCEMENT (Closing Gap #1)
================================================================================
Objective: Prevent raw `poke` / `peek` operations unless the target memory 
region's capability token is explicitly claimed in the $O(1)$ SRAM registry.

ACTION 1.1: Create Memory-to-Capability Mapper in `src/capabilities/registry.rs`

```rust
// File: src/capabilities/registry.rs

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapId {
    GpioA = 0,
    GpioB = 1,
    Uart1 = 2,
    Uart2 = 3,
    Spi1  = 4,
    Tim1  = 5,
}

/// O(1) Physical Address to Capability ID Resolution
#[inline(always)]
pub fn addr_to_cap_id(addr: u32) -> Option<CapId> {
    match addr {
        // STM32 / Standard Cortex-M MMIO ranges (Adjust addresses per PAC)
        0x4002_0000..=0x4002_03FF => Some(CapId::GpioA),
        0x4002_0400..=0x4002_07FF => Some(CapId::GpioB),
        0x4001_1000..=0x4001_13FF => Some(CapId::Uart1),
        0x4000_4400..=0x4000_47FF => Some(CapId::Uart2),
        0x4001_3000..=0x4001_33FF => Some(CapId::Spi1),
        0x4001_2C00..=0x4001_2FFF => Some(CapId::Tim1),
        _ => None, // Unmapped regions require active SuperUserCap
    }
}

ACTION 1.2: Intercept MMIO Execution in src/kernel/memory.rs
// File: src/kernel/memory.rs
use crate::capabilities::registry::{addr_to_cap_id, CAPABILITY_REGISTRY, SUPERUSER_AUDIT_LOG};

pub fn enforced_poke_u32(addr: u32, value: u32) -> Result<(), &'static str> {
    if let Some(cap_id) = addr_to_cap_id(addr) {
        // O(1) Atomic Bitfield Verification
        if !CAPABILITY_REGISTRY.is_claimed(cap_id as usize) {
            return Err("E001: CAPABILITY_VIOLATION - Peripheral token not claimed");
        }
    } else {
        // Unmapped hardware regions MUST hold SuperUserCap
        if !CAPABILITY_REGISTRY.is_superuser_active() {
            return Err("E002: PERMISSION_DENIED - Unmapped MMIO access requires SuperUserCap");
        }
        // Record all raw, non-capability MMIO access
        unsafe { SUPERUSER_AUDIT_LOG.record_event(addr, value) };
    }

    // SAFETY: Address hardware safety verified by capability registry checks above
    unsafe { core::ptr::write_volatile(addr as *mut u32, value) };
    Ok(())
}

pub fn enforced_peek_u32(addr: u32) -> Result<u32, &'static str> {
    if let Some(cap_id) = addr_to_cap_id(addr) {
        if !CAPABILITY_REGISTRY.is_claimed(cap_id as usize) {
            return Err("E001: CAPABILITY_VIOLATION - Peripheral token not claimed");
        }
    } else {
        if !CAPABILITY_REGISTRY.is_superuser_active() {
            return Err("E002: PERMISSION_DENIED - Unmapped MMIO access requires SuperUserCap");
        }
    }

    // SAFETY: Verified safe access
    Ok(unsafe { core::ptr::read_volatile(addr as *const u32) })
}

================================================================================
2. SUPERUSER AUDIT LOG WIRING (Closing Gap #2)
Objective: Record every raw memory operation executed under SuperUserCap into
a fixed-size SRAM ring buffer and output it directly to the UART REPL.
ACTION 2.1: Implement Ring-Buffer Logger in src/capabilities/audit.rs
// File: src/capabilities/audit.rs

#[derive(Copy, Clone)]
pub struct AuditEntry {
    pub addr: u32,
    pub val: u32,
    pub timestamp_cycles: u32,
}

pub struct AuditLog {
    buffer: [AuditEntry; 16], // Zero-allocation fixed SRAM ring buffer
    head: usize,
    count: usize,
}

impl AuditLog {
    pub const fn new() -> Self {
        Self {
            buffer: [AuditEntry { addr: 0, val: 0, timestamp_cycles: 0 }; 16],
            head: 0,
            count: 0,
        }
    }

    pub fn record_event(&mut self, addr: u32, val: u32) {
        let cycles = get_cycle_count();
        self.buffer[self.head] = AuditEntry { addr, val, timestamp_cycles: cycles };
        self.head = (self.head + 1) % 16;
        self.count = self.count.saturating_add(1);
    }

    pub fn total_audits(&self) -> usize {
        self.count
    }

    pub fn entries(&self) -> &[AuditEntry; 16] {
        &self.buffer
    }
}

pub static mut SUPERUSER_AUDIT_LOG: AuditLog = AuditLog::new();

#[inline(always)]
fn get_cycle_count() -> u32 {
    #[cfg(target_arch = "arm")]
    unsafe { core::ptr::read_volatile(0xE000_1004 as *const u32) } // DWT->CYCCNT
    
    #[cfg(target_arch = "riscv32")]
    {
        let cycles: u32;
        unsafe { core::arch::asm!("csrr {}, mcycle", out(reg) cycles) };
        cycles
    }

    #[cfg(not(any(target_arch = "arm", target_arch = "riscv32")))]
    0
}

ACTION 2.2: Add REPL Command Handler in src/drivers/repl.rs
// File: src/drivers/repl.rs
// Wire 'sys_audit' command to output the log over UART

pub fn handle_audit_command() {
    unsafe {
        let count = SUPERUSER_AUDIT_LOG.total_audits();
        uart_write_str("--- SUPERUSER AUDIT LOG ---\r\n");
        uart_write_str("Total Unsafe Operations: ");
        uart_write_u32(count as u32);
        uart_write_str("\r\nRecent Events:\r\n");

        for entry in SUPERUSER_AUDIT_LOG.entries().iter() {
            if entry.addr != 0 {
                uart_write_str("ADDR: 0x");
                uart_write_hex(entry.addr);
                uart_write_str(" | VAL: 0x");
                uart_write_hex(entry.val);
                uart_write_str(" | CYCLES: ");
                uart_write_u32(entry.timestamp_cycles);
                uart_write_str("\r\n");
            }
        }
    }
}

================================================================================
3. DYNAMIC INTERRUPT ROUTING TO JIT CLOSURES (Closing Gap #3)
Objective: Relocate Vector Table to SRAM and atomically update interrupt
pointers to jump straight to JIT-compiled function addresses in EXEC_BUFFER.
ACTION 3.1: Define Relocatable Vector Table in src/kernel/interrupt.rs
// File: src/kernel/interrupt.rs

#[repr(C, align(128))]
pub struct VectorTable {
    pub initial_sp: u32,
    pub reset_handler: unsafe extern "C" fn() -> !,
    pub nmi_handler: unsafe extern "C" fn(),
    pub hard_fault: unsafe extern "C" fn(),
    pub mem_manage: unsafe extern "C" fn(),
    pub bus_fault: unsafe extern "C" fn(),
    pub usage_fault: unsafe extern "C" fn(),
    pub reserved: [u32; 4],
    pub sv_call: unsafe extern "C" fn(),
    pub debug_mon: unsafe extern "C" fn(),
    pub reserved2: u32,
    pub pend_sv: unsafe extern "C" fn(),
    pub sys_tick: unsafe extern "C" fn(),
    pub irq_handlers: [Option<unsafe extern "C" fn()>; 32],
}

#[link_section = ".sram_vtable"]
pub static mut RAM_VECTOR_TABLE: VectorTable = VectorTable {
    initial_sp: 0,
    reset_handler: default_reset,
    nmi_handler: default_handler,
    hard_fault: default_handler,
    mem_manage: default_handler,
    bus_fault: default_handler,
    usage_fault: default_handler,
    reserved: [0; 4],
    sv_call: default_handler,
    debug_mon: default_handler,
    reserved2: 0,
    pend_sv: default_handler,
    sys_tick: default_handler,
    irq_handlers: [None; 32],
};

pub unsafe fn relocate_vector_table() {
    #[cfg(target_arch = "arm")]
    {
        let vtor = 0xE000_ED08 as *mut u32;
        let table_addr = &RAM_VECTOR_TABLE as *const _ as u32;
        core::ptr::write_volatile(vtor, table_addr);
        core::arch::asm!("dsb", "isb", options(nostack));
    }
}

pub unsafe fn attach_jit_irq(irq_index: usize, jit_fn: extern "C" fn()) {
    if irq_index < 32 {
        RAM_VECTOR_TABLE.irq_handlers[irq_index] = Some(jit_fn);
        #[cfg(target_arch = "arm")]
        core::arch::asm!("dsb", "isb", options(nostack));
    }
}

unsafe extern "C" fn default_reset() -> ! { loop {} }
unsafe extern "C" fn default_handler() { loop {} }

================================================================================
4. RISC-V EXECUTION PERMISSION & CACHE FENCING (Closing Gap #4)
Objective: Fix QEMU PT_LOAD execution permissions on RISC-V and force pipeline
instruction cache flushes after emitting JIT machine code.
ACTION 4.1: Update Linker Script (memory.x) for Executable RAM
/* File: memory.x */
MEMORY
{
  FLASH (rx)  : ORIGIN = 0x08000000, LENGTH = 128K
  RAM   (rwx) : ORIGIN = 0x20000000, LENGTH = 64K  /* Ensure 'x' execution bit */
}

SECTIONS
{
  .sram_code (NOLOAD) : ALIGN(4)
  {
    *(.sram_code .sram_code.*);
  } > RAM

  .sram_vtable (NOLOAD) : ALIGN(128)
  {
    *(.sram_vtable .sram_vtable.*);
  } > RAM
}

ACTION 4.2: Implement Pipeline Fence Barriers in src/kernel/exec.rs
// File: src/kernel/exec.rs

#[link_section = ".sram_code"]
pub static mut EXEC_BUFFER: [u8; 4096] = [0; 4096];

#[inline(always)]
pub unsafe fn flush_instruction_cache() {
    #[cfg(target_arch = "arm")]
    core::arch::asm!("dsb", "isb", options(nostack));

    #[cfg(target_arch = "riscv32")]
    core::arch::asm!("fence.i", options(nostack));
}

pub unsafe fn execute_sram_buffer(offset: usize) -> u32 {
    flush_instruction_cache();
    
    // Transmute SRAM buffer pointer directly into Ring-0 executable function
    #[cfg(target_arch = "arm")]
    let func_ptr: extern "C" fn() -> u32 = core::mem::transmute(
        (EXEC_BUFFER.as_ptr().add(offset) as usize | 1) as *const () // Set Thumb bit (Bit 0)
    );

    #[cfg(target_arch = "riscv32")]
    let func_ptr: extern "C" fn() -> u32 = core::mem::transmute(
        EXEC_BUFFER.as_ptr().add(offset)
    );

    func_ptr()
}

================================================================================
5. ARM BINARY SIZE REDUCTION TO < 64 KB (Closing Gap #5)
Objective: Reduce ARM Cortex-M binary footprint from 150 KB to under 64 KB by
stripping core::fmt, unwind tables, and bloated panic machinery.
ACTION 5.1: Modify Cargo.toml Profile Configuration
# File: Cargo.toml
[profile.release]
opt-level = "z"         # Optimize aggressively for binary size
lto = true              # Link-Time Optimization (cross-crate dead code elimination)
codegen-units = 1       # Single thread for maximum LTO optimization
panic = "abort"         # Strip unwind tables
strip = true            # Strip all symbols from final release binary
debug = false

[profile.release.build-override]
opt-level = "z"

ACTION 5.2: Replace core::fmt with Zero-Alloc Custom Printers
// File: src/drivers/uart.rs

pub fn uart_write_str(s: &str) {
    for byte in s.bytes() {
        uart_write_byte(byte);
    }
}

pub fn uart_write_u32(mut val: u32) {
    if val == 0 {
        uart_write_byte(b'0');
        return;
    }
    let mut buf = [0u8; 10];
    let mut i = 0;
    while val > 0 {
        buf[i] = (val % 10) as u8 + b'0';
        val /= 10;
        i += 1;
    }
    while i > 0 {
        i -= 1;
        uart_write_byte(buf[i]);
    }
}

pub fn uart_write_hex(val: u32) {
    const HEX_CHARS: &[u8; 16] = b"0123456789ABCDEF";
    for i in (0..8).rev() {
        let nibble = ((val >> (i * 4)) & 0xF) as usize;
        uart_write_byte(HEX_CHARS[nibble]);
    }
}

================================================================================
6. PARSER INTEGRATION & VERIFICATION CHECKLIST
ACTION 6.1: Update REPL Parsing Engine Dispatch in src/compiler/parser.rs
Replace standard un-checked poke parsing with capability-guarded execution:
// Inside single-pass REPL stream parser:
match token {
    "poke" => {
        let addr = parse_next_u32()?;
        let val = parse_next_u32()?;
        crate::kernel::memory::enforced_poke_u32(addr, val)?;
    },
    "peek" => {
        let addr = parse_next_u32()?;
        let val = crate::kernel::memory::enforced_peek_u32(addr)?;
        uart_write_hex(val);
    },
    "sys_audit" => {
        crate::drivers::repl::handle_audit_command();
    },
    _ => return Err("E003: UNKNOWN_TOKEN"),
}

VERIFICATION CHECKLIST (Execute in QEMU / Hardware)
[ ] 1. Run cargo build --release --target thumbv7em-none-eabihf
-> Verify output size with llvm-size: Text section must be < 64 KB.
[ ] 2. Boot into QEMU (qemu-system-arm & qemu-system-riscv32)
[ ] 3. Test Mandatory Enforcement:
-> Type: poke 0x40020000 1 (Without claiming GPIOA)
-> Expected Output: E001: CAPABILITY_VIOLATION - Peripheral token not claimed
[ ] 4. Test Token Claim & Execution:
-> Type: cap_claim 0 (Claims GPIOA)
-> Type: poke 0x40020000 1
-> Expected Output: Execution success (Hardware register updated).
[ ] 5. Test SuperUser Audit Logging:
-> Claim SuperUserCap: cap_superuser_override
-> Poke unmapped address: poke 0x50000000 0xDEADBEEF
-> Run audit check: sys_audit
-> Expected Output: Printout of 0x50000000 write event with timestamp.
[ ] 6. Test RISC-V Execution:
-> Confirm EXEC_BUFFER code emits, fences instruction pipeline (fence.i),
and executes in RISC-V QEMU without instruction access fault.


================================================================================
7. RESOLUTION OF ARCHITECTURAL OPEN QUESTIONS
================================================================================

--------------------------------------------------------------------------------
QUESTION 1: SiFive FE310 (RISC-V QEMU `sifive_e`) Memory Map Verification
--------------------------------------------------------------------------------
RESOLUTION: The SiFive FE310-G002 memory map differs fundamentally from ARM
Cortex-M (STM32) layout. On RISC-V QEMU (`-M sifive_e`), use these physical
peripheral memory addresses inside `src/capabilities/registry.rs`:

  • UART0:       0x1001_3000 - 0x1001_3FFF
  • UART1:       0x1002_3000 - 0x1002_3FFF
  • GPIO0:       0x1001_2000 - 0x1001_2FFF
  • QSPI0/1/2:   0x1001_4000 / 0x1002_4000 / 0x1003_4000
  • PWM0/1/2:    0x1001_5000 / 0x1002_5000 / 0x1003_5000
  • PLIC (Interrupts): 0x0C00_0000 - 0x0FFF_FFFF
  • CLINT (Timer/IPI): 0x0200_0000 - 0x0200_FFFF

Implementation Rule:
Use conditional compilation attributes (`#[cfg(target_arch = "riscv32")]`) in
`addr_to_cap_id()` to swap between ARM STM32 and RISC-V SiFive address maps at
compile time.

--------------------------------------------------------------------------------
QUESTION 2: SuperUser Capability Bypass Semantics
--------------------------------------------------------------------------------
RESOLUTION: YES. When `SuperUserCap` is active in the SRAM registry, ALL
`poke` and `peek` operations to any physical memory address must bypass
peripheral token checks completely.

Implementation Rule:
The enforcer must check `SUPERUSER_ACTIVE` first before evaluating individual
peripheral capability bits. However, ALL writes executed under `SuperUserCap`
MUST write an entry to `SUPERUSER_AUDIT_LOG` to satisfy Chapter 2 of the
Manifesto ("Unrestricted memory access with mandatory safety audit logging").

--------------------------------------------------------------------------------
QUESTION 3: Capability Enforcement Inside Compiled JIT Functions (`fn NAME()`)
--------------------------------------------------------------------------------
RESOLUTION: ADOPT OPTION (a) — No runtime capability checks inside compiled function bodies; enforce exclusively at Definition/Tokenization Time.

Rationale & Architecture:
Emitting dynamic capability checks into JIT code (Option B) breaks zero-cost
execution and bloats the SRAM instruction buffer. Rejecting MMIO inside functions
(Option C) prevents writing hardware drivers.

Enforcement Model:
1. When a function `fn NAME() { ... }` is defined in the REPL, the parser scans
   all MMIO `poke`/`peek` statements inside the function body at DEFINITION TIME.
2. If the user does NOT hold the required capabilities during `fn` definition,
   the compiler rejects compilation immediately.
3. Once compiled into `EXEC_BUFFER`, execution runs at full Ring-0 hardware
   speed with zero runtime checks.

--------------------------------------------------------------------------------
QUESTION 4: SRAM / User Memory Access via `poke` and `peek`
--------------------------------------------------------------------------------
RESOLUTION: SRAM memory access is UNRESTRICTED and requires no capability tokens.

Rationale & Architecture:
The Manifesto's capability model protects physical hardware peripherals (GPIO,
SPI, Timers, UART) from concurrent data races and state corruption. SRAM is the
user's scratchpad memory.

Implementation Rule:
In `addr_to_cap_id()`, any address falling within the board's RAM bounds:
  • ARM SRAM:   0x2000_0000..=0x2007_FFFF
  • RISC-V RAM: 0x8000_0000..=0x8007_FFFF
Returns `None` (Unmapped / Normal Memory) and bypasses capability verification.
`poke`/`peek` to SRAM executes directly as safe volatile memory reads/writes.
================================================================================
