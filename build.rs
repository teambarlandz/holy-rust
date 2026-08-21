//! Build script: selects and validates the architecture-specific linker
//! script, then hands it to rustc as a link argument.
//!
//! ARM builds use `memory.x` (STM32-style map, Flash @ 0x0800_0000) with
//! the shared section layout `memory-layout.x`, which emits a hardware
//! vector table at the flash base. RISC-V builds use `memory-riscv.x`
//! (SiFive E310 map, DTIM @ 0x8000_0000, code linked at 0x2040_0000 where
//! QEMU's boot ROM jumps) with `memory-layout-riscv.x`, which has no
//! vector table — Reset must be the first thing in flash.

use std::env;
use std::fs;
use std::path::Path;

fn main() {
    let arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_else(|_| String::from("arm"));

    let (script, layout) = match arch.as_str() {
        "riscv32" => ("memory-riscv.x", "memory-layout-riscv.x"),
        _ => ("memory.x", "memory-layout.x"),
    };

    // Validate that the selected linker script exists and is parseable.
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let path = Path::new(&manifest_dir).join(script);
    let contents = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {}", path.display(), e));

    if !contents.contains("ORIGIN") || !contents.contains("LENGTH") {
        panic!("{} must contain ORIGIN and LENGTH directives", script);
    }
    let include_directive = format!("INCLUDE {}", layout);
    if !contents.contains(&include_directive) {
        panic!("{} must INCLUDE {} (shared SECTIONS)", script, layout);
    }

    // The shared layout is part of the link contract too.
    let layout_path = Path::new(&manifest_dir).join(layout);
    if !layout_path.exists() {
        panic!("{} not found at {}", layout, layout_path.display());
    }

    println!("cargo:rerun-if-changed={}", script);
    println!("cargo:rerun-if-changed={}", layout);
    println!("cargo:rerun-if-env-changed=CARGO_CFG_TARGET_ARCH");
    println!("cargo:rustc-link-arg=-T{}", script);
}
