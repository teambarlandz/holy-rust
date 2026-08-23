//! hros-hal — mathematics-first HAL trait specs (no_std, zero-cost).
//! See HR-OS/AXIS-*.md, UPGRADE.md. Every impl must preserve cycle invariants.

#![no_std]
#![deny(unsafe_op_in_unsafe_fn)]
#![warn(clippy::undocumented_unsafe_blocks)]

pub mod cap;
pub mod exec;
pub mod irq;
pub mod switch;

pub trait Hal: Sized {
    type Switch: switch::ContextSwitch;
    type Irq: irq::InterruptController;
    type Cap: cap::VectorCapabilityEngine;
    type Exec: exec::ExecutionBuffer;
}
