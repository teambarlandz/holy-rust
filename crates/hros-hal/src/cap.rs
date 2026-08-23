//! VectorCapabilityEngine — 256-bit SIMD bitmask validation (AXIS-3.md, UPGRADE.md).
//! Invariant P(a,C)=(W_{k>>6} >> (k &63)) &1 in O(1). Scalar 3 cyc → vector 1 cyc for 256×4 KiB = 1 MiB.

/// Capability token identifier (bit index N).
pub type CapId = u16;

/// 256-bit request mask (4×64) vs task vector `Vcap ∈ {0,1}²⁵⁶`.
/// `authorized = (Vcap & Mreq) == Mreq`.
#[repr(C, align(32))]
#[derive(Copy, Clone)]
pub struct Mask256(pub [u64; 4]);

/// O(1) vector capability engine — scalar + 256-bit SIMD paths.
pub trait VectorCapabilityEngine: Sized {
    /// Granularity shift `M` where block size `S=2^M` (HR-OS M=12 ⇒ 4 KiB).
    const SHIFT: u32 = 12;
    /// Max cycles scalar path.
    const CYCLES_SCALAR: usize = 3;
    /// Max cycles vector path (256 bits).
    const CYCLES_VECTOR: usize = 1;

    /// Scalar predicate `P(addr,C)` : bit-test one 4 KiB block.
    fn verify_scalar(addr: u32, vcap_base: *const u64) -> bool;

    /// Vector predicate for contiguous `len` blocks starting at `addr` (len ≤256).
    /// `mask` encodes the N requested bits. Returns true iff all required bits set.
    /// Must use 256-bit vector ALU when available (AVX2 VANDPS+VPTEST / NEON).
    fn verify_vector(addr: u32, mask: Mask256, vcap_base: *const u64) -> bool;

    /// Build `Mask256` for `[addr, addr+len*4096)`. Returns None if `len>256`.
    fn build_mask(addr: u32, len: usize) -> Option<Mask256>;

    /// Physical `addr` → `CapId` (None = SRAM/flash/unrestricted).
    fn addr_to_cap(addr: u32) -> Option<CapId>;

    /// Atomically claim `id` (`fetch_or`). False if already held.
    fn acquire(id: CapId) -> bool;
    /// Atomically release `id` (`fetch_and !mask`).
    fn release(id: CapId);
    /// True if `id` is free.
    fn available(id: CapId) -> bool;
}
