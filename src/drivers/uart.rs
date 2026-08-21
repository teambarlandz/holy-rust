//! Bare-metal UART driver.
//!
//! Architecture-selected MMIO targets (QEMU-first, see Thought.md §3):
//!
//! - ARM (STM32F405 / QEMU netduinoplus2): USART1 @ 0x4001_1000.
//!   SR(0x00): TXE=bit7, RXNE=bit5. DR(0x04). CR1(0x0C): UE=bit13,
//!   TE=bit3, RE=bit2 — QEMU only transmits once UE|TE are set.
//! - RISC-V (SiFive E310 / QEMU sifive_e): UART0 @ 0x1001_3000.
//!   txdata(0x00): write to send. rxdata(0x04): bit31 set = empty.
//!
//! The RX ring buffer is a 256-byte static allocation (no heap). It follows
//! a single-producer/single-consumer contract: producer = IRQ handler or
//! hardware FIFO polling, consumer = the REPL loop.

/// RX ring capacity in bytes.
const RING_SIZE: usize = 256;

struct RxRing {
    buf: [u8; RING_SIZE],
    head: usize, // write index (producer)
    tail: usize, // read index (consumer)
}

static mut RX_RING: RxRing = RxRing {
    buf: [0; RING_SIZE],
    head: 0,
    tail: 0,
};

// ---------------------------------------------------------------------------
// MMIO constants
// ---------------------------------------------------------------------------

#[cfg(target_arch = "arm")]
mod mmio {
    pub const UART_BASE: usize = 0x4001_1000; // STM32F4 USART1
    pub const SR: usize = UART_BASE;
    pub const DR: usize = UART_BASE + 0x04;
    pub const CR1: usize = UART_BASE + 0x0C;
    pub const SR_TXE: u32 = 1 << 7;
    pub const SR_RXNE: u32 = 1 << 5;
    pub const CR1_UE: u32 = 1 << 13;
    pub const CR1_TE: u32 = 1 << 3;
    pub const CR1_RE: u32 = 1 << 2;
}

#[cfg(target_arch = "riscv32")]
mod mmio {
    pub const UART_BASE: usize = 0x1001_3000; // SiFive UART0
    pub const TXDATA: usize = UART_BASE;
    pub const RXDATA: usize = UART_BASE + 0x04;
    pub const RX_EMPTY: u32 = 1 << 31;
}

// ---------------------------------------------------------------------------
// Core byte I/O
// ---------------------------------------------------------------------------

/// Bring up the console UART.
pub fn init() {
    #[cfg(target_arch = "arm")]
    {
        // Enable UE | TE | RE so QEMU's usart model accepts DR writes.
        let cr1 = crate::kernel::memory::peek_u32(mmio::CR1);
        crate::kernel::memory::poke_u32(
            mmio::CR1,
            cr1 | mmio::CR1_UE | mmio::CR1_TE | mmio::CR1_RE,
        );
    }
}

/// Transmit one byte (blocking until the TX buffer frees).
pub fn put_byte(byte: u8) {
    #[cfg(target_arch = "arm")]
    {
        while crate::kernel::memory::peek_u32(mmio::SR) & mmio::SR_TXE == 0 {
            core::hint::spin_loop();
        }
        crate::kernel::memory::poke_u32(mmio::DR, byte as u32);
    }

    #[cfg(target_arch = "riscv32")]
    {
        // QEMU's sifive uart accepts txdata writes immediately; real
        // silicon would poll the full flag on read-back.
        crate::kernel::memory::poke_u32(mmio::TXDATA, byte as u32);
    }
}

/// Try to consume one received byte without blocking.
pub fn poll_get_byte() -> Option<u8> {
    #[cfg(target_arch = "arm")]
    {
        if crate::kernel::memory::peek_u32(mmio::SR) & mmio::SR_RXNE != 0 {
            Some((crate::kernel::memory::peek_u32(mmio::DR) & 0xFF) as u8)
        } else {
            None
        }
    }

    #[cfg(target_arch = "riscv32")]
    {
        let word = crate::kernel::memory::peek_u32(mmio::RXDATA);
        if word & mmio::RX_EMPTY == 0 {
            Some((word & 0xFF) as u8)
        } else {
            None
        }
    }
}

// ---------------------------------------------------------------------------
// Formatted output (zero-allocation emitters)
// ---------------------------------------------------------------------------

/// Write a raw byte string.
pub fn write_str(s: &[u8]) {
    for &b in s {
        put_byte(b);
    }
}

/// Write `0x` + 8 hex digits.
pub fn write_hex_u32(value: u32) {
    write_str(b"0x");
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    for shift in (0..32).step_by(4).rev() {
        let nibble = ((value >> shift) & 0xF) as usize;
        put_byte(HEX[nibble]);
    }
}

/// Write a decimal number (stack scratch buffer, no heap).
pub fn write_dec_u32(mut value: u32) {
    let mut scratch = [0u8; 10];
    let mut len = 0;
    loop {
        scratch[len] = b'0' + (value % 10) as u8;
        len += 1;
        value /= 10;
        if value == 0 {
            break;
        }
    }
    while len > 0 {
        len -= 1;
        put_byte(scratch[len]);
    }
}

/// Convenience: string + newline.
pub fn write_line(s: &[u8]) {
    write_str(s);
    write_str(b"\r\n");
}

// ---------------------------------------------------------------------------
// RX ring buffer + interrupt path
// ---------------------------------------------------------------------------

/// UART receive interrupt handler: pull the byte into the ring.
///
/// Single-producer (this handler) / single-consumer (REPL) contract makes
/// plain index updates sound on single-core silicon.
pub fn irq_handler() {
    if let Some(byte) = poll_get_byte() {
        // SAFETY: SPSC ring; head is only written here, tail only by the
        // REPL consumer. Capacity check prevents wrap corruption.
        unsafe {
            let ring = core::ptr::addr_of_mut!(RX_RING);
            let next = ((*ring).head + 1) % RING_SIZE;
            if next != (*ring).tail {
                (*ring).buf[(*ring).head] = byte;
                (*ring).head = next;
            }
            // Ring full: byte dropped (documented backpressure policy).
        }
    }
}

/// Pop one byte from the RX ring, if any.
pub fn ring_pop() -> Option<u8> {
    // SAFETY: see irq_handler — consumer side of the SPSC contract.
    unsafe {
        let ring = core::ptr::addr_of_mut!(RX_RING);
        if (*ring).tail == (*ring).head {
            None
        } else {
            let byte = (*ring).buf[(*ring).tail];
            (*ring).tail = ((*ring).tail + 1) % RING_SIZE;
            Some(byte)
        }
    }
}
