//! Ring 0 core infrastructure.
//!
//! - [`exec`]      — executable SRAM buffer and threaded-code dispatch.
//! - [`interrupt`] — dynamic vector table relocation and C-ABI trampolines.
//! - [`memory`] — direct volatile access primitives and boot-time
//!   `.data`/`.bss` initialization.

pub mod exec;
pub mod interrupt;
pub mod memory;

/// Boot banner printed to the UART console before the REPL attaches.
pub const BANNER: &[u8] = b"Holy Rust REPL v0.1\r\n";
