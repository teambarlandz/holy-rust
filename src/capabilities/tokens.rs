//! Linear capability tokens and the Holy HAL peripheral model.
//!
//! `Cap<T>` is a linear (affine) token: it is deliberately **not** `Copy`
//! and **not** `Clone`, so the compiler enforces single ownership of every
//! hardware resource. Note: negative impls (`impl !Copy`) are unstable, so
//! linearity is achieved by omission — attempting `.clone()` or an implicit
//! copy fails to compile exactly as the docs require.
//!
//! There is intentionally **no `Drop` impl**: releasing hardware is an
//! explicit, auditable act (`drop_cap`). A hidden release inside Drop would
//! undermine the linear contract.

use core::marker::PhantomData;

use crate::capabilities::registry;
use crate::kernel::memory;

/// A hardware resource addressable by the capability engine.
pub trait HardwareResource {
    /// Unique registry bit index (0..256).
    const RESOURCE_ID: u16;
    /// Stable name used by the REPL (`cap_claim GPIOA`).
    const NAME: &'static str;
}

/// Linear capability token granting exclusive access to resource `T`.
///
/// Move semantics only: passing or assigning transfers ownership; copying
/// is a compile error.
pub struct Cap<T: HardwareResource> {
    id: u16,
    _phantom: PhantomData<T>,
}

impl<T: HardwareResource> Cap<T> {
    /// Registry id backing this token.
    pub fn id(&self) -> u16 {
        self.id
    }

    /// Resource name (REPL display).
    pub fn name(&self) -> &'static str {
        T::NAME
    }
}

/// Claim exclusive ownership of resource `T` if it is free. O(1).
pub fn claim<T: HardwareResource>() -> Option<Cap<T>> {
    if registry::acquire(T::RESOURCE_ID as usize) {
        Some(Cap {
            id: T::RESOURCE_ID,
            _phantom: PhantomData,
        })
    } else {
        None
    }
}

/// Explicitly relinquish ownership, consuming the token. O(1).
pub fn drop_cap<T: HardwareResource>(cap: Cap<T>) {
    registry::release(cap.id as usize);
    // cap dropped here; no Drop impl exists, so nothing else runs.
}

/// Bypass availability checks and mint a token unconditionally.
///
/// For early boot code that must hand out tokens before the registry is
/// meaningful. The bit is still marked claimed so later claims fail.
///
/// # Safety
/// Caller guarantees no other live token for `T` exists.
pub unsafe fn steal<T: HardwareResource>() -> Cap<T> {
    registry::acquire(T::RESOURCE_ID as usize);
    Cap {
        id: T::RESOURCE_ID,
        _phantom: PhantomData,
    }
}

// ---------------------------------------------------------------------------
// System resource definitions
// ---------------------------------------------------------------------------

macro_rules! define_resource {
    ($name:ident, $id:expr, $label:expr) => {
        pub struct $name;
        impl HardwareResource for $name {
            const RESOURCE_ID: u16 = $id;
            const NAME: &'static str = $label;
        }
    };
}

define_resource!(GpioA, 0, "GPIOA");
define_resource!(GpioB, 1, "GPIOB");
define_resource!(Uart0, 2, "UART0");
define_resource!(Spi0, 3, "SPI0");
define_resource!(I2c0, 4, "I2C0");
define_resource!(Timer0, 5, "TIMER0");
define_resource!(Dma0, 6, "DMA0");
define_resource!(SuperUserCap, 31, "SUPERUSER");

/// Audit counter: increments on every SuperUserCap grant (doc ch.2 rule 5).
pub static SUPERUSER_AUDIT_COUNT: core::sync::atomic::AtomicU32 =
    core::sync::atomic::AtomicU32::new(0);

/// Grant the boot-only superuser token, recording the grant in the audit
/// counter. Returns None if already held.
pub fn grant_superuser() -> Option<Cap<SuperUserCap>> {
    use core::sync::atomic::Ordering;
    let cap = claim::<SuperUserCap>();
    if cap.is_some() {
        SUPERUSER_AUDIT_COUNT.fetch_add(1, Ordering::AcqRel);
    }
    cap
}

// ---------------------------------------------------------------------------
// Holy HAL virtual GPIO port model
// ---------------------------------------------------------------------------
//
// Until real PAC integration lands (hardware milestone), Holy Rust defines
// its own minimal port contract used consistently across QEMU targets:
//
//   BASE + 0x00  DATA_SET   write 1<<n to set pin n
//   BASE + 0x04  DATA_CLR   write 1<<n to clear pin n
//   BASE + 0x08  DATA_OUT   read current output latch
//   BASE + 0x10  DIR        direction mask (1 = output)
//
// This mirrors modern SET/CLR-style controllers (RP2040 SIO, SiFive GPIO)
// and gives atomic single-instruction pin updates.

/// A [`HardwareResource`] that is also a GPIO port with fixed base address.
pub trait GpioPort: HardwareResource {
    /// Port register block base address.
    const BASE: usize;
}

impl GpioPort for GpioA {
    const BASE: usize = 0x4002_0000;
}
impl GpioPort for GpioB {
    const BASE: usize = 0x4002_0400;
}

mod gpio_regs {
    pub const DATA_SET: usize = 0x00;
    pub const DATA_CLR: usize = 0x04;
    pub const DATA_OUT: usize = 0x08;
    pub const DIR: usize = 0x10;
}

/// Borrowed pin lease derived from a port capability.
///
/// The lease borrows the `Cap` for its lifetime — the port cannot be
/// dropped or re-leased while any pin guard is alive ("borrow-lease token",
/// doc ch.2). Implements `embedded_hal::digital::OutputPin` for generic
/// driver interop; inherent linear methods implement the roadmap's
/// consume-and-return style.
pub struct PinGuard<'a, T: GpioPort, const N: u8> {
    _cap: &'a mut Cap<T>,
}

impl<T: GpioPort, const N: u8> PinGuard<'_, T, N> {
    const MASK: u32 = 1u32 << N;

    fn enable_output(&self) {
        memory::reg_set_bit(T::BASE + gpio_regs::DIR, N);
    }

    /// Linear consume-and-return set-high (roadmap signature).
    #[inline(always)]
    pub fn set_high_linear(self) -> Self {
        self.enable_output();
        memory::poke_u32(T::BASE + gpio_regs::DATA_SET, Self::MASK);
        self
    }

    /// Linear consume-and-return set-low.
    #[inline(always)]
    pub fn set_low_linear(self) -> Self {
        self.enable_output();
        memory::poke_u32(T::BASE + gpio_regs::DATA_CLR, Self::MASK);
        self
    }

    /// Read back the output latch bit for this pin.
    pub fn level(&self) -> bool {
        memory::peek_u32(T::BASE + gpio_regs::DATA_OUT) & Self::MASK != 0
    }
}

impl<T: GpioPort> Cap<T> {
    /// Lease pin `N` of this port for the guard's lifetime.
    pub fn pin<const N: u8>(&mut self) -> PinGuard<'_, T, N> {
        PinGuard { _cap: self }
    }
}

// embedded-hal interop ------------------------------------------------------

/// HAL error type: operations on claimed hardware cannot fail at runtime
/// beyond misuse, so the error carries no payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HalError;

impl embedded_hal::digital::Error for HalError {
    fn kind(&self) -> embedded_hal::digital::ErrorKind {
        embedded_hal::digital::ErrorKind::Other
    }
}

impl<T: GpioPort, const N: u8> embedded_hal::digital::ErrorType for PinGuard<'_, T, N> {
    type Error = HalError;
}

impl<T: GpioPort, const N: u8> embedded_hal::digital::OutputPin for PinGuard<'_, T, N> {
    #[inline(always)]
    fn set_low(&mut self) -> Result<(), Self::Error> {
        memory::poke_u32(T::BASE + gpio_regs::DATA_CLR, Self::MASK);
        Ok(())
    }

    #[inline(always)]
    fn set_high(&mut self) -> Result<(), Self::Error> {
        memory::poke_u32(T::BASE + gpio_regs::DATA_SET, Self::MASK);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// REPL name resolution
// ---------------------------------------------------------------------------

/// Resolve a resource by its stable name (used by `cap_claim`/`cap_drop`).
pub fn resolve_name(name: &[u8]) -> Option<u16> {
    const TABLE: &[(&[u8], u16)] = &[
        (b"GPIOA", 0),
        (b"GPIOB", 1),
        (b"UART0", 2),
        (b"SPI0", 3),
        (b"I2C0", 4),
        (b"TIMER0", 5),
        (b"DMA0", 6),
        (b"SUPERUSER", 31),
    ];
    TABLE.iter().find(|(n, _)| *n == name).map(|(_, id)| *id)
}
