//! Hardware Abstraction Layer & REPL interfaces.
//!
//! - [`uart`] — bare-metal UART driver with static RX ring buffer.
//! - [`repl`] — ASCII terminal state machine and command handler.

pub mod repl;
pub mod uart;
