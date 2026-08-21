# CHAPTER 02: CAPABILITY ENGINE

## Mathematical Foundation

### Linear Types vs. Standard Rust Lifetimes

Standard Rust relies on control-flow graph (CFG) construction, borrow checking, and
lifetime analysis to ensure memory safety. In a desktop or server environment, the
compiler allocates significant memory trees to map variable lifetimes across nested
scopes and async boundaries. The compiler solves lifetime constraints through O(N^2)
algorithm traversals over the control-flow graph, which introduces noticeable pauses
during compilation and is fundamentally incompatible with interactive, resource-constrained
environments.

For an interactive, Ring-0 JIT environment running on constrained embedded hardware
(e.g., 64 KB to 256 KB SRAM), this model creates unacceptable performance bottlenecks:

- **Memory Overhead**: Storing Intermediate Representations (HIR/MIR) and lifetime
  bounds requires large heap allocations that exceed the available SRAM.
- **Latency**: Solving lifetime constraints involves O(N^2) algorithm traversals,
  introducing multi-millisecond pauses during live REPL execution.
- **Complex Control Graphs**: Evaluating dynamic jumps and real-time execution
  branches in a streaming REPL quickly explodes control-flow complexity.

### Holy Rust Linear Capability Model

To preserve live JIT execution speed without sacrificing memory safety, Holy Rust
bypasses full lifetime analysis in favor of a Linear Hardware Capability Model.

The core philosophical shift is **ownership by type** rather than ownership by graph
analysis. In Holy Rust, safety is guaranteed through affine and linear type semantics
enforced directly during single-pass tokenization. Every hardware resource—whether a
physical memory range, a peripheral register, or an interrupt vector—is bound to a
single, uncopyable Capability Token (`Cap<T>`).

#### Token Semantics

```text
┌─────────────────────────────────────────────────────────┐
│                    Hardware Token                       │
│   struct Capability<T: HardwareResource> { ... }         │
└───────────────────────────┬─────────────────────────────┘
                            │
                Has Move Semantics / NO Copy / NO Clone
                            │
             ┌──────────────────┴──────────────────┐
             ▼                                     ▼
    [ Valid Ownership ]                   [ Consumed / Invalid ]
    Can write to address                  Compile error on reuse
```

#### The Rules of Linearity (O(1) Verification)

1. **Uniqueness**: A capability token for a specific resource (e.g.,
   `Capability<Pin13>`) can only exist once across the entire system. The type
   system enforces this uniqueness at compile time through affine/linear trait
   bounds.

2. **Move Semantics**: Assigning or passing a token moves ownership. The token
   cannot be duplicated. The `Copy` and `Clone` traits are explicitly
   unimplemented for all `Cap<T>` types. The compiler rejects any attempt
   to copy a capability token.

3. **Consumption**: Mutating a hardware resource consumes the token or requires
   an explicit borrow-lease token that expires upon function exit. After mutation,
   the original token is invalidated, preventing double-writes.

4. **Constant-Time Validation**: Verification requires checking token availability
   via a single-bit availability lookup (O(1)) rather than analyzing variable
   lifetime graphs. The capability bitmap is a fixed-size bitfield indexed by
   resource ID, and checking availability is a single AND operation.

### Token Registry Architecture

The runtime capability bitmap is a fixed-size data structure located in SRAM that
tracks the availability status of every hardware resource in the system.

#### Memory Layout

```text
+----------------------+-------------------------------+------------------------------+
| Capability Base Addr | Bitmask Word 0                | Bitmask Word 1               |
+----------------------+-------------------------------+------------------------------+
| 0x2000_1000          | Resource [0:31] availability  | Resource [32:63] availability |
+----------------------+-------------------------------+------------------------------+
| Size: 8 bytes        | Size: 4 bytes                 | Size: 4 bytes                |
+----------------------+-------------------------------+------------------------------+
```

#### Bitmask Validation Routine

```rust
/// Check if a capability resource is available (single-bit lookup, O(1))
pub fn capability_available(resource_id: usize) -> bool {
    let base = 0x2000_1000 as *const u32;
    let word_index = resource_id / 32;
    let bit_offset = resource_id % 32;
    let word = unsafe { *base.add(word_index) };
    let bit_mask = 1u32 << bit_offset;
    // Available if bit is 0 (no owner)
    (word & bit_mask) == 0
}

/// Acquire a capability token ( atomically set bit, O(1) )
pub fn capability_acquire(resource_id: usize) -> Option<CapResource> {
    let base = 0x2000_1000 as *mut u32;
    let word_index = resource_id / 32;
    let bit_offset = resource_id % 32;
    let word = unsafe { *base.add(word_index) };
    let bit_mask = 1u32 << bit_offset;

    // Check availability first
    if (word & bit_mask) != 0 {
        return None; // Resource already claimed
    }

    // Atomically set the bit to claim ownership
    unsafe {
        *base.add(word_index) = word | bit_mask;
    }

    // Return capability token
    Some(CapResource { id: resource_id })
}
```

#### Capability Token Structure

```rust
/// Linear capability token - non-copyable, affine type
#[derive(Debug)]
pub struct Cap<T: HardwareResource> {
    _token: u16, // O(1) token identifier
    _phantom: core::marker::PhantomData<T>,
}

// SAFETY: Cap<T> is !Copy and !Clone. Ownership is transferred on move.
impl<T: HardwareResource> !Copy for Cap<T> {}
impl<T: HardwareResource> !Clone for Cap<T> {}
```

### Memory & Peripheral Access Contracts

Holy Rust guarantees zero data races, zero buffer overflows, and zero use-after-free
bugs at the compiler level without relying on a Garbage Collector (GC) or runtime
checks. This is achieved through capability tokens that wrap raw PAC (Peripheral
Access Crate) registers.

#### Eliminating Data Races & Double Writes

Two routines (including interrupts) cannot write to the same register simultaneously
because only one execution context can hold the Capability token for that register
address at any point in time.

```rust
// Contract: Writing to Pin 13 requires consuming or borrowing its Capability token.
fn set_high(token: &mut Cap<Pin13>) {
    unsafe {
        // Direct physical write to Memory-Mapped Register
        *(0x4002_1018 as *mut u32) = 0x01;
    }
    // Token is consumed/invalidated after mutation
}

// 1. Acquire token (Single instance granted by system initialization)
let mut pin13 = system.take_pin13();

// 2. Safe execution - token is consumed after this call
set_high(&mut pin13);

// 3. Attempting a second parallel move or alias fails token validation instantly
//    compile error: value moved here after being moved
```

#### Preventing Buffer Overflows

Memory capabilities encapsulate region bounds directly within the type signature:

```rust
/// Memory region capability with compile-time bounds
pub struct MemoryRegionCap<const BASE: usize, const SIZE: usize>;

impl<const BASE: usize, const SIZE: usize> MemoryRegionCap<BASE, SIZE> {
    /// Write value at offset within compile-checked bounds
    #[inline(always)]
    pub fn write_offset(&mut self, offset: usize, value: u32) -> Result<(), SafetyError> {
        // Bounds check is CONSTANT-TIME - compiled to a single conditional
        // The compiler can prove offset < SIZE at compile time for many patterns
        if offset >= SIZE {
            return Err(SafetyError::OutOfBounds);
        }
        unsafe {
            *((BASE + offset) as *mut u32) = value;
        }
        Ok(())
    }
}
```

#### Preventing Use-After-Free

When a peripheral or memory block is released or reconfigured, its capability
token is explicitly consumed:

```rust
/// Deinitialize UART and consume its capability
pub fn deinit_uart(cap: Capability<UART0>) -> UnallocatedState {
    // 'cap' is moved here and dropped immediately
    // Subsequent access to UART0 hardware is a compile error
    UnallocatedState::new()
}
```

### Unsafe Escalation in Ring 0

In a single-address-space Ring-0 environment, an unchecked memory write can corrupt
system state or compromise peripheral hardware. To balance raw hardware access with
safety guarantees, Holy Rust defines strict rules for the `unsafe` keyword.

```text
┌──────────────────────────────┐
│   Holy Rust REPL / Code        │
└──────────────┬───────────────┘
               │
   Is 'unsafe' block requested?
               │
       ┌───────┴───────┐
       │               │
    [ YES ]          [ NO ]
       │               │
   Requires Explicit Token             Enforces Capability Rules
       │               │
       └───────┬───────┘
               │
               ▼
          [ Raw Hardware Register ]
```

#### Rules Governing `unsafe` Usage

1. **Encapsulated Unsafe**: `unsafe` blocks are restricted to low-level capability
   implementations and driver primitives. Application-level REPL code should never
   contain raw `unsafe` blocks. All hardware writes must go through capability-protected
   functions.

2. **Kernel Override Capability (SuperUserCap)**: Executing raw, arbitrary pointer
   arithmetic in the REPL requires explicitly holding a `SuperUserCap` token granted
   at system boot. This token is separate from all peripheral capability tokens and
   provides unrestricted memory access. Possession of `SuperUserCap` is audited and
   logged to the JIT execution tracer for instant debugging.

3. **Auditable Scope**: Because the single-pass compiler flags unsafe operations
   during tokenization, all raw pointer operations are logged directly to the JIT
   execution tracer. This provides instant debugging capability: every `unsafe` block
   execution is recorded with its capability token context and program counter.

4. **No Unsafe in Hot Paths**: The JIT compiler inlines all capability-protected
   hardware access into the threaded micro-primitive dispatch loop. `unsafe` blocks
   must not appear inside the inner interpreter thread loop (`run_threaded_stream`),
   as this would bypass the O(1) verification guarantee.

5. **SuperUserCap Rarity**: The `SuperUserCap` token is granted exclusively at
   system boot and should be held for the minimum duration necessary. All `unsafe`
   operations while holding `SuperUserCap` are recorded to the safety audit log.

---