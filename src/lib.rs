//! # HOLY RUST
//!
//! A single-address-space, Ring-0 bare-metal interactive operating
//! environment and single-pass streaming JIT compiler for ARM Cortex-M and
//! RISC-V microcontrollers.
//!
//! Module map (see `RoadMap.md` and `docs/` for the full architecture):
//!
//! - [`kernel`] — Ring 0 core: SRAM execution engine, vector table
//!   relocation, volatile memory primitives.
//! - [`capabilities`] — O(1) linear capability token engine (`Cap<T>`).
//! - [`compiler`] — zero-allocation streaming lexer/parser, threaded
//!   micro-primitives, Thumb-2 / RV32I emitters.
//! - [`drivers`] — bare-metal UART driver and the interactive REPL.

#![cfg_attr(not(feature = "std"), no_std)]

pub mod capabilities;
pub mod compiler;
pub mod drivers;
pub mod kernel;
