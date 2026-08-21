//! SuperUser audit log: fixed-size SRAM ring buffer.
//!
//! Every raw memory operation executed under SuperUserCap is recorded here
//! with its address, value, and cycle count. The `sys_audit` REPL command
//! dumps the log over UART.
//!
//! Zero-allocation: 16 entries × 12 bytes = 192 bytes static SRAM.

/// One audit record.
#[derive(Copy, Clone)]
pub struct AuditEntry {
    pub addr: u32,
    pub val: u32,
    pub timestamp_cycles: u32,
}

/// Fixed-capacity ring buffer (16 entries).
pub struct AuditLog {
    buffer: [AuditEntry; 16],
    head: usize,
    count: usize,
}

impl Default for AuditLog {
    fn default() -> Self {
        Self::new()
    }
}

impl AuditLog {
    pub const fn new() -> Self {
        Self {
            buffer: [AuditEntry {
                addr: 0,
                val: 0,
                timestamp_cycles: 0,
            }; 16],
            head: 0,
            count: 0,
        }
    }

    /// Record a memory access event.
    pub fn record_event(&mut self, addr: u32, val: u32) {
        let cycles = get_cycle_count();
        self.buffer[self.head] = AuditEntry {
            addr,
            val,
            timestamp_cycles: cycles,
        };
        self.head = (self.head + 1) % 16;
        self.count = self.count.saturating_add(1);
    }

    /// Total events recorded since boot.
    pub fn total_audits(&self) -> usize {
        self.count
    }

    /// Borrow the raw ring buffer for iteration.
    pub fn entries(&self) -> &[AuditEntry; 16] {
        &self.buffer
    }
}

/// Global audit log instance.
///
/// # Safety
/// Accessed only from the single-threaded REPL path (enforced poke/peek
/// under SuperUserCap). No interrupt handler writes to this.
pub static mut SUPERUSER_AUDIT_LOG: AuditLog = AuditLog::new();

/// Read the cycle counter for the current architecture.
#[inline(always)]
fn get_cycle_count() -> u32 {
    #[cfg(target_arch = "arm")]
    {
        // DWT->CYCCNT (Data Watchpoint and Trace Cycle Counter).
        // Address 0xE000_1004; enabled by default on Cortex-M4.
        unsafe { core::ptr::read_volatile(0xE000_1004 as *const u32) }
    }

    #[cfg(target_arch = "riscv32")]
    {
        let cycles: u32;
        unsafe {
            core::arch::asm!("csrr {}, mcycle", out(reg) cycles);
        }
        cycles
    }

    #[cfg(not(any(target_arch = "arm", target_arch = "riscv32")))]
    0
}
