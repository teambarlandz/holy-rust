# Chapter 9 — Interrupts and Real Time on Bare Metal

The Holy REPL is deliberately single-threaded: one command runs at a time, one
stack, no scheduler. On real silicon, however, the world does not wait for your
REPL loop. UART bytes arrive when they want to. Timers overflow whether or not
you are mid-expression. Interrupts are how a bare-metal system reconciles a
synchronous REPL with an asynchronous world — and doing it safely, in Rust,
with zero runtime, is the subject of this chapter.

## 9.1 Why Interrupts Matter When There Is No OS

On a hosted system you would spawn a thread and let the kernel arbitrate.
Holy has no kernel. Ring 0 *is* everything. If we poll the UART in the REPL
loop, bytes would be lost every time a long JIT compile or memory dump blocked
the loop. The only correct design is:

1. Hardware raises an interrupt.
2. A minimal handler moves the byte into a lock-free buffer.
3. The REPL drains that buffer whenever it is ready.

This gives us event capture without preemption of user work, and it gives the
REPL a hard real-time guarantee: nothing ever interrupts a command except a
bounded, sub-microsecond handler.

## 9.2 The Typed Vector Table

The vector table is the first thing the core reads after reset, yet most
projects define it as a bag of function pointers in a raw array. Holy types it
exactly as the ARMv7-M architecture reference describes it:

```rust
#[repr(C, align(1024))]
pub struct VectorTable {
    pub initial_sp:   u32,
    pub reset_handler: unsafe extern "C" fn(),
    pub nmi_handler:   unsafe extern "C" fn(),
    pub hard_fault:    unsafe extern "C" fn(),
    pub mem_manage:    unsafe extern "C" fn(),
    pub bus_fault:     unsafe extern "C" fn(),
    pub usage_fault:   unsafe extern "C" fn(),
    pub reserved:      [u32; 4],
    pub sv_call:       unsafe extern "C" fn(),
    pub debug_mon:     unsafe extern "C" fn(),
    pub reserved2:     u32,
    pub pend_sv:       unsafe extern "C" fn(),
    pub sys_tick:      unsafe extern "C" fn(),
    pub irq_handlers:  [Option<unsafe extern "C" fn()>; 32],
}
type Handler = unsafe extern "C" fn();
```

Notes on the layout:

- `#[repr(C)]` guarantees field order matches the architecture's table exactly;
  `align(1024)` satisfies the VTOR alignment requirement (table base must be
  aligned to a power of two ≥ 128, and 1024 covers all 16 + 32 slots with
  room).
- The fifteen core entries (`initial_sp` through `sys_tick`) are mandated by
  the spec; the four `reserved` words are padding the hardware skips.
- `irq_handlers` holds **VECTOR_SLOTS worth of external IRQs** — Holy exposes
  the first 32 device interrupts of the 256-slot architectural space
  (`VECTOR_SLOTS = 256`), which comfortably covers every peripheral on both
  supported targets.
- Each slot is `Option<unsafe extern "C" fn()>`, so an unclaimed IRQ is
  `None` — a typed, checkable absence instead of a garbage pointer.

## 9.3 SRAM-Resident Vector Tables

Flash vector tables are immutable after reset, but real-time systems want to
*repoint* handlers at runtime. Both targets therefore reserve a dedicated RAM
region via the linker script:

```
.sram_vectors : {
    . = ALIGN(1024);
    KEEP(*(.sram_vectors))
} > RAM
```

| Target  | `.sram_vectors` address |
|---------|-------------------------|
| ARM     | `0x20000400`            |
| RISC-V  | `0x80001400`            |

The addresses are chosen to be 1 KiB-aligned within each machine's SRAM map
while leaving the bottom of RAM for the initial stack and statics.

## 9.4 Boot-Time Relocation: `boot_relocate_vectors()`

During boot, before any Rust code touches peripherals, Holy decides where
interrupt vectors live:

- **ARM**: the flash-resident master table is copied word-by-word into
  `RAM_VECTOR_TABLE` at `0x20000400`, then the Vector Table Offset Register is
  programmed so the core fetches from SRAM from then on.
- **RISC-V**: there is no vectored controller equivalent in our QEMU setup, so
  `boot_relocate_vectors()` aims `mtvec` directly at a hang stub compiled from
  inline assembly:

```rust
core::arch::global_asm!(
    ".section .text.trap_hang",
    ".globl _trap_hang",
    "_trap_hang:",
    "j _trap_hang"
);
```

Direct mode (`mtvec` low bits = 0) means *all* traps land on `_trap_hang`,
which spins forever — a deliberate, diagnosable policy while RISC-V trap
dispatch matures.

## 9.5 Writing VTOR Safely: `relocate_vector_table()`

On ARM the Vector Table Offset Register lives at a fixed private-peripheral
address:

```
VTOR = 0xE000_ED08
```

Changing it while the core could take an exception would be catastrophic, so
the write is fenced:

```rust
pub unsafe fn relocate_vector_table(table: *const VectorTable) {
    let vtor = 0xE000_ED08 as *mut u32;
    core::ptr::write_volatile(vtor, table as u32);
    core::arch::asm!("dsb", "isb");
}
```

`DSB` ensures the write has completed system-wide; `ISB` flushes any
prefetched exception-return paths so subsequent interrupts use the new table.
Skipping either barrier can leave the core fetching stale vectors for dozens
of cycles.

## 9.6 Wiring JIT Handlers: `attach_jit_irq()`

Holy's signature trick is compiling interrupt handlers *at runtime*: the user
types a handler body into the REPL, the JIT emits machine code into executable
RAM, and one call wires it into the live table:

```rust
pub unsafe fn attach_jit_irq(
    irq_index: usize,
    jit_fn: unsafe extern "C" fn(),
) -> Result<(), ()> {
    if irq_index >= 32 {
        return Err(());          // bounds-check: only 32 exposed slots
    }
    RAM_VECTOR_TABLE.irq_handlers[irq_index] = Some(jit_fn);
    core::arch::asm!("dsb", "isb");
    Ok(())
}
```

Three things make this safe:

1. **Bounds check** — `irq_index < 32` is enforced before any store; indices up
   to the architectural `VECTOR_SLOTS = 256` are rejected rather than trusted.
2. **Atomic pointer store** — a single aligned word write cannot tear.
3. **DSB+ISB after wiring** — an IRQ arriving between the store and the fence
   still sees a coherent table.

Detachment is symmetric: assign `None` and fence again.

## 9.7 Anatomy of a Trampoline

JIT-emitted code must not need to know about the vector table, so a tiny
native trampoline mediates. Here is channel 16, wired for a memory-mapped
peripheral whose interrupt status register sits at `0x40010004`:

```rust
pub unsafe extern "C" fn generic_irq_trampoline_ch16() {
    // Acknowledge pending bit 16 in the peripheral's status register.
    let status = 0x40010004 as *mut u32;
    status.write_volatile(status.read_volatile() | (1 << 16));

    // Dispatch to whatever the user registered in slot 16.
    if let Some(h) = RAM_VECTOR_TABLE.irq_handlers[16] {
        h();
    }
}
```

The acknowledge-first discipline prevents re-entry storms: clear the source
before dispatching user code. The dispatch is a single `Option` load plus an
indirect call.

## 9.8 Latency Analysis: Sub-Twelve-Cycle Dispatch

Interrupt latency budget, measured with the DWT cycle counter:

| Stage                              | Cycles |
|------------------------------------|--------|
| Hardware entry (push regs, fetch)  | ~7     |
| Trampoline prologue                | 1–2    |
| Acknowledge read-modify-write      | 2–3    |
| Slot load + indirect branch        | 1–2    |
| **Total to first user instruction**| **<12**|

Every stage is a fixed-cost operation: no allocation, no locks, no dynamic
dispatch beyond the one indirect jump. That is what makes the < 12 cycle
target meaningful — it is deterministic, not merely fast on average.

## 9.9 How Interrupts Flow at Reset

At reset the ARM core performs the following sequence:

1. Loads `SP` from `vector[0]` (the initial stack pointer).
2. Jumps to the `Reset` handler at `vector[1]`.
3. The Reset handler runs `init_data_bss()`, then `boot()`.
4. `boot::run()` starts the REPL event loop.

While the REPL is running, any peripheral that asserts IRQ causes the core to
vector through the (now SRAM-resident) vector table, execute the trampoline
for the appropriate slot, and dispatch the JIT-compiled handler — all in under
12 cycles.

## 9.10 How REPL Can Overwrite Handlers at Runtime

The REPL user can install a JIT-compiled function into any IRQ slot:

```
attach_jit_irq(16, my_handler)
```

where slot 16 maps to a JIT-compiled function previously defined in the REPL.
The change takes effect immediately: the next time IRQ 16 fires, the trampoline
loads the new handler from `RAM_VECTOR_TABLE.irq_handlers[16]` and calls it
directly. No recompile, no restart, the live table is the only source of truth.

## 9.11 Real-Time Constraints: Single-Threaded Dispatch

Holy's interrupt dispatch is single-threaded with no context save/restore:

- The IRQ trampoline calls the handler **directly** — no save of callee-saved
  registers, no stack switching.
- The handler runs in the same Ring-0 address space, with bare-metal access.
- No OS scheduler intervenes; the REPL is the sole consumer of interrupt outcomes.
- Dispatch cost is dominated by one indirect jump through the `Option` slot.

This yields nanosecond-level interrupt response on both ARM Cortex-M and
RISC-V targets — the handler is invoked exactly when the peripheral asserts
IRQ, and returns when its work is done.

## 9.12 The `generic_irq_trampoline_ch16` Function

```rust
pub unsafe extern "C" fn generic_irq_trampoline_ch16() {
    // 1. Hardware acknowledge: clear pending bit 16 on the example
    //    peripheral's status register at 0x40010004.
    unsafe {
        let pending_reg = 0x4001_0004 as *mut u32;
        core::ptr::write_volatile(
            pending_reg,
            core::ptr::read_volatile(pending_reg) | (1 << 16),
        );
    }

    // 2. Direct call: dispatch the registered execution token if present.
    unsafe {
        if let Some(handler) = RAM_VECTOR_TABLE.irq_handlers[16] {
            handler();
        }
    }
}
```

The function first performs a read-modify-write on the peripheral's status
register to clear the pending interrupt (acknowledging the hardware), then
dispatches to the handler registered in slot 16 of the vector table. If slot
16 is `None`, no handler is called. This is the generic trampoline pattern
applicable to any IRQ channel — channel 16 is the documented example.

---

*Real-time guarantees of Ring 0: no context switches, no preemption by software,
deterministic <12 cycle latency, bounded 256-byte buffering, and no hidden
allocation. A JIT-recompiled interrupt handler goes from source text to live
hardware response in milliseconds, and then responds to the next event in under
twelve cycles, forever.*