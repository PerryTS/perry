//! Inline NaN-box / unbox helpers (extracted from `expr.rs`, issue
//! #1098). Pure move — no logic changes.

use crate::block::LlBlock;
use crate::nanbox::{BIGINT_TAG_I64, INT32_TAG_I64, POINTER_TAG_I64, STRING_TAG_I64};
use crate::types::{DOUBLE, F32, I1, I32, I64};

/// The one NaN a Perry value is allowed to be: `0x7FF8_0000_0000_0000`.
/// Emitted as LLVM's hexadecimal double form so the parser cannot round-trip
/// the payload away.
const CANONICAL_QNAN_DOUBLE: &str = "0x7FF8000000000000";

/// `PERRY_NANBOX_CANON` gate (#10779). Enabled by default; `=0`/`off`/`false`
/// emits the pre-fix IR byte-for-byte, so the cost of the fix can be measured
/// with ONE compiler binary and no cross-build confound. Keyed into the object
/// cache alongside the other repsel gates; a measurement must still run with
/// `PERRY_NO_CACHE=1`.
pub(crate) fn nanbox_canon_enabled() -> bool {
    use std::sync::OnceLock;
    static CACHED: OnceLock<bool> = OnceLock::new();
    *CACHED.get_or_init(|| {
        !matches!(
            std::env::var("PERRY_NANBOX_CANON").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        )
    })
}

/// #10779: collapse any NaN in a raw native `f64` — an ArrayBuffer float lane,
/// a POD record field, or a C function's `double` return — to the canonical
/// quiet NaN, so it cannot alias a NaN-box tag.
///
/// The runtime twin is `perry_runtime::array::canonical_raw_f64`, whose doc
/// comment carries the full argument for why EVERY NaN must be collapsed and
/// not just the ones already inside the band (signalling NaNs move into it
/// under arithmetic; negative payload NaNs move into it under `fneg`/`fabs`).
///
/// Apply this ONLY to a lane read out of an `ArrayBuffer` — a `Float64Array` /
/// `Float32Array` / `Float16Array` element or a `DataView` float read. A plain
/// JS `Array<number>` raw-f64 slot is already canonical by the store-side
/// invariant (`js_array_numeric_value_to_raw_f64` /
/// `canonicalize_array_numeric_store_bits`), so adding it there would be pure
/// cost. Integer element kinds cannot produce a NaN at all.
///
/// Costs one `fcmp uno` + one `select`, which LLVM lowers to a
/// `vcmpunordsd`/`vblendvpd` pair on x86-64-v3 with the NaN constant hoisted
/// out of any enclosing loop.
pub(crate) fn canonicalize_lane_f64(blk: &mut LlBlock, value: &str) -> String {
    if !nanbox_canon_enabled() {
        return value.to_string();
    }
    // `fcmp` emits no fast-math flags (see `block.rs`), so `uno` survives.
    let is_nan = blk.fcmp("uno", value, value);
    blk.select(I1, &is_nan, DOUBLE, CANONICAL_QNAN_DOUBLE, value)
}

/// The `float` twin of [`canonicalize_lane_f64`], for a lane that is still an
/// `f32` in the native lattice and will be `fpext`ed later. An `f32` NaN widens
/// to an `f64` NaN that KEEPS its payload (`0x7FFFFFFF` becomes
/// `0x7FFF_FFFF_E000_0000`, a forged string pointer), so canonicalising before
/// the widen is equivalent and costs the same. LLVM spells a `float` constant
/// in hex using its DOUBLE bit pattern, so the canonical `f32` qNaN
/// `0x7FC00000` is written `0x7FF8000000000000`.
pub(crate) fn canonicalize_lane_f32(blk: &mut LlBlock, value: &str) -> String {
    if !nanbox_canon_enabled() {
        return value.to_string();
    }
    // MUST be `fcmp uno float`, not the `double` default: the operand is an
    // f32 in the native lattice. Emitting `double` here made every program
    // calling `Buffer.readFloatLE` fail codegen.
    let is_nan = blk.fcmp_ty(F32, "uno", value, value);
    blk.select(I1, &is_nan, F32, CANONICAL_QNAN_DOUBLE, value)
}

/// Inline NaN-box of a raw heap pointer with `POINTER_TAG`.
pub(crate) fn nanbox_pointer_inline(blk: &mut LlBlock, ptr_i64: &str) -> String {
    let tagged = blk.or(I64, ptr_i64, POINTER_TAG_I64);
    blk.bitcast_i64_to_double(&tagged)
}

/// Inline NaN-box of a raw `BigIntHeader*` with `BIGINT_TAG`. Required
/// for `typeof x === "bigint"` (which reads the tag byte), and for the
/// runtime's dynamic-dispatch helpers (`js_dynamic_add` etc.) to
/// recognize the value as a bigint at their check sites. Without this,
/// literals like `5n` get tagged as `POINTER_TAG` and `typeof` reports
/// `"object"` / arithmetic falls back to float and returns `NaN`.
pub(crate) fn nanbox_bigint_inline(blk: &mut LlBlock, ptr_i64: &str) -> String {
    let tagged = blk.or(I64, ptr_i64, BIGINT_TAG_I64);
    blk.bitcast_i64_to_double(&tagged)
}

/// Alias kept for backwards compatibility with existing callers
/// in `stmt.rs` and `codegen.rs` that use the `_pub` suffix.
pub(crate) fn nanbox_pointer_inline_pub(blk: &mut LlBlock, ptr_i64: &str) -> String {
    nanbox_pointer_inline(blk, ptr_i64)
}

/// Inline NaN-box of a raw string handle with `STRING_TAG`.
pub(crate) fn nanbox_string_inline(blk: &mut LlBlock, ptr_i64: &str) -> String {
    let tagged = blk.or(I64, ptr_i64, STRING_TAG_I64);
    blk.bitcast_i64_to_double(&tagged)
}

/// Convert an i32 boolean (0 or 1) returned by a runtime function into a
/// NaN-tagged JSValue boolean (`TAG_TRUE` / `TAG_FALSE`).
pub(crate) fn i32_bool_to_nanbox(blk: &mut LlBlock, i32_val: &str) -> String {
    let bit = blk.icmp_ne(I32, i32_val, "0");
    let tagged = blk.select(
        I1,
        &bit,
        I64,
        crate::nanbox::TAG_TRUE_I64,
        crate::nanbox::TAG_FALSE_I64,
    );
    blk.bitcast_i64_to_double(&tagged)
}

/// Inline NaN-box of a raw signed i32. The low 32 payload bits are interpreted
/// as `u32`, matching `JSValue::int32` in the runtime.
pub(crate) fn i32_to_nanbox(blk: &mut LlBlock, i32_val: &str) -> String {
    let payload = blk.zext(I32, i32_val, I64);
    let tagged = blk.or(I64, &payload, INT32_TAG_I64);
    blk.bitcast_i64_to_double(&tagged)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::{LlBlock, RegCounter};
    use crate::inst::LlInst;
    use crate::types::DOUBLE;
    use std::rc::Rc;

    fn blk() -> LlBlock {
        LlBlock::new("t", Rc::new(RegCounter::new()))
    }

    /// #10779 follow-up. `LlBlock::fcmp` renders its operand type as `double`
    /// unconditionally, so canonicalising an f32 lane through it emitted
    /// `fcmp uno double %f32` — IR LLVM rejects with
    /// "'%r' defined with type 'float' but expected 'double'". Every program
    /// calling `Buffer.readFloatLE` failed codegen, and no fixture in the
    /// suite called it, so nothing caught it.
    ///
    /// The sabotage is one word: change `F32` back to `DOUBLE` in
    /// `canonicalize_lane_f32` and this test fails, naming the emitted type.
    #[test]
    fn f32_canonicalisation_compares_as_float_not_double() {
        let mut b = blk();
        let out = canonicalize_lane_f32(&mut b, "%x");
        assert_ne!(out, "%x", "the f32 lane must actually be canonicalised");
        let fcmp = b
            .insts()
            .iter()
            .find_map(|i| match i {
                LlInst::FCmp { pred, ty, .. } => Some((pred.clone(), *ty)),
                _ => None,
            })
            .expect("canonicalize_lane_f32 must emit an fcmp");
        assert_eq!(fcmp.0, "uno", "the NaN test must be an unordered compare");
        assert_eq!(
            fcmp.1, F32,
            "an f32 lane must be compared AS float; `double` here is IR LLVM \
             rejects and it broke every `Buffer.readFloatLE` call site"
        );
    }

    /// The f64 twin, so a future edit cannot fix the f32 case by widening
    /// both to `float`.
    #[test]
    fn f64_canonicalisation_compares_as_double() {
        let mut b = blk();
        let out = canonicalize_lane_f64(&mut b, "%x");
        assert_ne!(out, "%x");
        let ty = b
            .insts()
            .iter()
            .find_map(|i| match i {
                LlInst::FCmp { ty, .. } => Some(*ty),
                _ => None,
            })
            .expect("canonicalize_lane_f64 must emit an fcmp");
        assert_eq!(ty, DOUBLE);
    }

    /// Both canonicalisers must select the SAME canonical quiet NaN, spelled
    /// in LLVM's hex double form so the payload cannot be rounded away, and
    /// must select it on the TRUE (is-NaN) arm.
    #[test]
    fn both_select_the_canonical_quiet_nan_on_the_nan_arm() {
        for (name, want_ty) in [("f64", DOUBLE), ("f32", F32)] {
            let mut b = blk();
            if name == "f64" {
                let _ = canonicalize_lane_f64(&mut b, "%x");
            } else {
                let _ = canonicalize_lane_f32(&mut b, "%x");
            }
            let sel = b
                .insts()
                .iter()
                .find_map(|i| match i {
                    LlInst::Select { ty, a, b: fb, .. } => Some((*ty, a.clone(), fb.clone())),
                    _ => None,
                })
                .unwrap_or_else(|| panic!("{name} must emit a select"));
            assert_eq!(sel.0, want_ty, "{name} select operand type");
            assert_eq!(
                sel.1, CANONICAL_QNAN_DOUBLE,
                "{name} must pick the canonical quiet NaN when the value IS a NaN"
            );
            assert_eq!(sel.2, "%x", "{name} must pass a non-NaN through unchanged");
        }
    }
}
