# CHAPTER 05: BARE-METAL REPL & LIVE SYSTEM SHELL

## Interactive Shell Architecture

The Holy Rust Read-Eval-Print Loop (REPL) operates directly within Ring 0 as the
system's primary user interface and control plane. Unlike traditional operating systems
where a shell is an isolated userland process communicating with a kernel through
standard I/O streams and system calls, the Holy Rust REPL is the kernel control loop.

### Holy Rust Ring 0 Space

```text
+--------------------+      +----------------------------------+
| Hardware / Stream  | ---> | Stream Tokenizer & Lexer Buffer  |
| Interface (UART)   |      +----------------------------------+
+--------------------+                       |                    |
                                              v                    +--------------------+
+--------------------+      +----------------------------------+  | SRAM Execution     |
| Stream Tokenizer   | <---  | O(1) Capability & Type Verifier  |  | Vector Array       |
+--------------------+      +----------------------------------+            +--------------------+
```

### Stream Evaluation Cycle

- **Read (Stream Ingestion)**: Character input streams into a fixed-size ring buffer
  directly from hardware communication interfaces (such as UART, USB CDC, or memory-mapped
  console devices). Ingestion is handled via non-blocking polling or direct interrupt-driven
  ring buffers. The ring buffer is a static, fixed-size allocation (typically 256 bytes)
  that avoids dynamic allocation entirely.

- **Eval (Single-Pass Compilation)**: Upon receiving a line-break or command-end delimiter,
  the lexer processes the character buffer without heap allocations. Tokens pass directly
  to the Capability Verifier to confirm hardware safety constraints. Validated syntax
  emits Threaded Execution Tokens straight into an executable SRAM memory segment. This
  compilation step completes in approximately 100 microseconds.

- **Print (Direct Console Writing)**: Output formatting avoids intermediate allocations
  (`alloc::string`). Values write directly to the communication peripheral's output data
  register using formatted zero-allocation character emitters. Each character write is a
  direct memory-mapped write to the UART data register.

- **Loop (Immediate Re-entry)**: Memory pointers are reset; execution control returns to
  the primary poll loop in less than 1 microsecond. The REPL is a tight loop with no
  preemption, no scheduling, and no syscalls.

## Embedded Command Set & System Primitives

To allow live hardware inspection, direct register manipulation, and peripheral debugging,
the shell exposes a core set of zero-overhead, type-checked primitives. These primitives
bypass all OS-level call overhead, resolving directly to physical memory locations or
inlined machine instructions.

### REPL Protocol Primitives

| Primitive      | Signature                              | Description                               | Clock Cycle Cost |
|---|---|---|---|
| `peek`         | `peek<T>(addr: *const T) -> T`         | Reads value from physical address `addr`  | 1 - 3 cycles     |
| `poke`         | `poke<T>(addr: *mut T, val: T)`        | Writes value `val` to physical address `addr` | 1 - 3 cycles     |
| `reg_set_bit`  | `reg_set_bit(addr: usize, bit: u8)`    | Sets bit index `bit` at memory-mapped register address `addr` | 2 - 4 cycles     |
| `reg_clr_bit`  | `reg_clr_bit(addr: usize, bit: u8)`    | Clears bit index `bit` at memory-mapped register address `addr` | 2 - 4 cycles     |
| `cap_claim`    | `cap_claim<P>() -> Option<Cap<P>>`     | Claims exclusive, linear Capability token for peripheral `P` if unclaimed | O(1) lookup (~5 cycles) |
| `cap_drop`     | `cap_drop<P>(cap: Cap<P>)`             | Relinquishes ownership of peripheral `P`, making its capability available again | O(1) update (~3 cycles) |

### Hardware Inspection Primitives

#### `peek<T>(addr: *const T) -> T`

Reads a value from a physical memory address without consuming any capability token.
This is a zero-overhead inline read operation.

```rust
/// Read a 32-bit value from physical address
pub fn peek<T>(addr: *const T) -> T {
    unsafe { *addr }
}
```

#### `poke<T>(addr: *mut T, val: T)`

Writes a value to a physical memory address without consuming any capability token.
This is a zero-overhead inline write operation.

```rust
/// Write a 32-bit value to physical address
pub fn poke<T>(addr: *mut T, val: T) {
    unsafe { *addr = val; }
}
```

#### `reg_set_bit(addr: usize, bit: u8)`

Sets a specific bit at a memory-mapped register address. This is useful for setting
interrupt enables, pin controls, and other register bit fields.

```rust
/// Set a bit at a memory-mapped register address
pub fn reg_set_bit(addr: usize, bit: u8) {
    let reg_addr = addr;
    unsafe {
        core::ptr::write_volatile(reg_addr as *mut u32, 1u32 << bit);
    }
}
```

#### `reg_clr_bit(addr: usize, bit: u8)`

Clears a specific bit at a memory-mapped register address. Useful for clearing
interrupt flags, resetting peripheral enables, and other register bit operations.

```rust
/// Clear a bit at a memory-mapped register address
pub fn reg_clr_bit(addr: usize, bit: u8) {
    let reg_addr = addr;
    unsafe {
        core::ptr::write_volatile(reg_addr as *mut u32, !(1u32 << bit));
    }
}
```

#### `cap_claim<P>() -> Option<Cap<P>>`

Claims exclusive, linear Capability token for peripheral `P` if unclaimed. This
is the primary mechanism for gaining hardware access. The operation is O(1) - a
single-bit bitmap lookup to check availability, then set the bit atomically.

```rust
/// Claim a capability token for a peripheral
pub fn cap_claim<P>() -> Option<Cap<P>>
where
    P: HardwareResource,
{
    // O(1) bitmask check: is the bit set?
    if !capability_available(P::RESOURCE_ID) {
        return None; // Peripheral already claimed
    }
    
    // O(1) bitmask set: claim the peripheral
    capability_acquire(P::RESOURCE_ID);
    
    // Return capability token (ownership transferred)
    Some(Cap { _token: P::RESOURCE_ID, _phantom: core::marker::PhantomData })
}
```

#### `cap_drop<P>(cap: Cap<P>)`

Relinquishes ownership of peripheral `P`, making its capability available again.
This is O(1) - a single-bit bitmap clear operation.

```rust
/// Drop a capability token, making the peripheral available
pub fn cap_drop<P>(cap: Cap<P>)
where
    P: HardwareResource,
{
    // O(1) bitmask clear: release the peripheral
    let base = 0x2000_1000 as *mut u32;
    let word_index = cap._token / 32;
    let bit_offset = cap._token % 32;
    let bit_mask = 1u32 << bit_offset;
    
    unsafe {
        *base.add(word_index) = *base.add(word_index) & !bit_mask;
    }
}
```

### Real-Time REPL Execution Example

```rust
// Typing directly into the Holy Rust bare-metal shell:
// 1. Claim capability token for GPIO Port A
let mut gpio_a = cap_claim::<GPIOA>().expect("GPIOA already in use");

// 2. Direct memory-mapped register write via REPL
poke(0x4002_0000 as *mut u32, 0x0000_0001);

// 3. Inline toggle operation using capability token
gpio_a.pin(0).set_high();
```