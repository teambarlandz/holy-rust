# CHAPTER 05: BARE-METAL REPL & LIVE SYSTEM SHELL

## 5.1 Interactive Shell Architecture
The Holy Rust Read-Eval-Print Loop (REPL) operates directly within Ring 0 as the system's primary user interface and control plane. Unlike traditional operating systems where a shell is an isolated userland process communicating with a kernel through standard I/O streams and system calls, the Holy Rust REPL is the kernel control loop.

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
- **Read (Stream Ingestion)**: Character input streams into a fixed-size ring buffer directly from hardware communication interfaces (such as UART, USB CDC, or memory-mapped console devices). Ingestion is handled via non-blocking polling or direct interrupt-driven ring buffers.
- **Eval (Single-Pass Compilation)**: Upon receiving a line-break or command-end delimiter, the lexer processes the character buffer without heap allocations. Tokens pass directly to the Capability Verifier to confirm hardware safety constraints. Validated syntax emits Threaded Execution Tokens straight into an executable SRAM memory segment.
- **Print (Direct Console Writing)**: Output formatting avoids intermediate allocations (`alloc::string`). Values write directly to the communication peripheral's output data register using formatted zero-allocation character emitters.
- **Loop (Immediate Re-entry)**: Memory pointers are reset; execution control returns to the primary poll loop in less than 1 microsecond.

## 5.2 Embedded Command Set & System Primitives
To allow live hardware inspection, direct register manipulation, and peripheral debugging, the shell exposes a core set of zero-overhead, type-checked primitives. These primitives bypass all OS-level call overhead, resolving directly to physical memory locations or inlined machine instructions.

| Primitive      | Signature                              | Description                               | Clock Cycle Cost |
|---|---|---|---|
| `peek`         | `peek<T>(addr: *const T) -> T`         | Reads value from physical address `addr`  | 1 - 3 cycles     |
| `poke`         | `poke<T>(addr: *mut T, val: T)`        | Writes value `val` to physical address `addr` | 1 - 3 cycles     |
| `reg_set_bit`  | `reg_set_bit(addr: usize, bit: u8)`    | Sets bit index `bit` at memory-mapped register address `addr` | 2 - 4 cycles     |
| `reg_clr_bit`  | `reg_clr_bit(addr: usize, bit: u8)`    | Clears bit index `bit` at memory-mapped register address `addr` | 2 - 4 cycles     |
| `cap_claim`    | `cap_claim<P>() -> Option<Cap<P>>`     | Claims exclusive, linear Capability token for peripheral `P` if unclaimed | O(1) lookup (~5 cycles) |
| `cap_drop`     | `cap_drop<P>(cap: Cap<P>)`             | Relinquishes ownership of peripheral `P`, making its capability available again | O(1) update (~3 cycles) |

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

## 5.3 Module Loading & Dynamic Symbol Resolution
Because Holy Rust uses a single address space, dynamic module loading does not require complex ELF loaders, section relocations, or dynamic link libraries (.so / .dll). Modern runtime linking overhead is eliminated entirely.

### SRAM Execution Table
All JIT-compiled functions, interactive shell variables, and hardware handlers resolve to entry points inside a static system Symbol & Execution Table located at a known alignment boundary in SRAM.

```text
SRAM Base Address: 0x2000_0000
+-----------------------+------------------------+---------------------------------+
| Symbol Hash (32-bit)  | Target Address Pointer | Metadata / Capability Signature |
+-----------------------+------------------------+---------------------------------+
| 0xA3F190B2            | 0x2000_1040            | Executable, Cap: GPIOA          |
| 0x4B2C11D9            | 0x2000_10B8            | Executable, Cap: TIMER1         |
| 0x00000000 (Empty)    | 0x0000_0000            | Unallocated                     |
+-----------------------+------------------------+---------------------------------+
```

### Dynamic Resolution Procedure
- **Symbol Hashing**: When a new function or module is compiled by the JIT engine, its identifier is hashed using a fast, non-cryptographic 32-bit hash algorithm (such as FNV-1a).
- **In-SRAM Linking**: If the code references another compiled primitive, the JIT engine queries the SRAM Symbol Table via O(1) open-addressing hash lookup.
- **Direct Pointer Patching**: The engine writes the resolved physical SRAM function address directly into the instruction payload of the calling routine.
- **Execution**: Calling a dynamic module becomes a single, raw assembly jump instruction (jal on RISC-V or bl on ARM), completely stripping out dynamic dispatch overhead.