// Adapted from https://github.com/Alexhuszagh/rust-lexical.

//! Precalculated large powers for limbs.

#[cfg(not(any(target_arch = "aarch64", target_arch = "loongarch64", target_arch = "mips64", target_arch = "powerpc64", target_arch = "riscv64", target_arch = "wasm32", target_arch = "x86_64", target_pointer_width = "64")))]
pub(crate) use super::large_powers32::*;

#[cfg(any(target_arch = "aarch64", target_arch = "loongarch64", target_arch = "mips64", target_arch = "powerpc64", target_arch = "riscv64", target_arch = "wasm32", target_arch = "x86_64", target_pointer_width = "64"))]
pub(crate) use super::large_powers64::*;
