//! #6386: direct lowering for DataView accessor method calls.
//!
//! `dv.getFloat64(off, le)` / `dv.setInt32(off, v)` on a receiver whose
//! STATIC type is `DataView` previously lowered to the fully generic
//! `js_typed_feedback_native_call_method_by_id` tower — per call: a method-id
//! resolution, a typed-feedback observation, an args `Vec` + handle-scope
//! setup, then the buffer-registry dispatch ladder (`is_registered_buffer` →
//! own-prop shadow probe → `is_data_view` → suffix re-parse). This lowers the
//! same calls to one `js_data_view_{get,set}_direct` call carrying the
//! pre-resolved element-kind code.
//!
//! The runtime entry re-checks the receiver (a variable whose static type
//! was violated at runtime, a shadowed method, exotica) and falls back to the
//! generic dispatcher, so this is a pure fast path — semantics unchanged.

use anyhow::Result;
use perry_hir::Expr;

use crate::expr::{lower_expr, FnCtx};
use crate::types::{DOUBLE, I32};

/// Classify a DataView accessor method name: `Some((is_set, kind_code))` for
/// the `get*`/`set*` numeric family. `kind_code` is the ABI contract with
/// `DataViewKind` in `perry-runtime/src/buffer/dataview.rs` (`repr(i32)`
/// discriminants) — keep the two in sync.
fn classify_data_view_accessor(method: &str) -> Option<(bool, i32)> {
    let (is_set, suffix) = if let Some(s) = method.strip_prefix("get") {
        (false, s)
    } else if let Some(s) = method.strip_prefix("set") {
        (true, s)
    } else {
        return None;
    };
    let kind_code = match suffix {
        "Int8" => 0,
        "Uint8" => 1,
        "Int16" => 2,
        "Uint16" => 3,
        "Int32" => 4,
        "Uint32" => 5,
        "Float32" => 6,
        "Float64" => 7,
        "BigInt64" => 8,
        "BigUint64" => 9,
        _ => return None,
    };
    Some((is_set, kind_code))
}

/// Try to lower `object.<property>(args)` as a direct DataView accessor call.
/// Returns `Ok(None)` when the method/arity doesn't match the direct form —
/// the generic dispatch path then handles it (missing REQUIRED arguments stay
/// on the generic path so its argument-defaulting behavior is preserved
/// exactly; extra arguments beyond the accessor's arity likewise).
pub(super) fn try_emit_data_view_accessor(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    property: &str,
    args: &[Expr],
    call_byte_offset: u32,
) -> Result<Option<String>> {
    let Some((is_set, kind_code)) = classify_data_view_accessor(property) else {
        return Ok(None);
    };
    let (min_args, max_args) = if is_set { (2, 3) } else { (1, 2) };
    if args.len() < min_args || args.len() > max_args {
        return Ok(None);
    }
    let recv = lower_expr(ctx, object)?;
    let mut lowered: Vec<String> = Vec::with_capacity(args.len());
    for a in args {
        lowered.push(lower_expr(ctx, a)?);
    }
    // Absent littleEndian lowers to undefined — the runtime evaluates its
    // truthiness exactly like the generic path's `truthy(args[2])`.
    let undef = ctx
        .block()
        .bitcast_i64_to_double(crate::nanbox::TAG_UNDEFINED_I64);
    // The accessors can throw (RangeError on an out-of-bounds offset, the
    // offset's `valueOf`) — record the call location for the error message.
    crate::expr::calls::emit_call_location_at(ctx, call_byte_offset);
    let argc = args.len().to_string();
    let kind = kind_code.to_string();
    let blk = ctx.block();
    let result = if is_set {
        let little = lowered.get(2).unwrap_or(&undef);
        blk.call(
            DOUBLE,
            "js_data_view_set_direct",
            &[
                (DOUBLE, &recv),
                (DOUBLE, &lowered[0]),
                (DOUBLE, &lowered[1]),
                (DOUBLE, little),
                (I32, &kind),
                (I32, &argc),
            ],
        )
    } else {
        let little = lowered.get(1).unwrap_or(&undef);
        blk.call(
            DOUBLE,
            "js_data_view_get_direct",
            &[
                (DOUBLE, &recv),
                (DOUBLE, &lowered[0]),
                (DOUBLE, little),
                (I32, &kind),
                (I32, &argc),
            ],
        )
    };
    Ok(Some(result))
}
