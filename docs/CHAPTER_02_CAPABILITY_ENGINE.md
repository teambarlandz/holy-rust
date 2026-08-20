# CHAPTER 02: CAPABILITY ENGINE

## 2.1 Beyond Lifetime Analysis
Standard Rust relies on control-flow graph (CFG) construction, borrow checking, and lifetime analysis to ensure memory safety. In a desktop or server environment, running rustc allocates significant memory trees to map variable lifetimes across nested scopes and async boundaries.

For an interactive, Ring-0 JIT environment running on resource-constrained embedded hardware (e.g., 64 KB to 256 KB SRAM), this model creates unacceptable performance bottlenecks:
- **Memory Overhead**: Storing Intermediate Representations (HIR/MIR) and lifetime bounds requires large heap allocations.
- **Latency**: Solving lifetime constraints involves O(N^2) algorithm traversals, introducing noticeable pauses during live REPL execution.
- **Complex Control Graphs**: Evaluating dynamic jumps and real-time execution branches in a streaming REPL quickly explodes control-flow complexity.

To preserve live JIT execution speed without sacrificing memory safety, Holy Rust bypasses full lifetime analysis in favor of a Linear Hardware Capability Model.

## 2.2 Linear Capabilities & Hardware Tokens
In Holy Rust, safety is guaranteed through affine and linear type semantics enforced directly during single-pass tokenization. Every hardware resource—whether a physical memory range, a peripheral register, or an interrupt vector—is bound to a single, uncopyable Capability Token.

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

### The Rules of Linearity (O(1) Verification)
- **Uniqueness**: A capability token for a specific resource (e.g., Capability<Pin13>) can only exist once across the entire system.
- **Move Semantics**: Assigning or passing a token moves ownership. The token cannot be duplicated (Copy and Clone traits are explicitly unimplemented).
- **Consumption**: Mutating a hardware resource consumes the token or requires an explicit borrow-lease token that expires upon function exit.
- **Constant-Time Validation**: Verification requires checking token availability via a single-bit availability lookup (O(1)) rather than analyzing variable lifetime graphs.

## 2.3 Memory & Peripheral Access Contracts
Holy Rust guarantees zero data races, zero buffer overflows, and zero use-after-free bugs at the compiler level without relying on a Garbage Collector (GC) or runtime checks.

### Eliminating Data Races & Double Writes
Two routines (including interrupts) cannot write to the same register simultaneously because only one execution context can hold the Capability token for that register address at any point in time.

```rust
// Contract: Writing to Pin 13 requires consuming or borrowing its Capability token.
fn set_high(token: &mut Capability<Pin13>) {
    unsafe {
        // Direct physical write to Memory-Mapped Register
        *(0x4002_1018 as *mut u32) = 0x01;
    }
}

// 1. Acquire token (Single instance granted by system initialization)
let mut pin13 = system.take_pin13();

// 2. Safe execution
set_high(&mut pin13);

// 3. Attempting a second parallel move or alias fails token validation instantly
```

### Preventing Buffer Overflows
Memory capabilities encapsulate region bounds directly within the type signature:

```rust
pub struct MemoryRegionCap<const BASE: usize, const SIZE: usize>;

impl<const BASE: usize, const SIZE: usize> MemoryRegionCap<BASE, SIZE> {
    #[inline(always)]
    pub fn write_offset(&mut self, offset: usize, value: u32) -> Result<(), SafetyError> {
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

### Preventing Use-After-Free
When a peripheral or memory block is released or reconfigured, its capability token is explicitly consumed:

```rust
pub fn deinit_uart(cap: Capability<UART0>) -> UnallocatedState {
    // 'cap' is dropped here and cannot be accessed by subsequent statements
    UnallocatedState::new()
}
```

## 2.4 Unsafe Escalation in Ring 0
In a single-address-space Ring-0 environment, an unchecked memory write can corrupt system state or compromise peripheral hardware. To balance raw hardware access with safety guarantees, Holy Rust defines strict rules for the `unsafe` keyword:

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
   Requires Explicit Token     Enforces Capability Rules
       │               │
       └───────┬───────┘
               │
               ▼
          [ Raw Hardware Register ]
```

- **Encapsulated Unsafe**: `unsafe` blocks are restricted to low-level capability implementations and driver primitives.
- **Kernel Override Capability (SuperUserCap)**: Executing raw, arbitrary pointer arithmetic in the REPL requires explicitly holding a SuperUserCap token granted at system boot.
- **Auditable Scope**: Because the single-pass compiler flags unsafe operations during tokenization, all raw pointer operations are logged directly to the JIT execution tracer for instant debugging.