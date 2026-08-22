# Chapter 6: Field Diagnostics

## 6.1 Error Code Reference Table

The Holy Rust kernel uses a compact set of error codes. Each code is emitted over UART as a single line when a fault is detected at parse time or runtime.

| Code | Name | Cause |
|------|------|-------|
| `E001` | `CAPABILITY_VIOLATION` | Peripheral token not claimed before poke/peek |
| `E002` | `PERMISSION_DENIED` | Unmapped MMIO access without `SuperUserCap` |
| `LEX` | `LexError` | Invalid character or malformed literal |
| `UNEXPECTED TOKEN` | Wrong token | Token appeared in a position the grammar does not expect |
| `UNSUPPORTED OPERATOR` | Operator not allowed | Operator is not one of `+`, `-`, `*`, `/`, `%` |
| `UNKNOWN SYMBOL` | Undefined name | Variable or function name not found in symbol table |
| `FN REDEFINED` | Duplicate name | A function with this name was already defined |
| `SYMBOL TABLE FULL` | 32 slots exhausted | No free slot in the 32-entry symbol table |
| `FN TABLE FULL` | 2 functions defined | Both function table slots are occupied |
| `STREAM FULL` | Token stream > 128 words | Compiled token stream exceeds the 128-word buffer |
| `NAME TOO LONG` | Identifier > 16 bytes | Identifier exceeds the 16-byte name limit |
| `DIV BY ZERO` | Division by zero | Division or modulo by zero attempted |
| `MISSING SEMICOLON` | Statement not terminated | Line did not end with `;` |

## 6.2 Hard Fault Handling on ARM

When an unhandled exception fires on Cortex-M, the vector table calls `fault_hang()`. This function is written in inline assembly so it cannot be unwound or optimised away.

```rust
#[naked]
#[no_mangle]
#[link_section = ".text.fault"]
pub unsafe extern "C" fn fault_hang() {
    asm!(
        "movw r0, #0x2000",
        "movt r0, #0x4002",
        "ldr  r1, =0x4641554c",   // "FAUL"
        "str  r1, [r0, #0x04]",   // UART2_TDR
        "ldr  r1, =0x20543a20",   // "T: "
        "str  r1, [r0, #0x04]",
        "ldr  r1, =0x726f6320",   // " cor"
        "str  r1, [r0, #0x04]",
        "ldr  r1, =0x202c6565",   // "e, "
        "str  r1, [r0, #0x04]",
        "ldr  r1, =0x616c6168",   // "hala"
        "str  r1, [r0, #0x04]",
        "ldr  r1, =0x21646574",   // "ted!"
        "str  r1, [r0, #0x04]",
        "wfi",
        "b    .",
        options(noreturn)
    );
}
```

The message `**FAULT: core exception, halted**` is written character by character to the UART transmit data register, followed by `wfi` (wait-for-interrupt) and an infinite self-branch. The processor is effectively frozen.

## 6.3 RISC-V Trap Hang Stub

On RISC-V, the trap vector lands on `_trap_hang`, a single instruction:

```asm
.section .text.trap
.globl _trap_hang
_trap_hang:
    j _trap_hang
```

This is the simplest possible trap handler — an infinite loop. When a trap fires on RISC-V, the core jumps here and never returns. There is no UART output in this stub; diagnostics require JTAG or external debugger attachment.

## 6.4 Fault Register Diagnostic Dump

Before entering the hang loop on ARM, the fault handler can optionally dump the following registers to UART. These values are read from the stacked exception frame and the fault status registers.

| Register | Source | Description |
|----------|--------|-------------|
| `PC` | Stacked `EXC_RETURN` or LR | Program counter at time of fault |
| `LR` | Stacked LR | Link register |
| `R0` | Stacked R0 | General purpose register 0 |
| `R1` | Stacked R1 | General purpose register 1 |
| `R2` | Stacked R2 | General purpose register 2 |
| `R3` | Stacked R3 | General purpose register 3 |
| `R12` | Stacked R12 | General purpose register 12 (IP) |
| `SP` | Current SP | Stack pointer at time of fault |
| `CFSR` | `0xE000ED28` | Configurable Fault Status Register |
| `FAR` | `0xE000ED38` | Fault Address Register |

The `CFSR` value decodes into sub-registers:

- **CFSR[7:0]** — MemManage Fault Status (SCB_CFSR_MMFSR)
- **CFSR[15:8]** — BusFault Status (SCB_CFSR_BFSR)
- **CFSR[31:16]** — UsageFault Status (SCB_CFSR_UFSR)

A non-zero `FAR` on a MemManage or BusFault tells you the exact address that triggered the access violation.

## 6.5 Debugging with QEMU

QEMU provides two powerful mechanisms for field diagnostics without hardware.

### Instruction Trace

```bash
qemu-system-arm -M netduinoplus2 -nographic \
    -d in_asm -D /tmp/holy-rust.trace \
    -kernel target/thumbv7em-none-eabihf/release/holy-rust
```

The `-d in_asm` flag logs every translated basic block to the file specified by `-D`. This produces a full instruction-level trace of execution. The trace file can be enormous — filter it with `grep` for specific PC ranges.

### GDB Attach

```bash
qemu-system-arm -M netduinoplus2 -nographic \
    -gdb tcp::1234 -S \
    -kernel target/thumbv7em-none-eabihf/release/holy-rust
```

In a separate terminal:

```bash
arm-none-eabi-gdb target/thumbv7em-none-eabihf/release/holy-rust
(gdb) target remote :1234
(gdb) break fault_hang
(gdb) continue
(gdb) info registers
(gdb) x/10i $pc-16
```

The `-S` flag freezes the CPU at startup until GDB connects. From there you can set breakpoints, single-step, and inspect memory.

## 6.6 Common Fault Scenarios

### Wild Peek to Unmapped Address → HardFault

Typing `peek 0xDEAD0000;` on an address not mapped in the MPU triggers a BusFault or HardFault. The fault handler fires, writes the diagnostic message, and halts.

### Unclaimed Peripheral Poke → E001

Typing `poke 0x40020000 0xFF;` without first calling `claim GPIOA;` returns `E001 CAPABILITY_VIOLATION`. This is caught at parse time before any MMIO access occurs.

### Division by Zero → "DIV BY ZERO"

Typing `10 / 0;` or `10 % 0;` produces the error message `DIV BY ZERO`. The division routine checks the divisor operand and aborts before the hardware divide instruction executes.

### Stack Overflow → Silent Corruption

The kernel runs in Ring 0 with no guard pages. A deep recursive call chain or large stack allocations silently corrupt adjacent memory. There is no stack canary, no MPU-based stack limit, and no detection mechanism. The symptom is typically a HardFault at an unrelated address, making diagnosis difficult.

## 6.7 Formatted Output Functions

### `write_hex_u32`

Outputs a 32-bit value as an 8-character hexadecimal string with `0x` prefix.

```rust
fn write_hex_u32(mut val: u32) {
    uart_write_byte(b'0');
    uart_write_byte(b'x');
    let mut i = 8;
    while i > 0 {
        let nibble = (val >> 28) as u8;
        let c = if nibble < 10 { b'0' + nibble } else { b'A' + nibble - 10 };
        uart_write_byte(c);
        val <<= 4;
        i -= 1;
    }
}
```

### `write_dec_u32`

Outputs a 32-bit value as a decimal string with no leading zeros.

```rust
fn write_dec_u32(mut val: u32) {
    if val == 0 {
        uart_write_byte(b'0');
        return;
    }
    let mut buf = [0u8; 10];
    let mut i = 0;
    while val > 0 {
        buf[i] = b'0' + (val % 10) as u8;
        val /= 10;
        i += 1;
    }
    while i > 0 {
        i -= 1;
        uart_write_byte(buf[i]);
    }
}
```

Both functions are used throughout the REPL output path. The `write_value` function in Chapter 8 combines them into the standard `0xHEX (decimal)` output format.
