//! NaN-boxing tag constants and small instruction helpers used across the emit
//! sub-modules.
//!
//! Pure code-movement from `mod.rs`. The constants must match perry-runtime
//! and `wasm_runtime.js`.

use super::*;

#[derive(Clone)]
pub(super) enum EnumResolvedValue {
    Number(f64),
    String(String),
}

/// Helper: create an F64Const instruction from raw f64 bits
pub(super) fn f64_const(val: f64) -> Instruction<'static> {
    Instruction::F64Const(Ieee64::from(val))
}

/// Helper: create an F64Const instruction from NaN-boxed tag bits (kept for potential future use)
#[allow(dead_code)]
pub(super) fn f64_const_bits(bits: u64) -> Instruction<'static> {
    Instruction::F64Const(Ieee64::from(f64::from_bits(bits)))
}

// NaN-boxing constants (must match perry-runtime and wasm_runtime.js)
pub(super) const STRING_TAG: u64 = 0x7FFF;
pub(super) const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
pub(super) const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
pub(super) const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
pub(super) const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
