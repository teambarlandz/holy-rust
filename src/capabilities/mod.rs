//! O(1) linear capability token engine.
//!
//! - [`registry`] — SRAM bitfield tracking resource ownership and
//!   physical-address-to-capability resolution.
//! - [`tokens`] — non-copyable `Cap<T>` abstractions and the Holy HAL
//!   peripheral resource definitions.
//! - [`audit`] — fixed-size SuperUser audit ring buffer.

pub mod audit;
pub mod registry;
pub mod tokens;
