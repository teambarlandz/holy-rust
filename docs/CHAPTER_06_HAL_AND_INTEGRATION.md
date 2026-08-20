# CHAPTER 06: HARDWARE ABSTRACTION LAYER (HAL) & INTEGRATION

## 6.1 `no_std` Compatibility Strategy
Holy Rust operates in a pure bare-metal environment devoid of operating system services, process isolation, or traditional runtime libraries. Integration with the existing Rust ecosystem is achieved by enforcing strict `no_std` compliance across all embedded abstractions.

### Execution Stack Flow
1. **Holy Rust Application**: High-level user application logic or REPL execution context.
2. **`embedded-hal` Generic Traits**: Standard hardware interfaces for cross-platform portability.
3. **Peripheral Access Crates (PACs) / Memory Maps**: Direct register structures and hardware memory definitions.
4. **Bare-Metal Hardware Registers**: Physical silicon memory addresses.

---

### Zero-Cost Inlining Guarantees
To ensure that high-level peripheral calls incur zero execution penalty compared to hand-written assembly or direct raw pointer writes, the Holy Rust HAL relies on explicit compile-time inlining contracts:

1. **Monomorphization over Dynamic Dispatch**: Generic parameters and traits are resolved at JIT-compile time. The compiler never emits `vtable` lookups or indirect calls for peripheral operations.
2. **Aggressive Inlining (`#[inline(always)]`)**: Abstractions wrapping physical register writes collapse directly into single machine instructions (`STR`, `SW`, or `BITBAND`).
3. **Zero-Sized Types (ZSTs)**: Hardware pins and peripherals are represented as zero-sized structures. They consume zero bytes of RAM at runtime while carrying complete type-level safety metadata.

```rust
// Example of a Zero-Sized Type HAL representation
pub struct Pin<const PORT: char, const PIN: u8>;

impl<const PORT: char, const PIN: u8> Pin<PORT, PIN> {
    #[inline(always)]
    pub fn set_high(&mut self, _cap: &mut Cap<GPIO>) {
        let reg_addr = get_gpio_bsrr_address(PORT);
        unsafe {
            core::ptr::write_volatile(reg_addr as *mut u32, 1 << PIN);
        }
    }
}
```

## 6.2 Peripheral Mapping & Type-Safe Driver Interfaces
Holy Rust enforces hardware safety by coupling peripheral access directly to the Hardware Capability Engine (as detailed in Chapter 2). Physical hardware registers cannot be accessed directly without presenting a corresponding non-copyable Linear Capability Token.

### Peripheral Interface Specifications

#### 1. General Purpose Input/Output (GPIO)
GPIO interfaces use type-state programming to enforce pin modes (Input, Output, Alternate Function) at compile time, preventing invalid register configurations.

```rust
pub struct Input;
pub struct Output;

pub struct GpioPin<const PORT: char, const PIN: u8, Mode> {
    _mode: core::marker::PhantomData<Mode>,
}

impl<const PORT: char, const PIN: u8> GpioPin<PORT, Input,> {
    pub fn into_output(self, cap: &mut Cap<GPIO>) -> GpioPin<PORT, Output,> {
        // Mutate hardware configuration registers safely
        configure_pin_mode(PORT, PIN, PinMode::Output);
        GpioPin { _mode: core::marker::PhantomData }
    }
}
```

#### 2. Universal Asynchronous Receiver-Transmitter (UART)
UART drivers implement non-blocking and interrupt-driven streaming contracts.

```rust
pub trait UartDriver {
    fn write_byte(&mut self, cap: &mut Cap<UART>, byte: u8);
    fn read_byte(&mut self, cap: &mut Cap<UART>) -> Option<u8>;
    fn flush(&mut self, cap: &Cap<UART>);
}
```

#### 3. Serial Peripheral Interface (SPI) & Inter-Integrated Circuit (I²C)
Bus transactions enforce explicit ownership over data buffers to prevent data races during DMA (Direct Memory Access) transfers.

```rust
pub trait SpiBusTransfer {
    fn transfer_in_place<'a>(
        &mut self,
        cap: &mut Cap<SPI>,
        buffer: &'a mut [u8]
    ) -> Result<&'a [u8], SpiError>;
}
```

#### 4. Pulse Width Modulation (PWM) & Timers
Timer abstractions separate frequency configuration from duty-cycle manipulation to protect hardware state integrity.

## 6.3 Porting Guide: Target Architecture Integration
Bringing the Holy Rust environment to new silicon requires implementing a defined set of core primitives. This section outlines the required steps to port Holy Rust to a new CPU target.

### Porting Checklist Overview
- **Step 1**: Memory Map & Register Definitions
- **Step 2**: Relocatable Vector Table Configuration
- **Step 3**: Architecture-Specific Thunk Emitter Implementation
- **Step 4**: Capability & HAL Trait Binding

### Porting Implementation Details

#### Step 1: Memory Map & Register Definitions
Define the physical memory boundaries, SRAM start/end addresses, and register base addresses for the target chip in a core configuration crate.

```rust
pub struct MemoryLayout {
    pub flash_start: usize,
    pub flash_size: usize,
    pub sram_start: usize,
    pub sram_size: usize,
    pub vector_table_offset: usize,
}
```

#### Step 2: Relocatable Vector Table Configuration
Implement the mechanism to point the hardware CPU trap vector to an SRAM-allocated array.

- **ARM Cortex-M**: Write target address to VTOR (0xE000_ED08).
- **RISC-V**: Set mtvec CSR to the SRAM base address in Vectored Mode.

#### Step 3: Architecture-Specific Thunk Emitter
Implement the C-ABI interrupt trampoline generator for the target architecture instruction set (e.g., Thumb-2 for ARM, RV32I for RISC-V). The thunk must:
- Push caller-saved registers to the stack.
- Clear peripheral interrupt flags.
- Call the target JIT execution token.
- Pop registers and execute exception return instructions (BX LR or mret).

#### Step 4: Capability & HAL Trait Binding
Instantiate the base Cap<T> singletons during system initialization and bind them to the chip's Peripheral Access Crate (PAC) registers.

```rust
// System Initialization Entry Point for target architecture
#[no_mangle]
pub unsafe fn holy_rust_target_init() -> HardwareCapabilities {
    init_sram_vector_table();
    
    HardwareCapabilities {
        gpio: Cap::new_unchecked(),
        uart: Cap::new_unchecked(),
        spi:  Cap::new_unchecked(),
        timer: Cap::new_unchecked(),
    }
}