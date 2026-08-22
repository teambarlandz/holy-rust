# Chapter 8: REPL As Operating System

## 8.1 The REPL State Machine

The Holy Rust kernel is a REPL that never returns. Its lifecycle is a four-state loop: Idle → Reading → Evaluating → Printing → Idle. The state machine is implicit — there is no enum of states. The `run()` function is a `loop` that calls three phases in sequence: read, evaluate, print.

## 8.2 How Each State Works

### Idle / Reading

The UART receive loop collects bytes into a 128-byte static line buffer. Printable ASCII is appended and echoed. Backspace deletes the last character. `Ctrl-U` kills the line. `Ctrl-C` cancels and reprints the prompt. Newline marks the line complete.

The reading phase blocks until a full line is available. There is no timeout, no escape sequence processing, and no history buffer.

### Evaluating

When a complete line is available, `take_line()` transfers ownership of the line buffer to the compiler. The compiler tokenises the input, parses it, and produces a `StreamProgram` or an error code.

The compiler is a `static mut COMPILER` that owns the token stream, symbol table, and function table. These persist across REPL lines — this is how variable bindings and function definitions accumulate.

### Printing

The outcome of evaluation is an `Outcome` enum variant. The `execute` function matches on this variant and dispatches to the appropriate UART output routine. After printing, the REPL clears the buffer, prints `holy>`, and returns to idle.

## 8.3 The Line Buffer

```rust
static mut LINE_BUF: [u8; 128] = [0u8; 128];
static mut LINE_LEN: usize = 0;
```

A fixed 128-byte array accessed by a single thread. The `take_line()` function copies its contents into the compiler's input and resets the length counter. The buffer is not null-terminated.

## 8.4 The Compiler State

```rust
static mut COMPILER: Compiler = Compiler::new();

struct Compiler {
    symbol_names: [SymbolName; 32],
    symbol_values: [u32; 32],
    fn_names: [SymbolName; 2],
    fn_bodies: [FnBody; 2],
    fn_count: usize,
    symbol_count: usize,
}
```

The compiler is a `static mut` global that persists for the kernel's entire lifetime. When `compile()` is called, it tokenises the input, parses tokens against the grammar, and returns an `Outcome`. Variable bindings and function definitions are stored as side effects.

## 8.5 Why Function Definitions Survive Across Lines

A function defined on one line is available on the next:

```
holy> fn add(a,b){a+b;}
FN add DEFINED
holy> add(2,3);
= 0x00000005 (5)
```

The compiler owns `fn_names` and `fn_bodies`. On definition, it stores the name and body. On call, it looks up the name and inlines the body at the call site. The function table is only cleared on kernel reset.

## 8.6 The Outcome Enum

```rust
enum Outcome {
    Empty,
    Help,
    Banner,
    Bound { name: SymbolName, value: u32 },
    FnDefined { name: SymbolName },
    Run { result: StreamProgram },
    Claim { periph: PeripheralId },
    Drop { periph: PeripheralId },
    EnforcedPoke { addr: u32, value: u32 },
    EnforcedPeek { addr: u32 },
    SetBit { addr: u32, bit: u32 },
    ClrBit { addr: u32, bit: u32 },
    SysAudit,
}
```

Each variant maps to a specific output or side effect:

| Variant | Output / Effect |
|---------|-----------------|
| `Empty` | Nothing printed |
| `Help` | Prints command reference |
| `Banner` | Reprints boot banner |
| `Bound` | `"name = 0xVALUE (decimal)"` |
| `FnDefined` | `"FN name DEFINED"` |
| `Run` | Executes program, prints `"= 0xVALUE (decimal)"` or `"OK"` |
| `Claim` / `Drop` | Capability management |
| `EnforcedPoke` / `EnforcedPeek` | Capability-checked MMIO access |
| `SetBit` / `ClrBit` | Atomic register bit manipulation |
| `SysAudit` | Dumps SuperUser audit log |

## 8.7 The `write_value` Function

All numeric output flows through `write_value`, which formats a `u32` as `0xHEX (decimal)`:

```rust
fn write_value(val: u32) {
    write_hex_u32(val);
    uart_write_byte(b' ');
    uart_write_byte(b'(');
    write_dec_u32(val);
    uart_write_byte(b')');
}
```

Example: `= 0x00000005 (5)`. The dual format is useful because register values are best read in hex while arithmetic results are best read in decimal.

## 8.8 The `print_help` Function

```rust
fn print_help() {
    uart_write_str("COMMANDS:\r\n");
    uart_write_str("  peek <addr>;           — read 32-bit word\r\n");
    uart_write_str("  poke <addr> <val>;     — write 32-bit word\r\n");
    uart_write_str("  claim <periph>;        — claim peripheral token\r\n");
    uart_write_str("  drop <periph>;         — release peripheral token\r\n");
    uart_write_str("  let <name> = <expr>;   — bind a variable\r\n");
    uart_write_str("  fn <name>(<args>) { <body> } — define function\r\n");
    uart_write_str("  help;                  — print this message\r\n");
    uart_write_str("  banner;                — reprint boot banner\r\n");
    uart_write_str("  audit;                 — dump audit log\r\n");
}
```

Each line is a literal string pushed to UART. No formatting engine, no `format!` macro, no heap allocation.

## 8.9 The `execute` Function

```rust
fn execute(outcome: Outcome) {
    match outcome {
        Outcome::Empty => {}
        Outcome::Help => print_help(),
        Outcome::Banner => print_banner(),
        Outcome::Bound { name, value } => {
            uart_write_str(&name.as_str());
            uart_write_str(" = ");
            write_value(value);
            uart_write_str("\r\n");
        }
        Outcome::FnDefined { name } => {
            uart_write_str("FN ");
            uart_write_str(&name.as_str());
            uart_write_str(" DEFINED\r\n");
        }
        Outcome::Run { result } => {
            let val = result.execute();
            uart_write_str("= ");
            write_value(val);
            uart_write_str("\r\n");
        }
        Outcome::Claim { periph } => { /* claim token */ }
        Outcome::Drop { periph } => { /* drop token */ }
        Outcome::EnforcedPoke { addr, value } => { /* MMIO write */ }
        Outcome::EnforcedPeek { addr } => { /* MMIO read */ }
        Outcome::SetBit { addr, bit } => { /* atomic bit set */ }
        Outcome::ClrBit { addr, bit } => { /* atomic bit clear */ }
        Outcome::SysAudit => { /* dump audit log */ }
    }
}
```

The match is exhaustive. The `Run` variant calls `result.execute()` which walks the compiled token stream and returns the final value.

## 8.10 Why the REPL Never Returns

```rust
pub fn run() -> ! {
    print_banner();
    loop {
        print_prompt();
        let line = take_line();
        let outcome = compile(line);
        execute(outcome);
    }
}
```

The return type `!` (never type) means this function will never return. The REPL **is** the kernel. There is no `main()` that returns to an OS, no scheduler, no idle loop, no shutdown path. When `run()` is entered, it loops forever.

This is the defining architectural choice of Holy Rust: the REPL is not a layer on top of an OS — the REPL **is** the operating system. Every feature, from memory-mapped I/O to capability management to function definitions, is exposed through the REPL grammar.
