# Chapter 5 — System Inspection and Memory Layout

## 5.1 The ARM Memory Map

Holy Rust targets the STM32F405 (QEMU netduinoplus2) on ARM Cortex-M4F.
The 128 KB flash and 64 KB SRAM are carved into non-overlapping regions
defined in `memory.x`:

```text
MEMORY
{
    flash (rx)     : ORIGIN = 0x08000000, LENGTH = 128K
    sram (rwx)     : ORIGIN = 0x20003000, LENGTH = 52K
    vectors (rw)   : ORIGIN = 0x20000400, LENGTH = 3K
    registry (rw)  : ORIGIN = 0x20001000, LENGTH = 256
    sram_code (rwx): ORIGIN = 0x20002000, LENGTH = 4K
}
```

| Region       | Address              | Size  | Permissions | Contents                           |
|--------------|----------------------|-------|-------------|-------------------------------------|
| `flash`      | `0x0800_0000`        | 128K  | rx          | Kernel, JIT engine, vector table    |
| `sram`       | `0x2000_3000`        | 52K   | rwx         | `.data`, `.bss`, stack              |
| `vectors`    | `0x2000_0400`        | 3K    | rw          | Relocatable trap/handler slots      |
| `registry`   | `0x2000_1000`        | 256   | rw          | O(1) capability bitfield            |
| `sram_code`  | `0x2000_2000`        | 4K    | rwx         | JIT execution buffer                |

The stack descends from the top of SRAM: `_stack_top = 0x2000_3000 + 52K = 0x2010_0000`.

Flash is mapped at `0x0800_0000` (aliased at `0x0000_0000` by the SoC, which
is how QEMU's stm32f4xx model boots it). The `.isr_vector` section in flash
holds the initial stack pointer and Reset entry, followed by exception handlers
routed to `fault_hang`.

## 5.2 The RISC-V Memory Map

Holy Rust targets the SiFive E310 (QEMU sifive_e) on RISC-V RV32IMAC.
The memory layout is defined in `memory-riscv.x`:

```text
MEMORY
{
    flash (rx)     : ORIGIN = 0x20400000, LENGTH = 512K
    sram (rwx)     : ORIGIN = 0x80000000, LENGTH = 5K
    vectors (rw)   : ORIGIN = 0x80001400, LENGTH = 1K
    registry (rw)  : ORIGIN = 0x80001800, LENGTH = 256
    sram_code (rwx): ORIGIN = 0x08000000, LENGTH = 4K
}
```

| Region       | Address              | Size  | Permissions | Contents                           |
|--------------|----------------------|-------|-------------|-------------------------------------|
| `flash`      | `0x2040_0000`        | 512K  | rx          | Code (QEMU boot ROM jumps here)     |
| `sram`       | `0x8000_0000`        | 5K    | rwx         | DTIM: `.data`, `.bss`, stack        |
| `vectors`    | `0x8000_1400`        | 1K    | rw          | Trap/handler slots                  |
| `registry`   | `0x8000_1800`        | 256   | rw          | O(1) capability bitfield            |
| `sram_code`  | `0x0800_0000`        | 4K    | rwx         | ITIM: JIT execution buffer          |

The stack descends from `_stack_top = 0x8000_0000 + 5K = 0x8000_1400`.

QEMU's sifive_e boot ROM unconditionally jumps to the flash controller base
`0x2040_0000` (the `0x2000_0000` window is the XIP alias), so code is linked
there. The DTIM at `0x8000_0000` is 8 KB in this machine and is carved into
non-overlapping regions. The ITIM at `0x0800_0000` is the natural home for
generated code on this SoC — tightly-coupled instruction RAM that can be
fetched at full speed.

**Note:** There is no `.isr_vector` section on RISC-V. The boot ROM jumps
directly to `ORIGIN(flash)`, so the `Reset` entry code must be the first
thing there.

## 5.3 The VectorTable Struct

The vector table is a typed, relocatable struct placed in the `.sram_vectors`
link section:

```rust
#[repr(C, align(1024))]
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
```

The struct is laid out exactly as ARM Cortex-M hardware expects: SP, Reset,
NMI, HardFault, MemManage, BusFault, UsageFault, reserved slots, SVCall,
DebugMon, reserved, PendSV, SysTick, then 32 external IRQ slots.

- `#[repr(C, align(1024))]` — 1024-byte alignment satisfies the VTOR
  (Vector Table Offset Register) alignment requirement on ARM
- `VECTOR_SLOTS = 256` — total dispatch slots (16 core exceptions + 32 IRQs,
  padded to 256)
- `irq_handlers` are `Option<unsafe extern "C" fn()>` — `None` means "no
  handler installed"

The global instance is placed at `0x2000_0400` via the `.sram_vectors` link
section:

```rust
#[used]
#[link_section = ".sram_vectors"]
pub static mut RAM_VECTOR_TABLE: VectorTable = VectorTable {
    initial_sp: 0,
    reset_handler: default_reset,
    nmi_handler: default_handler,
    hard_fault: default_handler,
    // ... all core exceptions default to busy-loop handlers ...
    irq_handlers: [None; 32],
};
```

## 5.4 attach_jit_irq(): Wiring IRQ Slots

The `attach_jit_irq()` function atomically wires an IRQ slot to a
JIT-compiled function address:

```rust
pub unsafe fn attach_jit_irq(irq_index: usize, jit_fn: extern "C" fn()) {
    if irq_index < 32 {
        unsafe {
            RAM_VECTOR_TABLE.irq_handlers[irq_index] = Some(jit_fn);
        }
        #[cfg(target_arch = "arm")]
        unsafe {
            core::arch::asm!("dsb", "isb", options(nostack));
        }
    }
}
```

Constraints:
- `irq_index` must be < 32
- `jit_fn` must be a valid interrupt handler with C-ABI linkage (`extern "C"`)
- The handler must follow the interrupt contract: no stack frame setup,
  direct register access only
- After writing, ARM requires `dsb` + `isb` to ensure the write is visible
  to the interrupt controller

This enables runtime-reconfigurable interrupt routing: a REPL session can
compile a handler into `EXEC_BUFFER` and wire it to an IRQ slot without
rebooting.

## 5.5 boot_relocate_vectors(): Vector Table Relocation

The `boot_relocate_vectors()` function runs once during boot, before any
interrupt source is enabled:

### ARM

```rust
unsafe fn boot_relocate_vectors() {
    // 1. Copy flash vector table into the typed RAM table
    let begin = core::ptr::addr_of!(__vector_start) as usize;
    let end = core::ptr::addr_of!(__vector_end) as usize;
    let words = core::cmp::min((end - begin) / 4, VECTOR_SLOTS);
    let src = begin as *const u32;
    let dst = core::ptr::addr_of_mut!(RAM_VECTOR_TABLE) as *mut u32;
    for i in 0..words {
        let v = core::ptr::read_volatile(src.add(i));
        core::ptr::write_volatile(dst.add(i), v);
    }
    // 2. Point VTOR at the RAM table
    let table_addr = core::ptr::addr_of!(RAM_VECTOR_TABLE) as u32;
    core::ptr::write_volatile(VTOR as *mut u32, table_addr);
    core::arch::asm!("dsb", "isb", options(nostack));
}
```

The Cortex-M VTOR (Vector Table Offset Register) at `0xE000_ED08` is
written with the address of `RAM_VECTOR_TABLE`. The `dsb` + `isb` sequence
ensures the write takes effect before any interrupt is enabled.

### RISC-V

```rust
unsafe fn boot_relocate_vectors() {
    extern "C" { fn _trap_hang(); }
    let base = (_trap_hang as *const ()) as usize & !0x3;
    core::arch::asm!(
        "csrw mtvec, {reg}",
        reg = in(reg) base,
    );
}
```

On RISC-V, `mtvec` (Machine Trap-Vector Base Address) is set to a hang
stub in direct mode (mode bits = 00). All traps land on the same PC,
which is a tight `j _trap_hang` loop. This makes unexpected traps
observable (instead of jumping through address 0) while preserving fault
state for a debugger. Vectored dispatch will be configured later when IRQ
bring-up lands.

## 5.6 The UART Driver

### ARM: USART1 @ 0x4001_1000

```rust
mod mmio {
    pub const UART_BASE: usize = 0x4001_1000;  // STM32F4 USART1
    pub const SR: usize = UART_BASE;           // Status Register
    pub const DR: usize = UART_BASE + 0x04;    // Data Register
    pub const CR1: usize = UART_BASE + 0x0C;   // Control Register 1
    pub const SR_TXE: u32 = 1 << 7;            // TX empty
    pub const SR_RXNE: u32 = 1 << 5;           // RX not empty
    pub const CR1_UE: u32 = 1 << 13;           // USART enable
    pub const CR1_TE: u32 = 1 << 3;            // TX enable
    pub const CR1_RE: u32 = 1 << 2;            // RX enable
}
```

Initialization enables UE | TE | RE in CR1 so QEMU's USART model accepts
DR writes. Transmission polls SR.TXE until the TX buffer is empty, then
writes to DR.

### RISC-V: UART0 @ 0x1001_3000

```rust
mod mmio {
    pub const UART_BASE: usize = 0x1001_3000;  // SiFive UART0
    pub const TXDATA: usize = UART_BASE;       // TX data register
    pub const RXDATA: usize = UART_BASE + 0x04;// RX data register
    pub const RX_EMPTY: u32 = 1 << 31;         // RX FIFO empty flag
}
```

QEMU's sifive UART accepts txdata writes immediately; real silicon would
poll the full flag on read-back.

## 5.7 The RX Ring Buffer

The UART receive path uses a 256-byte ring buffer with single-producer /
single-consumer (SPSC) contract:

```rust
const RING_SIZE: usize = 256;

struct RxRing {
    buf: [u8; RING_SIZE],
    head: usize,  // write index (producer)
    tail: usize,  // read index (consumer)
}
```

- **Producer** (`irq_handler()`): polls `poll_get_byte()`, writes to
  `buf[head]`, advances `head = (head + 1) % RING_SIZE`. On ring full,
  the byte is dropped (documented backpressure policy).
- **Consumer** (`ring_pop()`): reads from `buf[tail]`, advances
  `tail = (tail + 1) % RING_SIZE`. Returns `None` when empty.

The SPSC contract makes plain index updates sound on single-core silicon
without atomics or locks.

## 5.8 Boot Sequence in main.rs

### ARM Boot

```text
Reset (vector word 1)
  |
  +-> Reset()             // main.rs, #[no_mangle]
        |
        +-> init_data_bss()  // copy .data from flash, zero .bss
        |
        +-> boot()
              |
              +-> uart::init()                    // enable UE|TE|RE
              +-> uart::write_str(BANNER)         // "Holy Rust REPL v0.1"
              +-> boot_relocate_vectors()         // flash -> SRAM, write VTOR
              +-> repl::run()                     // never returns
```

The ARM Cortex-M loads SP from vector word 0 before entering `Reset()`, so
plain Rust code can run immediately. No naked assembly prologue is needed.

### RISC-V Boot

```text
Reset (naked asm, vector word 0)
  |
  +-> la gp, __global_pointer$
  +-> la sp, _stack_top
  +-> tail rust_boot_riscv
        |
        +-> init_data_bss()  // copy .data from flash, zero .bss
        |
        +-> boot()
              |
              +-> uart::init()                    // no-op on QEMU
              +-> uart::write_str(BANNER)         // "Holy Rust REPL v0.1"
              +-> boot_relocate_vectors()         // set mtvec to hang stub
              +-> repl::run()                     // never returns
```

The RISC-V `Reset` is `#[unsafe(naked)]` so no prologue touches SP before
we set it. The naked assembly establishes `gp` (global pointer) and `sp`
from linker symbols, then tail-calls `rust_boot_riscv` which is a normal
Rust function.

## 5.9 Panic Handler

The panic handler writes the panic message over UART and enters an infinite
`wfi` loop:

```rust
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
        unsafe {
            #[cfg(target_arch = "arm")]
            core::arch::asm!("wfi");
            #[cfg(target_arch = "riscv32")]
            core::arch::asm!("wfi");
        }
    }
}
```

The `wfi` (Wait For Interrupt) instruction parks the core efficiently.
Nothing re-enables IRQs here, so this is a permanent halt until a debugger
or reset ends the session. The handler is infallible by construction: it
must not itself panic.

## 5.10 The help Command Output

The `help` command prints the exact text below:

```text
commands:
peek ADDR;              read u32 from address (requires capability)
poke ADDR VAL;          write u32 to address (requires capability)
reg_set_bit ADDR BIT;   set register bit (requires capability)
reg_clr_bit ADDR BIT;   clear register bit (requires capability)
cap_claim NAME;         claim peripheral (GPIOA GPIOB UART0 SPI0 I2C0 TIMER0 DMA0 SUPERUSER)
cap_drop NAME;          release peripheral
let NAME = EXPR;        bind constant
fn NAME() { ... }       define callable body
EXPR;                   evaluate (+ - * / % left-to-right)
sys_audit               dump SuperUser audit log
banner                  reprint banner
```

## 5.11 The banner Command Output

The `banner` command reprints the boot banner:

```text
Holy Rust REPL v0.1
```

This is the constant `kernel::BANNER` defined in `kernel/mod.rs`:

```rust
pub const BANNER: &[u8] = b"Holy Rust REPL v0.1\r\n";
```

## 5.12 The sys_audit Command

The `sys_audit` command dumps the SuperUser audit log over UART. The audit
log is a fixed-size ring buffer of 16 entries (192 bytes total) stored in
SRAM:

```rust
pub struct AuditEntry {
    pub addr: u32,
    pub val: u32,
    pub timestamp_cycles: u32,
}
```

Output format:

```text
--- SUPERUSER AUDIT LOG ---
Total Unsafe Operations: <count>
Recent Events:
ADDR: 0x40011000 | VAL: 0x0000000D | CYCLES: 12345678
ADDR: 0x40020000 | VAL: 0x00000001 | CYCLES: 12345690
...
```

Every raw memory operation executed under `SuperUserCap` is recorded with
its address, value, and cycle count (DWT->CYCCNT on ARM, `mcycle` CSR on
RISC-V). The `handle_audit()` function in the REPL iterates the ring buffer
and prints each non-zero entry. The total count saturates (never overflows)
and represents the total number of unsafe operations performed since boot.
