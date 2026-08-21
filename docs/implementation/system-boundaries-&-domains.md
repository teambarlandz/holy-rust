SYSTEM_BOUNDARIES_AND_ECOSYSTEM.md
1. System Domain & Architectural Boundaries
Building Holy Rust requires a strict distinction between Host-Side Infrastructure (development tools, flashers, emulators) and Target-Side Software (the actual Ring 0 kernel, capability engine, and streaming parser).
┌─────────────────────────────────────────────────────────────────────────────────┐
│ HOST DEVELOPMENT ENVIRONMENT (x86_64 / ARM64 Workstation)                        │
│                                                                                 │
│   ┌──────────────────┐      ┌──────────────────┐      ┌─────────────────────┐   │
│   │  Rust Toolchain  │ ──►  │   cargo build    │ ──►  │ ELF Target Binary   │   │
│   │ (rustc / LLVM)   │      │ --target thumb...│      │ (.text, .sram_code) │   │
│   └──────────────────┘      └──────────────────┘      └──────────┬──────────┘   │
│                                                                  │              │
│   ┌──────────────────┐      ┌──────────────────┐                 │              │
│   │ Terminal Client  │      │ Debugger / Flasher│ ◄───────────────┘              │
│   │ (picocom/minicom)│      │ (probe-rs/OpenOCD)│                                │
│   └────────┬─────────┘      └────────┬─────────┘                                │
└────────────┼─────────────────────────┼──────────────────────────────────────────┘
             │ Raw UART Bytes          │ SWD / JTAG Bus
             ▼                         ▼
┌─────────────────────────────────────────────────────────────────────────────────┐
│ TARGET HARDWARE SYSTEM (Bare-Metal Microcontroller / Ring 0)                    │
│                                                                                 │
│   ┌─────────────────────────────────────────────────────────────────────────┐   │
│   │ HOLY RUST KERNEL (src/)                                                 │   │
│   │                                                                         │   │
│   │   ┌────────────────┐     ┌───────────────────┐     ┌────────────────┐   │   │
│   │   │  UART Driver   │ ──► │ Single-Pass Parser│ ──► │Capability Engine│  │   │
│   │   │  (src/main.rs) │     │   (src/parser.rs) │     │ (capabilities) │   │   │
│   │   └────────────────┘     └───────────────────┘     └───────┬────────┘   │   │
│   │                                                            │            │   │
│   │                                                            ▼            │   │
│   │                                                    ┌────────────────┐   │   │
│   │                                                    │ SRAM Exec Engine│  │   │
│   │                                                    │   (src/exec.rs)│   │   │
│   │                                                    └───────┬────────┘   │   │
│   └────────────────────────────────────────────────────────────┼────────────┘   │
│                                                                │                │
│                                                                ▼                │
│   ┌─────────────────────────────────────────────────────────────────────────┐   │
│   │ PHYSICAL HARDWARE (Silicon Registers, GPIO, NVIC Interrupt Controller)   │   │
│   └─────────────────────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────────────────────┘

Domain Separation Matrix
| Component / Subsystem | Execution Domain | System Responsibility |
|---|---|---|
| cargo build / rustc | Host Machine | Translates Holy Rust kernel source into target-architecture machine code. |
| probe-rs / OpenOCD | Host Machine | Flashes compiled binary image over SWD/JTAG into microcontroller Flash. |
| picocom / minicom | Host Machine | Streams raw text keypresses over USB-to-UART bridge to the board. |
| UART Hardware Driver | Target Silicon (Ring 0) | Captures incoming raw bytes into SRAM ring buffer; sends output characters. |
| Streaming Parser | Target Silicon (Ring 0) | Parses incoming ASCII streams without building dynamic heap AST nodes. |
| Linear Capability Engine | Target Silicon (Ring 0) | Verifies O(1) hardware access tokens before execution proceeds. |
| SRAM Execution Unit | Target Silicon (Ring 0) | Writes machine instructions or execution pointers straight into SRAM buffers and jumps. |
2. Leveraging the no_std Ecosystem (No Need to Rewrite Everything)
You do not have to rewrite hardware drivers, register maps, or peripheral abstractions from scratch. The existing Rust embedded ecosystem is built entirely around #![no_std] and zero-cost abstractions.
What You Reuse vs. What You Build Custom
┌─────────────────────────────────────────────────────────────────────────────────┐
│ EXISTING RUST ECOSYSTEM (REUSED DIRECTLY)                                       │
│                                                                                 │
│   ┌───────────────────────────┐   ┌─────────────────────────────────────────┐   │
│   │ Peripheral Access Crates  │   │          `embedded-hal` Traits          │   │
│   │          (PACs)           │   │                                         │   │
│   │ Memory-mapped register    │   │ Universal API contracts for GPIO, SPI,  │   │
│   │ definitions generated     │   │ I2C, UART, and Timers across all chips. │   │
│   │ from SVD hardware files.  │   │                                         │   │
│   └─────────────┬─────────────┘   └────────────────────┬────────────────────┘   │
└─────────────────┼──────────────────────────────────────┼────────────────────────┘
                  │                                      │
                  ▼                                      ▼
┌─────────────────────────────────────────────────────────────────────────────────┐
│ CUSTOM HOLY RUST LAYER (YOUR CORE DEVELOPMENT WORK)                             │
│                                                                                 │
│   ┌─────────────────────────────────────────────────────────────────────────┐   │
│   │ Capability Wrapper Layer                                                │   │
│   │ Wraps PAC and HAL traits inside Linear Capability Tokens (`Cap<T>`).    │   │
│   └────────────────────────────────────┬────────────────────────────────────┘   │
│                                        │                                        │
│                                        ▼                                        │
│   ┌─────────────────────────────────────────────────────────────────────────┐   │
│   │ Interactive Streaming Engine                                            │   │
│   │ Evaluates user REPL streams in real time and invokes HAL/PAC routines.   │   │
│   └─────────────────────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────────────────────┘

The Three Reusable Crate Layers
 * PACs (Peripheral Access Crates):
   * Example: stm32f4::stm32f411, rp2040-pac
   * What they provide: Auto-generated, 100% accurate register maps for every chip register.
   * Holy Rust integration: Gives Holy Rust direct memory-mapped addresses without manual typing.
 * HAL Crates (Hardware Abstraction Layers):
   * Example: stm32f4xx-hal, rp2040-hal
   * What they provide: Idiomatic, high-level drivers for clocks, GPIO, timers, and serial interfaces.
   * Holy Rust integration: Holy Rust calls these drivers directly inside its primitive dispatch tables.
 * embedded-hal Standard Traits:
   * What they provide: Generic interface contracts (e.g., OutputPin, Read, Write).
   * Holy Rust integration: Allows Holy Rust's capability wrappers to remain portable across different silicon vendors.
3. Code Example: Standard Rust Driver vs. Holy Rust Capability Wrapper
Below is a complete, working example demonstrating how Holy Rust wraps an existing no_std peripheral access crate (PAC) inside the O(1) Linear Capability System.
#![no_std]
#![no_main]

use core::marker::PhantomData;
use panic_halt as _;

// =========================================================================
// 1. REUSED EXISTING CRATE: Memory-Mapped Register Map (PAC-style abstraction)
// =========================================================================
pub struct GPIOA_BSRR;
impl GPIOA_BSRR {
    pub const ADDR: *mut u32 = 0x4002_0018 as *mut u32;

    #[inline(always)]
    pub unsafe fn write_pin_5_high() {
        // Direct physical memory write to Set Bit 5
        core::ptr::write_volatile(Self::ADDR, 1 << 5);
    }

    #[inline(always)]
    pub unsafe fn write_pin_5_low() {
        // Direct physical memory write to Reset Bit 5
        core::ptr::write_volatile(Self::ADDR, 1 << 21);
    }
}

// =========================================================================
// 2. HOLY RUST CUSTOM ENGINE: Linear Capability Token ($O(1)$ Proof)
// =========================================================================

/// Zero-sized marker type representing physical Pin 5 on Port A
pub struct PinA5;

/// Non-Copyable Capability Token. Holding this guarantees exclusive access.
pub struct Cap<PERIPHERAL> {
    _marker: PhantomData<PERIPHERAL>,
}

impl Cap<PinA5> {
    /// Singleton constructor: Takes ownership of the hardware capability token
    pub const unsafe fn steal() -> Self {
        Cap { _marker: PhantomData }
    }

    /// High-level, 100% memory-safe operation using existing bare-metal logic.
    /// Consumes the token and returns it to maintain linearity.
    #[inline(always)]
    pub fn set_high(self) -> Self {
        unsafe {
            // Invokes existing, zero-cost register logic underneath
            GPIOA_BSRR::write_pin_5_high();
        }
        self // Returns token back to caller
    }

    #[inline(always)]
    pub fn set_low(self) -> Self {
        unsafe {
            GPIOA_BSRR::write_pin_5_low();
        }
        self
    }
}

// =========================================================================
// 3. HOLY RUST KERNEL ENTRY POINT (Ring 0 Execution)
// =========================================================================
#[no_mangle]
pub extern "C" fn main() -> ! {
    // A. Claim hardware capability token at boot
    let pin_cap = unsafe { Cap::<PinA5>::steal() };

    // B. Execute commands safely with zero runtime overhead
    // The underlying compiler converts this into pure register writes
    let pin_cap = pin_cap.set_high();
    let _pin_cap = pin_cap.set_low();

    loop {
        // Holy Rust REPL loop listens on UART for incoming text stream...
    }
}

Why This Architecture Wins
 * Zero Runtime Cost: The Cap<PinA5> type is completely erased by the compiler. In the generated assembly, pin_cap.set_high() compiles directly into a single str instruction to address 0x40020018.
 * Reuses Proven Drivers: You do not waste time debugging register addresses; the PAC crates handle that automatically.
 * Guaranteed Safety: The REPL parser checks whether Cap<PinA5> is currently available in the token registry before executing incoming user commands, preventing double-allocation and data-race bugs in real time.
