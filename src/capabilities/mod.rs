//! O(1) linear capability token engine.
//!
//! - [`registry`] — SRAM bitfield tracking resource ownership.
//! - [`tokens`] — non-copyable `Cap<T>` abstractions and the Holy HAL
//!   peripheral resource definitions.

pub mod registry;
pub mod tokens;
