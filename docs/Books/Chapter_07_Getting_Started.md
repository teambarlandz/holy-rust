# Chapter 7: Getting Started

## 7.1 Prerequisites

| Requirement | Minimum Version | Notes |
|-------------|----------------|-------|
| Rust toolchain | 1.97+ | Stable channel, installed via `rustup` |
| QEMU | 8.2+ | System emulators for ARM and RISC-V |
| Target components | — | Two bare-metal target triples (see below) |

Verify your installation:

```bash
rustc --version   # rustc 1.97.0 (aede1b12d 2025-01-16)
qemu-system-arm --version
qemu-system-riscv32 --version
```

## 7.2 Installing Targets

Both targets are bare-metal — no OS, no stdlib — so the `none` triples are used.

```bash
rustup target add thumbv7em-none-eabihf
rustup target add riscv32imac-unknown-none-elf
```

The `eabihf` suffix: embedded ABI, aligned, hardware FP. The `riscv32imac` suffix: base integer + multiply/divide + atomic + compressed instructions.

If you see `can't find crate for core`, install the missing component:

```bash
rustup component add rust-src
```

## 7.3 Building

```bash
cargo build --release --target thumbv7em-none-eabihf
cargo build --release --target riscv32imac-unknown-none-elf
```

Output binaries land in `target/<triple>/release/holy-rust`. These are raw ELF binaries — QEMU loads them directly via `-kernel`.

The release profile optimises aggressively for size:

```toml
[profile.release]
opt-level = "z"    # size optimisation
lto = true         # link-time optimisation
codegen-units = 1  # maximum inlining
panic = "abort"    # no unwinding
strip = true       # strip symbols
```

## 7.4 Running ARM in QEMU

```bash
qemu-system-arm -M netduinoplus2 -nographic \
    -kernel target/thumbv7em-none-eabihf/release/holy-rust
```

| Flag | Purpose |
|------|---------|
| `-M netduinoplus2` | Netduino Plus 2 board (STM32F205, Cortex-M3) |
| `-nographic` | UART to terminal, no GUI |
| `-kernel` | Load ELF into emulated flash |

Press `Ctrl-A X` to exit QEMU.

## 7.5 Running RISC-V in QEMU

```bash
qemu-system-riscv32 -M sifive_e -bios none -nographic \
    -kernel target/riscv32imac-unknown-none-elf/release/holy-rust
```

| Flag | Purpose |
|------|---------|
| `-M sifive_e` | SiFive FE310 board (RV32IMAC core) |
| `-bios none` | No bootloader — load kernel directly |
| `-nographic` | UART to terminal |
| `-kernel` | Load ELF into flash |

UART0 is at `0x10013000`. The `-bios none` flag is required — without it the kernel may not boot.

## 7.6 First Session

After launching QEMU, you will see:

```
HOLY RUST v0.1.0
bare-metal OS — type 'help;'
holy>
```

### Type `help;`

```
holy> help;
COMMANDS:
  peek <addr>;           — read 32-bit word from address
  poke <addr> <val>;     — write 32-bit word to address
  claim <periph>;        — claim peripheral token
  drop <periph>;         — release peripheral token
  let <name> = <expr>;   — bind a variable
  fn <name>(<args>) { <body> } — define a function
  help;                  — print this message
  banner;                — reprint boot banner
  audit;                 — dump SuperUser audit log
```

### Try a Peek

```
holy> peek 0x40020000;
0x00000000 (0)
```

This reads a 32-bit word from GPIOA MODER. The value depends on board state.

### Try Arithmetic

```
holy> 2+3;
= 0x00000005 (5)
```

Expressions evaluate left-to-right with no operator precedence. Results appear in both hex and decimal.

### Try a Variable Binding

```
holy> let x = 10;
holy> let y = 20;
holy> x + y;
= 0x0000001E (30)
```

Variables persist for the kernel's lifetime. Once bound, they cannot be reassigned.

## 7.7 The REPL Prompt

The prompt is `holy>`. Type a command and press Enter. Every statement must end with `;` and fit on a single line. There is no multi-line input and no command history.

## 7.8 Line Editing

| Key | Action |
|-----|--------|
| Printable characters | Append to line buffer |
| Backspace | Delete last character |
| `Ctrl-U` | Kill entire line, reprint prompt |
| `Ctrl-C` | Cancel line, reprint prompt |
| Enter | Submit line for evaluation |

The line buffer is 128 bytes. Characters beyond this limit are silently dropped.

## 7.9 Key Constraints

1. **128-byte max line** — Long expressions are truncated. Split complex computations across lines.
2. **Left-to-right evaluation** — `2+3*4` is `(2+3)*4 = 20`, not `14`. No operator precedence.
3. **All constants immutable** — `let` bindings cannot be reassigned. No `mut` at the REPL level.
4. **Semicolons required** — Omitting `;` produces `MISSING SEMICOLON`.
5. **Two functions max** — Exceeding it produces `FN TABLE FULL`.
6. **32 variables max** — Exceeding it produces `SYMBOL TABLE FULL`.
7. **16-byte name limit** — Longer identifiers produce `NAME TOO LONG`.
8. **No standard library** — No imports, no heap, no `println!`. Everything is bare-metal.
