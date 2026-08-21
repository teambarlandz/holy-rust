//! Fixed-size capability bitfield registry.
//!
//! One bit per hardware resource, resident in SRAM at `__capreg_base`
//! (SRAM + 0x1000) via the `.capability_registry` link section — matching
//! the address contract in docs/CHAPTER_02.
//!
//! All operations are O(1). Atomics are used instead of the doc's plain
//! read-modify-write so acquire/release stay correct even if an interrupt
//! handler races the REPL; on single-core silicon this compiles to simple
//! load/store-with-reservation sequences.

use core::sync::atomic::{AtomicU32, Ordering};

/// Total tracked resources (8 words x 32 bits).
pub const MAX_RESOURCES: usize = 256;

const WORDS: usize = MAX_RESOURCES / 32;

/// Capability availability bitmap. Bit set = resource claimed.
///
/// Wrapped in a struct to carry `#[repr(align(4))]` (repr attributes do
/// not apply to statics directly).
#[repr(C, align(4))]
pub struct RegistryBits(pub [AtomicU32; WORDS]);

#[used]
#[link_section = ".capability_registry"]
pub static REGISTRY_BITS: RegistryBits = RegistryBits([const { AtomicU32::new(0) }; WORDS]);

/// Returns true when no owner holds `resource_id` (single-bit lookup).
#[inline(always)]
pub fn available(resource_id: usize) -> bool {
    let word = resource_id / 32;
    let bit = resource_id % 32;
    // SAFETY-free: bounds enforced by modulo against a compile-time-sized
    // array; index < WORDS always holds for resource_id < MAX_RESOURCES.
    match REGISTRY_BITS.0.get(word) {
        Some(w) => w.load(Ordering::Acquire) & (1u32 << bit) == 0,
        None => false,
    }
}

/// Atomically claim `resource_id`. Returns false if already claimed.
///
/// O(1): one fetch-or test-and-set. On loss the bit is left set (it was
/// already set by the winner), so state stays consistent.
#[inline(always)]
pub fn acquire(resource_id: usize) -> bool {
    let word = resource_id / 32;
    let bit = resource_id % 32;
    match REGISTRY_BITS.0.get(word) {
        Some(w) => {
            let mask = 1u32 << bit;
            let prev = w.fetch_or(mask, Ordering::AcqRel);
            prev & mask == 0
        }
        None => false,
    }
}

/// Atomically release `resource_id` (clear its bit). O(1).
#[inline(always)]
pub fn release(resource_id: usize) {
    let word = resource_id / 32;
    let bit = resource_id % 32;
    if let Some(w) = REGISTRY_BITS.0.get(word) {
        w.fetch_and(!(1u32 << bit), Ordering::AcqRel);
    }
}
