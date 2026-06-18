//! i32-native expression fast path + flat-const 2D-table lowering
//! (extracted from `expr.rs`, issue #1098). Pure move — no logic changes.

use anyhow::Result;
use perry_hir::{BinaryOp, Expr};

use super::{lower_expr, unbox_to_i64, FlatConstInfo, FnCtx};
use crate::native_value::{
    materialize_js_value_bits, ExpectedNativeRep, LoweredValue, MaterializationReason,
};
use crate::type_analysis::{expr_may_return_boxed_value_from_raw_f64_fallback, is_numeric_expr};
use crate::types::{DOUBLE, F32, I32, I64};

/// Returns true if `e` is guaranteed to produce a finite double value
/// (not NaN, not ±Infinity). Used to skip the NaN/Inf guard in `toint32`
/// for integer-arithmetic hot paths — saving 5 instructions per bitwise op.
pub(crate) fn is_known_finite(ctx: &FnCtx<'_>, e: &Expr) -> bool {
    match e {
        Expr::Integer(_)
        | Expr::PodLayoutSizeOf { .. }
        | Expr::PodLayoutAlignOf { .. }
        | Expr::PodLayoutOffsetOf { .. } => true,
        // Number literals can be NaN or ±Infinity (e.g., `Number(NaN)`,
        // `Number(f64::INFINITY)`). Inspect the value: only true f64
        // finites can use the toint32_fast path. Without this check
        // `(NaN) | 0` and `(Infinity) | 0` hit fast-path `fptosi NaN`,
        // which is poison in LLVM and produced subnormal-double output
        // (which downstream code interpreted as a NaN-boxed string with
        // STRING_TAG bits, leading to garbled `console.log` output).
        Expr::Number(n) => n.is_finite(),
        Expr::LocalGet(id) => {
            ctx.integer_locals.contains(id) || ctx.unsigned_i32_locals.contains(id)
        }
        Expr::Update { id, .. } => {
            ctx.integer_locals.contains(id) || ctx.unsigned_i32_locals.contains(id)
        }
        Expr::Uint8ArrayGet { .. } | Expr::BufferIndexGet { .. } => true,
        Expr::MathImul(_, _) => true, // Math.imul returns i32 → always finite
        Expr::Call { callee, .. } => {
            matches!(callee.as_ref(), Expr::FuncRef(fid) if ctx.integer_returning_functions.contains(fid))
        }
        Expr::Binary { op, left, right } => match op {
            BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul => {
                is_known_finite(ctx, left) && is_known_finite(ctx, right)
            }
            BinaryOp::BitAnd
            | BinaryOp::BitOr
            | BinaryOp::BitXor
            | BinaryOp::Shl
            | BinaryOp::Shr
            | BinaryOp::UShr => true,
            _ => false,
        },
        _ => false,
    }
}

/// (Issue #50) If `IndexGet { object, index }` is a flat-const access
/// (inline `X[i][j]` or aliased `krow[j]`), lower it directly against
/// the `[N x i32]` global and return the NaN-boxed-double form of the
/// element. Returns `Ok(None)` when the pattern doesn't apply.
pub(crate) fn try_lower_flat_const_index_get(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    index: &Expr,
) -> Result<Option<String>> {
    let (info, row_expr, col_expr): (FlatConstInfo, Box<Expr>, Box<Expr>) = match object {
        // Inline: IndexGet(IndexGet(LocalGet(X), i), j)
        Expr::IndexGet {
            object: outer_obj,
            index: outer_idx,
        } => {
            if let Expr::LocalGet(id) = outer_obj.as_ref() {
                if let Some(info) = ctx.flat_const_arrays.get(id).cloned() {
                    (info, outer_idx.clone(), Box::new(index.clone()))
                } else {
                    return Ok(None);
                }
            } else {
                return Ok(None);
            }
        }
        // Aliased: IndexGet(LocalGet(krow), j) where krow was init'd
        // as `IndexGet(LocalGet(X), i)` for a flat-const X.
        Expr::LocalGet(alias_id) => {
            if let Some((const_id, row_expr)) = ctx.array_row_aliases.get(alias_id).cloned() {
                if let Some(info) = ctx.flat_const_arrays.get(&const_id).cloned() {
                    (info, row_expr, Box::new(index.clone()))
                } else {
                    return Ok(None);
                }
            } else {
                return Ok(None);
            }
        }
        _ => return Ok(None),
    };

    // A string-keyed access (`m["1"]["0"]`) must NOT take the integer flat
    // path: `fptosi` on a NaN-boxed string collapses to index 0, so every
    // string-keyed read returned the matrix's element 0. Bail to the caller's
    // tag-aware dispatch, which resolves a canonical numeric-string index to
    // the real element (`m` itself materializes as a heap array; only the
    // separately-tracked `const row = m[i]` alias does not). Proven-numeric /
    // loop-counter indices keep the flat path.
    let row_is_string = matches!(row_expr.as_ref(), Expr::String(_))
        || crate::type_analysis::is_string_expr(ctx, &row_expr);
    let col_is_string = matches!(col_expr.as_ref(), Expr::String(_))
        || crate::type_analysis::is_string_expr(ctx, &col_expr);
    if row_is_string || col_is_string {
        return Ok(None);
    }

    // Compute `row_i32` and `col_i32` as i32 SSA values. Use the existing
    // integer lowering when possible (both operands are likely small
    // loop-derived values); otherwise fall back to the double path and
    // fptosi.
    let i32_slots = ctx.i32_counter_slots.clone();
    let flat_ca = ctx.flat_const_arrays.clone();
    let ara = ctx.array_row_aliases.clone();
    let int_locals = ctx.integer_locals.clone();
    let row_i32 = if can_lower_expr_as_i32(
        &row_expr,
        &i32_slots,
        &flat_ca,
        &ara,
        &int_locals,
        ctx.clamp3_functions,
        ctx.clamp_u8_functions,
        ctx.integer_returning_functions,
        ctx.i32_identity_functions,
    ) {
        lower_expr_as_i32(ctx, &row_expr)?
    } else {
        let d = lower_expr(ctx, &row_expr)?;
        ctx.block().fptosi(DOUBLE, &d, I32)
    };
    let col_i32 = if can_lower_expr_as_i32(
        &col_expr,
        &i32_slots,
        &flat_ca,
        &ara,
        &int_locals,
        ctx.clamp3_functions,
        ctx.clamp_u8_functions,
        ctx.integer_returning_functions,
        ctx.i32_identity_functions,
    ) {
        lower_expr_as_i32(ctx, &col_expr)?
    } else {
        let d = lower_expr(ctx, &col_expr)?;
        ctx.block().fptosi(DOUBLE, &d, I32)
    };

    // flat_idx = row * cols + col  (i32)
    let blk = ctx.block();
    let cols_str = info.cols.to_string();
    let row_scaled = blk.mul(I32, &row_i32, &cols_str);
    let flat_idx = blk.add(I32, &row_scaled, &col_i32);

    // GEP into the `[N x i32]` global: ptr = &global[0][flat_idx]
    let reg = blk.fresh_reg();
    let n = info.rows * info.cols;
    let ty = format!("[{} x i32]", n);
    blk.emit_raw(format!(
        "{} = getelementptr inbounds {}, ptr @{}, i32 0, i32 {}",
        reg, ty, info.global_name, flat_idx
    ));
    let v_i32 = blk.load(I32, &reg);
    Ok(Some(blk.sitofp(I32, &v_i32, DOUBLE)))
}

/// (Issue #50) Detect module-level `const X = [[int, ...], ...]` that
/// qualifies as a flat-const 2D int array: rectangular shape, all
/// elements are `Expr::Integer(n)` with n in i32, at least 1 row.
/// Returns (rows, cols, flat_values).
pub(crate) fn try_flat_const_2d_int(e: &Expr) -> Option<(usize, usize, Vec<i32>)> {
    let rows = match e {
        Expr::Array(r) => r,
        _ => return None,
    };
    if rows.is_empty() {
        return None;
    }
    let mut cols: Option<usize> = None;
    let mut vals = Vec::new();
    for row in rows {
        let row_elems = match row {
            Expr::Array(re) => re,
            _ => return None,
        };
        match cols {
            None => cols = Some(row_elems.len()),
            Some(c) if c != row_elems.len() => return None,
            _ => {}
        }
        for el in row_elems {
            match el {
                Expr::Integer(n) => {
                    let v = i32::try_from(*n).ok()?;
                    vals.push(v);
                }
                _ => return None,
            }
        }
    }
    Some((rows.len(), cols?, vals))
}

/// (Issue #49) Return `true` if `e` can be lowered as an i32-native
/// expression: every leaf is sourced from an i32 slot, a typed-array byte
/// load, or an integer literal, and the combining operators are
/// `Add/Sub/Mul`. Used by the `LocalSet` fast path to decide whether the
/// rhs can bypass the fp round-trip.
///
/// The fallback `lower_expr_as_i32` path is fptosi(lower_expr()), which
/// handles Uint8ArrayGet / BufferIndexGet (their existing lowering already
/// produces an i32 → sitofp → double chain that LLVM's instcombine
/// collapses). We only commit to the fast path when every leaf is
/// recognizably int-sourced so the overall rhs lowers to a short chain of
/// `add/sub/mul i32` instructions.
pub(crate) fn can_lower_expr_as_i32(
    e: &Expr,
    i32_slots: &std::collections::HashMap<u32, String>,
    flat_const_arrays: &std::collections::HashMap<u32, FlatConstInfo>,
    array_row_aliases: &std::collections::HashMap<u32, (u32, Box<Expr>)>,
    integer_locals: &std::collections::HashSet<u32>,
    clamp3_fns: &std::collections::HashSet<u32>,
    clamp_u8_fns: &std::collections::HashSet<u32>,
    integer_returning_fns: &std::collections::HashSet<u32>,
    i32_identity_fns: &std::collections::HashSet<u32>,
) -> bool {
    match e {
        Expr::Integer(n) => i32::try_from(*n).is_ok(),
        Expr::LocalGet(id) => i32_slots.contains_key(id) || integer_locals.contains(id),
        Expr::Uint8ArrayGet { .. } | Expr::BufferIndexGet { .. } => true,
        Expr::MathImul(a, b) => {
            can_lower_expr_as_i32(
                a,
                i32_slots,
                flat_const_arrays,
                array_row_aliases,
                integer_locals,
                clamp3_fns,
                clamp_u8_fns,
                integer_returning_fns,
                i32_identity_fns,
            ) && can_lower_expr_as_i32(
                b,
                i32_slots,
                flat_const_arrays,
                array_row_aliases,
                integer_locals,
                clamp3_fns,
                clamp_u8_fns,
                integer_returning_fns,
                i32_identity_fns,
            )
        }
        Expr::Binary {
            op: BinaryOp::BitOr,
            left,
            right,
        } if matches!(right.as_ref(), Expr::Integer(0)) => can_lower_expr_as_i32(
            left,
            i32_slots,
            flat_const_arrays,
            array_row_aliases,
            integer_locals,
            clamp3_fns,
            clamp_u8_fns,
            integer_returning_fns,
            i32_identity_fns,
        ),
        Expr::Binary { op, left, right }
            if matches!(
                op,
                BinaryOp::Add
                    | BinaryOp::Sub
                    | BinaryOp::Mul
                    | BinaryOp::BitAnd
                    | BinaryOp::BitOr
                    | BinaryOp::BitXor
                    | BinaryOp::Shl
                    | BinaryOp::Shr
                    | BinaryOp::UShr
            ) =>
        {
            can_lower_expr_as_i32(
                left,
                i32_slots,
                flat_const_arrays,
                array_row_aliases,
                integer_locals,
                clamp3_fns,
                clamp_u8_fns,
                integer_returning_fns,
                i32_identity_fns,
            ) && can_lower_expr_as_i32(
                right,
                i32_slots,
                flat_const_arrays,
                array_row_aliases,
                integer_locals,
                clamp3_fns,
                clamp_u8_fns,
                integer_returning_fns,
                i32_identity_fns,
            )
        }
        Expr::Call { callee, args, .. } => {
            if let Expr::FuncRef(fid) = callee.as_ref() {
                if (clamp3_fns.contains(fid) && args.len() == 3)
                    || (clamp_u8_fns.contains(fid) && args.len() == 1)
                    || integer_returning_fns.contains(fid)
                {
                    if integer_returning_fns.contains(fid)
                        && !clamp3_fns.contains(fid)
                        && !clamp_u8_fns.contains(fid)
                        && !i32_identity_fns.contains(fid)
                    {
                        return false;
                    }
                    return args.iter().all(|a| {
                        can_lower_expr_as_i32(
                            a,
                            i32_slots,
                            flat_const_arrays,
                            array_row_aliases,
                            integer_locals,
                            clamp3_fns,
                            clamp_u8_fns,
                            integer_returning_fns,
                            i32_identity_fns,
                        )
                    });
                }
            }
            false
        }
        // Issue #50 bridge: element of a flat-const 2D int table.
        Expr::IndexGet { object, .. } => match object.as_ref() {
            Expr::IndexGet { object: inner, .. } => {
                matches!(inner.as_ref(), Expr::LocalGet(id) if flat_const_arrays.contains_key(id))
            }
            Expr::LocalGet(id) => array_row_aliases
                .get(id)
                .is_some_and(|(cid, _)| flat_const_arrays.contains_key(cid)),
            _ => false,
        },
        _ => false,
    }
}

/// Typed native-expression lowering entry point. It deliberately returns a
/// `LoweredValue` so callers keep the JS semantic meaning separate from the
/// LLVM representation chosen for the hot path.
pub(crate) fn lower_expr_native(
    ctx: &mut FnCtx<'_>,
    e: &Expr,
    expected: ExpectedNativeRep,
) -> Result<LoweredValue> {
    match expected {
        ExpectedNativeRep::JsValueBits => lower_expr_native_js_value_bits(ctx, e),
        ExpectedNativeRep::I32 => lower_expr_native_i32(ctx, e),
        ExpectedNativeRep::I64 => lower_expr_native_i64(ctx, e),
        ExpectedNativeRep::U32 => lower_expr_native_u32(ctx, e),
        ExpectedNativeRep::U64 => lower_expr_native_u64(ctx, e),
        ExpectedNativeRep::USize => lower_expr_native_usize(ctx, e),
        ExpectedNativeRep::F64 => lower_expr_native_f64(ctx, e),
        ExpectedNativeRep::F32 => lower_expr_native_f32(ctx, e),
        ExpectedNativeRep::BufferLen => lower_expr_native_buffer_len(ctx, e),
        ExpectedNativeRep::HandleId => lower_expr_native_handle_id(ctx, e),
        ExpectedNativeRep::NativeHandle => lower_expr_native_handle(ctx, e),
        ExpectedNativeRep::PromiseBoundary => lower_expr_native_promise_boundary(ctx, e),
    }
}

/// (Issue #49) Lower `e` as an i32 SSA value. Must be called only after
/// `can_lower_expr_as_i32` returned true for the same expression.
pub(crate) fn lower_expr_as_i32(ctx: &mut FnCtx<'_>, e: &Expr) -> Result<String> {
    Ok(lower_expr_native(ctx, e, ExpectedNativeRep::I32)?.value)
}

fn i32_lowered(value: String) -> LoweredValue {
    LoweredValue::i32(value)
}

fn i64_lowered(value: String) -> LoweredValue {
    LoweredValue::i64(value)
}

fn u32_lowered(value: String) -> LoweredValue {
    LoweredValue::u32(value)
}

fn u64_lowered(value: String) -> LoweredValue {
    LoweredValue::u64(value)
}

fn usize_lowered(value: String) -> LoweredValue {
    LoweredValue::usize(value)
}

fn f64_lowered(value: String) -> LoweredValue {
    LoweredValue::f64(value)
}

fn f32_lowered(value: String) -> LoweredValue {
    LoweredValue::f32(value)
}

fn buffer_len_lowered(value: String) -> LoweredValue {
    LoweredValue::buffer_len(value)
}

fn handle_id_lowered(value: String) -> LoweredValue {
    LoweredValue::handle_id(value)
}

fn js_value_bits_lowered(value: String) -> LoweredValue {
    LoweredValue::js_value_bits(value)
}

fn native_expr_kind(e: &Expr) -> &'static str {
    match e {
        Expr::Integer(_) => "Integer",
        Expr::LocalGet(_) => "LocalGet",
        Expr::MathImul(_, _) => "MathImul",
        Expr::Binary { .. } => "Binary",
        Expr::Call { .. } => "Call",
        Expr::Uint8ArrayGet { .. } => "Uint8ArrayGet",
        Expr::BufferIndexGet { .. } => "BufferIndexGet",
        _ => "Expr",
    }
}

fn lower_expr_native_i32(ctx: &mut FnCtx<'_>, e: &Expr) -> Result<LoweredValue> {
    let value = match e {
        Expr::Integer(n) => (*n as i32).to_string(),
        Expr::LocalGet(id) => {
            if let Some(slot) = ctx.i32_counter_slots.get(id).cloned() {
                ctx.block().load(I32, &slot)
            } else {
                let d = lower_expr(ctx, e)?;
                ctx.block().fptosi(DOUBLE, &d, I32)
            }
        }
        // Math.imul(a, b) → single `mul i32` instruction.
        Expr::MathImul(a, b) => {
            let l = lower_expr_native_i32(ctx, a)?.value;
            let r = lower_expr_native_i32(ctx, b)?.value;
            ctx.block().mul(I32, &l, &r)
        }
        Expr::Binary {
            op: BinaryOp::BitOr,
            left,
            right,
        } if matches!(right.as_ref(), Expr::Integer(0)) => lower_expr_native_i32(ctx, left)?.value,
        Expr::Binary { op, left, right }
            if matches!(
                op,
                BinaryOp::Add
                    | BinaryOp::Sub
                    | BinaryOp::Mul
                    | BinaryOp::BitAnd
                    | BinaryOp::BitOr
                    | BinaryOp::BitXor
                    | BinaryOp::Shl
                    | BinaryOp::Shr
                    | BinaryOp::UShr
            ) =>
        {
            let l = lower_expr_native_i32(ctx, left)?.value;
            let r = lower_expr_native_i32(ctx, right)?.value;
            let blk = ctx.block();
            match op {
                BinaryOp::Add => blk.add(I32, &l, &r),
                BinaryOp::Sub => blk.sub(I32, &l, &r),
                BinaryOp::Mul => blk.mul(I32, &l, &r),
                BinaryOp::BitAnd => blk.and(I32, &l, &r),
                BinaryOp::BitOr => blk.or(I32, &l, &r),
                BinaryOp::BitXor => blk.xor(I32, &l, &r),
                BinaryOp::Shl => blk.shl(I32, &l, &r),
                BinaryOp::Shr => blk.ashr(I32, &l, &r),
                BinaryOp::UShr => blk.lshr(I32, &l, &r),
                _ => unreachable!(),
            }
        }
        // Clamp-pattern calls: emit @llvm.smax.i32 / @llvm.smin.i32 directly
        // in i32, no double round-trip. Produces vectorizable IR.
        Expr::Call { callee, args, .. } => {
            let fid = if let Expr::FuncRef(id) = callee.as_ref() {
                *id
            } else {
                0
            };
            if ctx.clamp3_functions.contains(&fid) && args.len() == 3 {
                let v = lower_expr_native_i32(ctx, &args[0])?.value;
                let lo = lower_expr_native_i32(ctx, &args[1])?.value;
                let hi = lower_expr_native_i32(ctx, &args[2])?.value;
                let blk = ctx.block();
                let r1 = blk.fresh_reg();
                blk.emit_raw(format!(
                    "{} = call i32 @llvm.smax.i32(i32 {}, i32 {})",
                    r1, v, lo
                ));
                let r2 = blk.fresh_reg();
                blk.emit_raw(format!(
                    "{} = call i32 @llvm.smin.i32(i32 {}, i32 {})",
                    r2, r1, hi
                ));
                r2
            } else if ctx.clamp_u8_functions.contains(&fid) && args.len() == 1 {
                let v = lower_expr_native_i32(ctx, &args[0])?.value;
                let blk = ctx.block();
                let r1 = blk.fresh_reg();
                blk.emit_raw(format!(
                    "{} = call i32 @llvm.smax.i32(i32 {}, i32 0)",
                    r1, v
                ));
                let r2 = blk.fresh_reg();
                blk.emit_raw(format!(
                    "{} = call i32 @llvm.smin.i32(i32 {}, i32 255)",
                    r2, r1
                ));
                r2
            } else if ctx.i32_identity_functions.contains(&fid) && args.len() == 1 {
                lower_expr_native_i32(ctx, &args[0])?.value
            } else {
                // Non-clamp integer-returning helpers still route through the
                // typed lowering decision. The callee is marked alwaysinline
                // elsewhere, so optimized IR can still collapse this ABI bridge.
                let d = lower_expr(ctx, e)?;
                ctx.block().fptosi(DOUBLE, &d, I32)
            }
        }
        Expr::Uint8ArrayGet { array, index } => {
            super::arrays_finds::lower_uint8array_get_i32(ctx, array, index)?.value
        }
        Expr::BufferIndexGet { buffer, index } => {
            super::arrays_finds::lower_buffer_index_get_i32(ctx, buffer, index)?.value
        }
        // Fallback for other expressions.
        _ => {
            let d = lower_expr(ctx, e)?;
            ctx.block().fptosi(DOUBLE, &d, I32)
        }
    };
    let lowered = i32_lowered(value);
    ctx.record_lowered_value(
        native_expr_kind(e),
        None,
        "lower_expr_native_i32",
        &lowered,
        None,
        None,
        None,
        false,
        false,
        Vec::new(),
    );
    Ok(lowered)
}

fn lower_expr_native_js_value_bits(ctx: &mut FnCtx<'_>, e: &Expr) -> Result<LoweredValue> {
    let value = lower_expr(ctx, e)?;
    let bits = materialize_js_value_bits(
        ctx,
        LoweredValue::js_value(value),
        MaterializationReason::FunctionAbi,
    );
    let lowered = js_value_bits_lowered(bits);
    ctx.record_lowered_value(
        native_expr_kind(e),
        None,
        "lower_expr_native_js_value_bits",
        &lowered,
        None,
        None,
        None,
        false,
        false,
        Vec::new(),
    );
    Ok(lowered)
}

fn lower_expr_native_u32(ctx: &mut FnCtx<'_>, e: &Expr) -> Result<LoweredValue> {
    let value = match e {
        Expr::Integer(n) if *n >= 0 && u32::try_from(*n).is_ok() => (*n as u32).to_string(),
        Expr::LocalGet(id) => {
            if let Some(slot) = ctx.i32_counter_slots.get(id).cloned() {
                ctx.block().load(I32, &slot)
            } else {
                let d = lower_expr(ctx, e)?;
                ctx.block().toint32(&d)
            }
        }
        Expr::Binary {
            op: BinaryOp::UShr,
            left,
            right,
        } => {
            let l = lower_expr_native_u32(ctx, left)?.value;
            let r = lower_expr_native_u32(ctx, right)?.value;
            ctx.block().lshr(I32, &l, &r)
        }
        _ => {
            let d = lower_expr(ctx, e)?;
            ctx.block().toint32(&d)
        }
    };
    let lowered = u32_lowered(value);
    ctx.record_lowered_value(
        native_expr_kind(e),
        None,
        "lower_expr_native_u32",
        &lowered,
        None,
        None,
        None,
        false,
        false,
        Vec::new(),
    );
    Ok(lowered)
}

fn lower_expr_native_i64(ctx: &mut FnCtx<'_>, e: &Expr) -> Result<LoweredValue> {
    let value = match e {
        Expr::Integer(n) => n.to_string(),
        _ => {
            let d = lower_expr(ctx, e)?;
            ctx.block().fptosi(DOUBLE, &d, I64)
        }
    };
    let lowered = i64_lowered(value);
    ctx.record_lowered_value(
        native_expr_kind(e),
        None,
        "lower_expr_native_i64",
        &lowered,
        None,
        None,
        None,
        false,
        false,
        Vec::new(),
    );
    Ok(lowered)
}

fn lower_expr_native_u64(ctx: &mut FnCtx<'_>, e: &Expr) -> Result<LoweredValue> {
    let value = match e {
        Expr::Integer(n) if *n >= 0 => (*n as u64).to_string(),
        _ => {
            let d = lower_expr(ctx, e)?;
            ctx.block().fptoui(DOUBLE, &d, I64)
        }
    };
    let lowered = u64_lowered(value);
    ctx.record_lowered_value(
        native_expr_kind(e),
        None,
        "lower_expr_native_u64",
        &lowered,
        None,
        None,
        None,
        false,
        false,
        Vec::new(),
    );
    Ok(lowered)
}

fn lower_expr_native_usize(ctx: &mut FnCtx<'_>, e: &Expr) -> Result<LoweredValue> {
    let value = lower_expr_native_u64(ctx, e)?.value;
    let lowered = usize_lowered(value);
    ctx.record_lowered_value(
        native_expr_kind(e),
        None,
        "lower_expr_native_usize",
        &lowered,
        None,
        None,
        None,
        false,
        false,
        Vec::new(),
    );
    Ok(lowered)
}

fn lower_expr_native_f64(ctx: &mut FnCtx<'_>, e: &Expr) -> Result<LoweredValue> {
    let needs_raw_f64_fallback_coercion = expr_may_return_boxed_value_from_raw_f64_fallback(ctx, e)
        || matches!(e, Expr::IndexGet { .. }) && is_numeric_expr(ctx, e);
    let raw = lower_expr(ctx, e)?;
    let value = if needs_raw_f64_fallback_coercion {
        ctx.block()
            .call(DOUBLE, "js_number_coerce", &[(DOUBLE, &raw)])
    } else {
        raw
    };
    let lowered = f64_lowered(value);
    ctx.record_lowered_value(
        native_expr_kind(e),
        None,
        "lower_expr_native_f64",
        &lowered,
        None,
        None,
        None,
        false,
        false,
        Vec::new(),
    );
    Ok(lowered)
}

fn lower_expr_native_f32(ctx: &mut FnCtx<'_>, e: &Expr) -> Result<LoweredValue> {
    let needs_raw_f64_fallback_coercion = expr_may_return_boxed_value_from_raw_f64_fallback(ctx, e)
        || matches!(e, Expr::IndexGet { .. }) && is_numeric_expr(ctx, e);
    let raw = lower_expr(ctx, e)?;
    let d = if needs_raw_f64_fallback_coercion {
        ctx.block()
            .call(DOUBLE, "js_number_coerce", &[(DOUBLE, &raw)])
    } else {
        raw
    };
    let value = ctx.block().fptrunc(DOUBLE, &d, F32);
    let lowered = f32_lowered(value);
    ctx.record_lowered_value(
        native_expr_kind(e),
        None,
        "lower_expr_native_f32",
        &lowered,
        None,
        None,
        None,
        false,
        false,
        Vec::new(),
    );
    Ok(lowered)
}

fn lower_expr_native_buffer_len(ctx: &mut FnCtx<'_>, e: &Expr) -> Result<LoweredValue> {
    let value = lower_expr_native_u32(ctx, e)?.value;
    let lowered = buffer_len_lowered(value);
    ctx.record_lowered_value(
        native_expr_kind(e),
        None,
        "lower_expr_native_buffer_len",
        &lowered,
        None,
        None,
        None,
        false,
        false,
        Vec::new(),
    );
    Ok(lowered)
}

fn lower_expr_native_handle_id(ctx: &mut FnCtx<'_>, e: &Expr) -> Result<LoweredValue> {
    let value = lower_expr_native_u64(ctx, e)?.value;
    let lowered = handle_id_lowered(value);
    ctx.record_lowered_value(
        native_expr_kind(e),
        None,
        "lower_expr_native_handle_id",
        &lowered,
        None,
        None,
        None,
        false,
        false,
        Vec::new(),
    );
    Ok(lowered)
}

fn lower_expr_native_handle(ctx: &mut FnCtx<'_>, e: &Expr) -> Result<LoweredValue> {
    let value = lower_expr(ctx, e)?;
    let handle = unbox_to_i64(ctx.block(), &value);
    let lowered = LoweredValue::native_handle(handle);
    ctx.record_lowered_value(
        native_expr_kind(e),
        None,
        "lower_expr_native_native_handle",
        &lowered,
        None,
        None,
        None,
        false,
        false,
        Vec::new(),
    );
    Ok(lowered)
}

fn lower_expr_native_promise_boundary(ctx: &mut FnCtx<'_>, e: &Expr) -> Result<LoweredValue> {
    let value = lower_expr(ctx, e)?;
    let handle = unbox_to_i64(ctx.block(), &value);
    let lowered = LoweredValue::promise_boundary(handle);
    ctx.record_lowered_value(
        native_expr_kind(e),
        None,
        "lower_expr_native_promise_boundary",
        &lowered,
        None,
        None,
        None,
        false,
        false,
        Vec::new(),
    );
    Ok(lowered)
}
