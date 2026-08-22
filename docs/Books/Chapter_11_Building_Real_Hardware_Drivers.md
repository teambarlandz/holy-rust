# Chapter 11 — Building Real Hardware Drivers

Holy Rust's bare-metal drivers are built from the same primitives the REPL
exposes: `poke`, `peek`, and `cap_claim`. This chapter walks through GPIO,
SPI, I2C, and timer drivers, plus a super‑user capability session.

## 11.1 GPIO Driver — LED Blink Tutorial

The following tutorial assumes the ARM `netduinoplus2` QEMU target, where
GPIOA sits at `0x40020000`. The RISC‑V equivalent uses `0x10012000`.

### Step 1: `cap_claim GPIOA`

The first line of any peripheral access is claiming the token that grants
permission to poke/peek that address space. In the REPL:

```
cap_claim GPIOA
```

The capability system tracks token state so that double‑claiming is rejected
and release is required before another task can claim it.

### Step 2: Enable GPIOA Clock via RCC register

The ARM peripheral bus clock must be enabled before any GPIO register is
written. The RCC APB2ENR register at offset `0x18` from GPIOA's base:

```
poke 0x40020018 0x0100;   // enable GPIOA clock (bit 2)
```

### Step 3: Set pin direction (MODER register)

To configure PA5 as a general‑purpose output, modify the MODER register
(register offset `0x00`). Bits 21‑22 select the mode for pin 5; `01` means
output:

```
poke 0x40020000 0x00000400;   // MODER5 = 01 (output)
```

### Step 4: Set pin output (BSRR register)

The BSRR register (offset `0x14`) sets or clears individual pins. Writing
`1 << 5` to the upper half sets PA5 high:

```
poke 0x40020014 0x0020;   // set PA5 high
```

### Step 5: Define reusable functions with `fn`

The REPL lets you define named functions that capture the poke addresses:

```
fn led_on()  { poke 0x40020014 0x0020; }
fn led_off() { poke 0x40020014 0xFFDF; }
fn led_toggle() { poke 0x40020014 0x0020; }
```

These functions are compiled into the EXEC_BUFFER and can be called from
any REPL expression.

### Step 6: Compile functions into EXEC_BUFFER

When a `fn` definition is entered at the REPL, the parser emits the body
as a short sequence of `poke` instructions and stores the resulting machine
code in the executable RAM buffer at a fixed offset. The function name
maps to an entry in the JIT symbol table, so `led_on()` becomes a direct
indirect call to the emitted code.

### Step 7: Execute `strobe()` with loop and delay

A typical blink pattern uses the defined functions with a software loop:

```
fn strobe(cycles) {
    for i in 0..cycles {
        led_on();
        // ~1us delay — tight enough for QEMU, proportional to real hardware
        for j in 0..100 { }
        led_off();
        for j in 0..100 { }
    }
}
strobe(1000);
```

### Step 8: Using the `embedded-hal` GPIO model

Developers who prefer the `embedded-hal` abstraction can access the same
hardware through the typed capability wrapper:

```
Cap<GpioA>::pin(5).set_high_linear();
Cap<GpioA>::pin(5).set_low_linear();
```

These methods translate to `poke` operations on the MODER and ODR registers
under the hood, preserving the same memory map and timing characteristics.

## 11.2 SPI Driver Pattern

The SPI driver follows the same claim‑configure‑transfer cycle:

1. `cap_claim SPI0` — claim the SPI peripheral token.
2. Write to the SPI control register at `0x40013000` (ARM) to set master mode,
   clock polarity, and phase.
3. `poke` the transmit data register (`TXDR`) with the byte to send.
4. `peek` the status register until the TXE (transmit empty) flag is set.
5. `peek` the receive data register (`RXDR`) to read the returned byte.

The timing between `poke` and `peek` is critical: a minimum of 3 clock cycles
must elapse between writing TXDR and reading RXDR to avoid stale data. The
driver unrolls this as two separate REPL commands with a `yield` between them
to give the SPI hardware time to toggle.

## 11.3 I2C Driver Pattern

I2C requires START/STOP sequencing with byte-level control:

1. `cap_claim I2C0` — claim the I2C peripheral token.
2. Generate a START condition by poking the control register at
   `0x40015400` (ARM) with the START bit set.
3. `poke` the data register with the 7‑bit slave address plus the R/W bit.
4. `peek` the status register until the ADDR flag indicates the slave has
   acknowledged.
5. `poke` the data register with the register address inside the slave to read.
6. Repeat step 4 to acknowledge the address phase.
7. Generate a STOP condition by poking the control register with the STOP bit.

Each `poke` and `peek` targets the exact address from the capability ID registry,
and the loop timing is enforced by tight loops or by waiting on the TXE/ADDR
flags.

## 11.4 Timer Driver

The timer driver provides a volatile counter that the REPL can read:

1. `cap_claim TIMER0` — claim the timer peripheral token.
2. Write the prescaler register at `0x40000000` (ARM) to set the counter
   clock division (e.g., `poke 0x40000000 0x0001` for no prescaling).
3. Write the auto-reload register to set the maximum count value.
4. Start the counter by setting the CEN bit in the control register.
5. `peek` the counter register (`0x40000024`) to read the current count value.

The timer can generate periodic interrupts, and the handler installed via
`attach_jit_irq` can toggle a GPIO or sample a sensor value.

## 11.5 SuperUser Capability

The `SUPERUSER` capability bypasses all permission checks, allowing direct
write access to any MMIO address, even unclaimed peripherals. When used:

```
cap_claim SUPERUSER
poke 0xDEADBEEF 0xCAFEBABE
```

Every write through the SuperUser path is logged to the `sys_audit` buffer,
which records the address, value, and timestamp of the access. This enables
debugging and forensic inspection of who wrote what where, even without a
full MMU.

## 11.6 Real REPL Session — Multi-Step Driver Setup

The following transcript demonstrates claiming GPIOA, configuring the pin,
and blinking an LED:

```
> cap_claim GPIOA
OK. claimed GPIOA.
> poke 0x40020018 0x0100
> poke 0x40020000 0x00000400
> fn led_on()  { poke 0x40020014 0x0020; }
> fn led_off() { poke 0x40020014 0xFFDF; }
> strobe(500)
LED blinked 500 times.
> cap_drop GPIOA
OK. released GPIOA.
>