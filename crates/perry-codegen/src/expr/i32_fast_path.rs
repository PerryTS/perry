//! i32-native expression fast path + flat-const 2D-table lowering
//! (extracted from `expr.rs`, issue #1098). Pure move — no logic changes.

use anyhow::{bail, Result};
use perry_hir::{BinaryOp, Expr};

use super::{
    array_kind_fact, lower_expr, raw_f64_layout_fact, unbox_str_handle, unbox_to_i64,
    FlatConstInfo, FnCtx, PackedNumericLoopKind,
};
use crate::native_value::{
    materialize_js_value_bits, BoundsState, BufferAccessMode, ExpectedNativeRep, LoweredValue,
    MaterializationReason, NativeRep,
};
use crate::type_analysis::{
    expr_may_return_boxed_value_from_raw_f64_fallback, is_definitely_string_expr, is_numeric_expr,
};
use crate::types::{DOUBLE, F32, I32, I64};

/// Returns true if `e` provably produces a finite double whose magnitude is
/// small enough (`|v| < 2^63`) for the unguarded `toint32_fast` lowering.
/// Used to skip the NaN/Inf/range guard in `toint32` for integer-arithmetic
/// hot paths — saving 5 instructions per bitwise op.
pub(crate) fn is_known_finite(ctx: &FnCtx<'_>, e: &Expr) -> bool {
    known_finite_magnitude_bits(ctx, e).is_some_and(|bits| bits <= 62)
}

/// Conservative magnitude bound for `e`'s numeric value: `Some(b)` proves the
/// value is finite AND `|v| < 2^b`. `toint32_fast` is a bare
/// `fptosi f64 → i64` + `trunc` — exactly JS ToInt32 for every `|v| < 2^63`,
/// but LLVM *poison* at or beyond it. Finiteness alone is NOT enough:
/// `(1e20) | 0` and nested integer multiplies (`(a*a)*a | 0` with i32-range
/// `a`) are finite yet exceed 2^63, and pre-fix produced NaN instead of the
/// ToInt32-wrapped value (CodeRabbit review on #5466; the same hole shipped
/// on main). Composition keeps the proof airtight where the old boolean
/// recursion silently escalated: Add/Sub grow the bound by one bit, Mul sums
/// the operand bounds, and anything unprovable returns `None` so callers fall
/// back to the guarded `toint32` runtime helper.
fn known_finite_magnitude_bits(ctx: &FnCtx<'_>, e: &Expr) -> Option<u32> {
    match e {
        Expr::Integer(n) => Some(64 - n.unsigned_abs().leading_zeros()),
        // Pod layout sizes/alignments/offsets are u32-class quantities.
        Expr::PodLayoutSizeOf { .. }
        | Expr::PodLayoutAlignOf { .. }
        | Expr::PodLayoutOffsetOf { .. } => Some(32),
        // Number literals can be NaN or ±Infinity (e.g., `Number(NaN)`,
        // `Number(f64::INFINITY)`). Inspect the value: `fptosi NaN` is
        // poison in LLVM and produced subnormal-double output (which
        // downstream code interpreted as a NaN-boxed string with
        // STRING_TAG bits, leading to garbled `console.log` output).
        Expr::Number(n) => {
            if !n.is_finite() {
                return None;
            }
            let magnitude = n.abs();
            if magnitude < 1.0 {
                Some(0)
            } else {
                Some(magnitude.log2() as u32 + 1)
            }
        }
        Expr::LocalGet(id) | Expr::Update { id, .. } => (ctx.integer_locals.contains(id)
            || ctx.unsigned_i32_locals.contains(id))
        .then_some(32),
        Expr::Uint8ArrayGet { .. } | Expr::BufferIndexGet { .. } => Some(8),
        // In-bounds loads from an int-element typed array are integers in
        // i32 range by construction (see `ta_int_elem_load_is_i32_provable`),
        // as are i32-tier masked-window plain-array loads (the dense-i32
        // range guard proved every window value is an i32 integer).
        Expr::IndexGet { object, index }
            if ta_int_elem_load_is_i32_provable(ctx, object, index)
                || super::masked_window::masked_window_i32_load_is_provable(ctx, object, index) =>
        {
            Some(32)
        }
        Expr::MathImul(_, _) => Some(32), // Math.imul returns i32 → always finite
        Expr::Call { callee, .. } => {
            matches!(callee.as_ref(), Expr::FuncRef(fid) if ctx.integer_returning_functions.contains(fid))
                .then_some(32)
        }
        Expr::Binary { op, left, right } => match op {
            BinaryOp::Add | BinaryOp::Sub => {
                let l = known_finite_magnitude_bits(ctx, left)?;
                let r = known_finite_magnitude_bits(ctx, right)?;
                Some(l.max(r) + 1)
            }
            BinaryOp::Mul => {
                let l = known_finite_magnitude_bits(ctx, left)?;
                let r = known_finite_magnitude_bits(ctx, right)?;
                Some(l + r)
            }
            // Bitwise results are already ToInt32/ToUint32-wrapped.
            BinaryOp::BitAnd
            | BinaryOp::BitOr
            | BinaryOp::BitXor
            | BinaryOp::Shl
            | BinaryOp::Shr
            | BinaryOp::UShr => Some(32),
            _ => None,
        },
        _ => None,
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

/// `object[index]` on a width-tracked typed-array local whose element kind is
/// integral and value-representable in a signed i32 (I8/U8/U8Clamped/I16/U16/
/// I32 — NOT U32, whose upper half doesn't round-trip through an i32 slot, and
/// not the float kinds), with the index bounds proven against the tracked view
/// length. In-bounds loads of these kinds are integers by construction, so the
/// access is an i32-native leaf — this is what keeps bcrypt-style S-box chains
/// (`(s + S[x & 1023]) | 0`) in `add i32` instead of a per-element
/// f64 round-trip through the branchless ToInt32 tower. Out-of-bounds reads
/// (which produce `undefined`) are excluded by the same bounds proof the
/// unchecked native load itself requires.
fn ta_int_elem_load_is_i32_provable(ctx: &FnCtx<'_>, object: &Expr, index: &Expr) -> bool {
    use crate::native_value::{BufferElem, BufferIndexUnit};
    if ctx.disable_buffer_fast_path {
        return false;
    }
    let Expr::LocalGet(id) = object else {
        return false;
    };
    let Some(view) = ctx.buffer_view_slots.get(id) else {
        return false;
    };
    if view.index_unit != BufferIndexUnit::Element
        || !view.alias.allows_noalias()
        || view.scope_idx.is_none()
    {
        return false;
    }
    if !matches!(
        view.elem,
        BufferElem::I8
            | BufferElem::U8
            | BufferElem::U8Clamped
            | BufferElem::I16
            | BufferElem::U16
            | BufferElem::I32
    ) {
        return false;
    }
    if ctx.closure_captures.contains_key(id)
        || matches!(
            ctx.buffer_hazard_reasons.get(id),
            Some(MaterializationReason::ClosureCapture)
        )
    {
        return false;
    }
    super::bounds_for_buffer_access_width(ctx, *id, index, 1).allows_inbounds()
}

fn packed_i32_loop_index_get_fact(ctx: &FnCtx<'_>, e: &Expr) -> Option<super::PackedF64LoopFact> {
    let Expr::IndexGet { object, index } = e else {
        return None;
    };
    let (Expr::LocalGet(arr_id), Expr::LocalGet(idx_id)) = (object.as_ref(), index.as_ref()) else {
        return None;
    };
    ctx.packed_f64_loop_facts
        .iter()
        .find(|fact| {
            fact.array_local_id == *arr_id
                && fact.index_local_id == *idx_id
                && fact.array_kind == PackedNumericLoopKind::I32
        })
        .cloned()
}

fn packed_u32_loop_index_get_fact(ctx: &FnCtx<'_>, e: &Expr) -> Option<super::PackedF64LoopFact> {
    let Expr::IndexGet { object, index } = e else {
        return None;
    };
    let (Expr::LocalGet(arr_id), Expr::LocalGet(idx_id)) = (object.as_ref(), index.as_ref()) else {
        return None;
    };
    ctx.packed_f64_loop_facts
        .iter()
        .find(|fact| {
            fact.array_local_id == *arr_id
                && fact.index_local_id == *idx_id
                && fact.array_kind == PackedNumericLoopKind::U32
        })
        .cloned()
}

pub(crate) fn can_lower_expr_as_i32_in_current_region(ctx: &FnCtx<'_>, e: &Expr) -> bool {
    if matches!(e, Expr::IterResultGetValue) {
        return true;
    }
    if can_lower_expr_as_i32(
        e,
        &ctx.i32_counter_slots,
        ctx.flat_const_arrays,
        &ctx.array_row_aliases,
        ctx.native_facts.integer_locals(),
        ctx.clamp3_functions,
        ctx.clamp_u8_functions,
        ctx.integer_returning_functions,
        ctx.i32_identity_functions,
    ) {
        return true;
    }
    if packed_i32_loop_index_get_fact(ctx, e).is_some() {
        return true;
    }
    match e {
        Expr::MathImul(left, right) => {
            can_lower_expr_as_i32_in_current_region(ctx, left)
                && can_lower_expr_as_i32_in_current_region(ctx, right)
        }
        Expr::Binary {
            op: BinaryOp::BitOr,
            left,
            right,
        } if matches!(right.as_ref(), Expr::Integer(0)) => {
            can_lower_expr_as_i32_in_current_region(ctx, left)
        }
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
            can_lower_expr_as_i32_in_current_region(ctx, left)
                && can_lower_expr_as_i32_in_current_region(ctx, right)
        }
        Expr::Call { callee, args, .. } => {
            let Expr::FuncRef(fid) = callee.as_ref() else {
                return false;
            };
            ((ctx.clamp3_functions.contains(fid) && args.len() == 3)
                || (ctx.clamp_u8_functions.contains(fid) && args.len() == 1)
                || ctx.i32_identity_functions.contains(fid))
                && args
                    .iter()
                    .all(|arg| can_lower_expr_as_i32_in_current_region(ctx, arg))
        }
        Expr::IndexGet { object, index } => {
            ta_int_elem_load_is_i32_provable(ctx, object, index)
                || super::masked_window::masked_window_i32_load_is_provable(ctx, object, index)
        }
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
        ExpectedNativeRep::I1 => lower_expr_native_i1(ctx, e),
        ExpectedNativeRep::F64 => lower_expr_native_f64(ctx, e),
        ExpectedNativeRep::F32 => lower_expr_native_f32(ctx, e),
        ExpectedNativeRep::StringRef => lower_expr_native_string_ref(ctx, e),
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

fn i1_lowered(value: String) -> LoweredValue {
    LoweredValue::i1(value)
}

fn f64_lowered(value: String) -> LoweredValue {
    LoweredValue::f64(value)
}

fn f32_lowered(value: String) -> LoweredValue {
    LoweredValue::f32(value)
}

fn string_ref_lowered(value: String) -> LoweredValue {
    LoweredValue::string_ref(value)
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
        Expr::Bool(_) => "Bool",
        Expr::LocalGet(_) => "LocalGet",
        Expr::Compare { .. } => "Compare",
        Expr::Unary { .. } => "Unary",
        Expr::BooleanCoerce(_) => "BooleanCoerce",
        Expr::MathImul(_, _) => "MathImul",
        Expr::Binary { .. } => "Binary",
        Expr::Call { .. } => "Call",
        Expr::Uint8ArrayGet { .. } => "Uint8ArrayGet",
        Expr::BufferIndexGet { .. } => "BufferIndexGet",
        Expr::IndexGet { .. } => "IndexGet",
        _ => "Expr",
    }
}

fn lower_expr_native_string_ref(ctx: &mut FnCtx<'_>, e: &Expr) -> Result<LoweredValue> {
    if !is_definitely_string_expr(ctx, e) {
        bail!("cannot lower expression as native StringRef without a string proof");
    }
    let boxed = lower_expr(ctx, e)?;
    let raw = unbox_str_handle(ctx.block(), &boxed);
    Ok(string_ref_lowered(raw))
}

fn try_lower_expr_native_i32_structural(ctx: &mut FnCtx<'_>, e: &Expr) -> Result<Option<String>> {
    let value = match e {
        Expr::Integer(n) => Some((*n as i32).to_string()),
        Expr::LocalGet(id) => ctx
            .i32_counter_slots
            .get(id)
            .cloned()
            .map(|slot| ctx.block().load(I32, &slot)),
        Expr::MathImul(a, b) => {
            let l = lower_expr_native_i32(ctx, a)?.value;
            let r = lower_expr_native_i32(ctx, b)?.value;
            Some(ctx.block().mul(I32, &l, &r))
        }
        Expr::Binary {
            op: BinaryOp::BitOr,
            left,
            right,
        } if matches!(right.as_ref(), Expr::Integer(0)) => {
            Some(lower_expr_native_i32(ctx, left)?.value)
        }
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
            Some(match op {
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
            })
        }
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
                Some(r2)
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
                Some(r2)
            } else if ctx.i32_identity_functions.contains(&fid) && args.len() == 1 {
                Some(lower_expr_native_i32(ctx, &args[0])?.value)
            } else {
                None
            }
        }
        Expr::Uint8ArrayGet { array, index } => {
            let lowered = super::arrays_finds::lower_uint8array_get_i32(ctx, array, index)?;
            Some(i32_from_indexed_get_lowered(ctx, lowered))
        }
        Expr::BufferIndexGet { buffer, index } => {
            let lowered = super::arrays_finds::lower_buffer_index_get_i32(ctx, buffer, index)?;
            Some(i32_from_indexed_get_lowered(ctx, lowered))
        }
        Expr::IndexGet { object, index } => {
            if ta_int_elem_load_is_i32_provable(ctx, object, index) {
                super::lower_typed_array_load(ctx, object, index)?
                    .map(|lowered| i32_from_indexed_get_lowered(ctx, lowered))
            } else {
                super::masked_window::lower_masked_window_index_get_i32(ctx, object, index)?
            }
        }
        _ => None,
    };
    Ok(value)
}

/// Bridge an indexed-get helper's `LoweredValue` into a guaranteed-i32 SSA
/// value. `lower_uint8array_get_i32`'s unproven-key escape (the mysql2
/// MockBuffer probe fix in `arrays_finds.rs`) returns the polymorphic
/// property read as a boxed JS VALUE (`F64` rep). The i32-context callers
/// here used to grab `.value` blindly and label that double register `i32`,
/// emitting malformed IR — `error: '%rN' defined with type 'double' but
/// expected 'i32'` — which the pi bundle hit (#6593) once its inliner
/// frontier left a `buf[k]` index insufficiently proven-numeric. Apply the
/// JS `ToInt32(ToNumber(v))` bridge instead.
fn i32_from_indexed_get_lowered(ctx: &mut FnCtx<'_>, lowered: LoweredValue) -> String {
    match lowered.rep {
        NativeRep::I32 | NativeRep::U32 => lowered.value,
        _ => {
            let number = ctx
                .block()
                .call(DOUBLE, "js_number_coerce", &[(DOUBLE, &lowered.value)]);
            ctx.block().toint32(&number)
        }
    }
}

fn lower_packed_i32_loop_index_get(ctx: &mut FnCtx<'_>, e: &Expr) -> Result<Option<LoweredValue>> {
    let Expr::IndexGet { object, index } = e else {
        return Ok(None);
    };
    let (Expr::LocalGet(arr_id), Expr::LocalGet(idx_id)) = (object.as_ref(), index.as_ref()) else {
        return Ok(None);
    };
    let Some(fact) = packed_i32_loop_index_get_fact(ctx, e) else {
        return Ok(None);
    };
    let Some(i32_slot) = ctx.i32_counter_slots.get(idx_id).cloned() else {
        return Ok(None);
    };

    let arr_box = lower_expr(ctx, object)?;
    let idx_i32 = ctx.block().load(I32, &i32_slot);
    let raw_f64 = {
        let blk = ctx.block();
        let arr_bits = blk.bitcast_double_to_i64(&arr_box);
        let arr_handle = blk.and(I64, &arr_bits, crate::nanbox::POINTER_MASK_I64);
        let idx_i64 = blk.zext(I32, &idx_i32, I64);
        let byte_offset = blk.shl(I64, &idx_i64, "3");
        let with_header = blk.add(I64, &byte_offset, "8");
        let element_addr = blk.add(I64, &arr_handle, &with_header);
        let element_ptr = blk.inttoptr(I64, &element_addr);
        blk.load(DOUBLE, &element_ptr)
    };
    let value = ctx.block().fptosi(DOUBLE, &raw_f64, I32);
    let lowered = LoweredValue::i32(value);
    let guard_id = fact.guard_id.clone();
    ctx.record_lowered_value_with_access_mode_and_facts(
        "PackedI32LoopLoad",
        Some(*arr_id),
        "packed_i32_loop_load",
        &lowered,
        Some(BoundsState::Guarded {
            guard_id: guard_id.clone(),
        }),
        None,
        Some(BufferAccessMode::CheckedNative),
        None,
        None,
        None,
        vec![
            array_kind_fact(Some(*arr_id), "consumed", "packed_i32", None),
            raw_f64_layout_fact(Some(*arr_id), "consumed", &guard_id, None),
        ],
        Vec::new(),
        false,
        false,
        vec![
            "index_range=nonnegative_i32".to_string(),
            "length_range=guarded_i32".to_string(),
            "storage_layout=raw_f64_numeric_slots".to_string(),
            "integer_materialization=fptosi_guarded_packed_i32".to_string(),
        ],
    );
    Ok(Some(lowered))
}

pub(crate) fn lower_packed_u32_loop_index_get(
    ctx: &mut FnCtx<'_>,
    e: &Expr,
) -> Result<Option<LoweredValue>> {
    let Expr::IndexGet { object, index } = e else {
        return Ok(None);
    };
    let (Expr::LocalGet(arr_id), Expr::LocalGet(idx_id)) = (object.as_ref(), index.as_ref()) else {
        return Ok(None);
    };
    let Some(fact) = packed_u32_loop_index_get_fact(ctx, e) else {
        return Ok(None);
    };
    let Some(i32_slot) = ctx.i32_counter_slots.get(idx_id).cloned() else {
        return Ok(None);
    };

    let arr_box = lower_expr(ctx, object)?;
    let idx_i32 = ctx.block().load(I32, &i32_slot);
    let raw_f64 = {
        let blk = ctx.block();
        let arr_bits = blk.bitcast_double_to_i64(&arr_box);
        let arr_handle = blk.and(I64, &arr_bits, crate::nanbox::POINTER_MASK_I64);
        let idx_i64 = blk.zext(I32, &idx_i32, I64);
        let byte_offset = blk.shl(I64, &idx_i64, "3");
        let with_header = blk.add(I64, &byte_offset, "8");
        let element_addr = blk.add(I64, &arr_handle, &with_header);
        let element_ptr = blk.inttoptr(I64, &element_addr);
        blk.load(DOUBLE, &element_ptr)
    };
    let value = ctx.block().fptoui(DOUBLE, &raw_f64, I32);
    let lowered = LoweredValue::u32(value);
    let guard_id = fact.guard_id.clone();
    ctx.record_lowered_value_with_access_mode_and_facts(
        "PackedU32LoopLoad",
        Some(*arr_id),
        "packed_u32_loop_load",
        &lowered,
        Some(BoundsState::Guarded {
            guard_id: guard_id.clone(),
        }),
        None,
        Some(BufferAccessMode::CheckedNative),
        None,
        None,
        None,
        vec![
            array_kind_fact(Some(*arr_id), "consumed", "packed_u32", None),
            raw_f64_layout_fact(Some(*arr_id), "consumed", &guard_id, None),
        ],
        Vec::new(),
        false,
        false,
        vec![
            "index_range=nonnegative_i32".to_string(),
            "length_range=guarded_i32".to_string(),
            "storage_layout=raw_f64_numeric_slots".to_string(),
            "integer_materialization=fptoui_guarded_packed_u32".to_string(),
        ],
    );
    Ok(Some(lowered))
}

fn lower_expr_native_i1(ctx: &mut FnCtx<'_>, e: &Expr) -> Result<LoweredValue> {
    if matches!(e, Expr::IterResultGetValue) {
        let value_i32 = ctx.block().call(I32, "js_iter_result_get_value_i1", &[]);
        let value = ctx.block().icmp_ne(I32, &value_i32, "0");
        let lowered = i1_lowered(value);
        ctx.record_lowered_value(
            native_expr_kind(e),
            None,
            "compiler_private_async_iter_result_get_i1",
            &lowered,
            None,
            None,
            None,
            false,
            false,
            vec!["slot_kind=raw_i1_or_truthy_jsvalue".to_string()],
        );
        return Ok(lowered);
    }
    if let Some(lowered) = crate::expr::lower_expr_value(ctx, e)? {
        if matches!(lowered.rep, NativeRep::I1) {
            ctx.record_lowered_value(
                native_expr_kind(e),
                None,
                "lower_expr_native_i1.proven",
                &lowered,
                None,
                None,
                None,
                false,
                false,
                Vec::new(),
            );
            return Ok(lowered);
        }
    }
    let boxed = lower_expr(ctx, e)?;
    let value = crate::lower_conditional::lower_truthy(ctx, &boxed, e);
    let lowered = i1_lowered(value);
    ctx.record_lowered_value(
        native_expr_kind(e),
        None,
        "lower_expr_native_i1.truthy_fallback",
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

fn lower_expr_native_i32(ctx: &mut FnCtx<'_>, e: &Expr) -> Result<LoweredValue> {
    if matches!(e, Expr::IterResultGetValue) {
        let value = ctx.block().call(I32, "js_iter_result_get_value_i32", &[]);
        let lowered = i32_lowered(value);
        ctx.record_lowered_value(
            native_expr_kind(e),
            None,
            "compiler_private_async_iter_result_get_i32",
            &lowered,
            None,
            None,
            None,
            false,
            false,
            vec!["slot_kind=raw_i32_or_toint32_jsvalue".to_string()],
        );
        return Ok(lowered);
    }
    if let Some(lowered) = lower_packed_i32_loop_index_get(ctx, e)? {
        return Ok(lowered);
    }
    if can_lower_expr_as_i32_in_current_region(ctx, e) {
        if let Some(value) = try_lower_expr_native_i32_structural(ctx, e)? {
            let lowered = i32_lowered(value);
            ctx.record_lowered_value(
                native_expr_kind(e),
                None,
                "lower_expr_native_i32.structural",
                &lowered,
                None,
                None,
                None,
                false,
                false,
                Vec::new(),
            );
            return Ok(lowered);
        }
    }
    if let Some(lowered) = crate::expr::lower_expr_value(ctx, e)? {
        let value = match lowered.rep {
            NativeRep::I32 | NativeRep::U32 | NativeRep::BufferLen => Some(lowered.value),
            NativeRep::U8 | NativeRep::I1 => {
                Some(ctx.block().zext(lowered.llvm_ty, &lowered.value, I32))
            }
            NativeRep::F64 => {
                // Index/internal i32 materialization — packed-store RHS and
                // numeric-index consumers prove their ranges upstream, so
                // keep the lean guard here (see toint32 vs toint32_wrap).
                if is_known_finite(ctx, e) {
                    Some(ctx.block().toint32_fast(&lowered.value))
                } else {
                    Some(ctx.block().toint32(&lowered.value))
                }
            }
            NativeRep::F32 => {
                let widened = ctx.block().fpext(F32, &lowered.value, DOUBLE);
                Some(ctx.block().toint32(&widened))
            }
            _ => None,
        };
        if let Some(value) = value {
            let lowered = i32_lowered(value);
            ctx.record_lowered_value(
                native_expr_kind(e),
                None,
                "lower_expr_native_i32.from_lowered_value",
                &lowered,
                None,
                None,
                None,
                false,
                false,
                Vec::new(),
            );
            return Ok(lowered);
        }
    }
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
            let lowered = super::arrays_finds::lower_uint8array_get_i32(ctx, array, index)?;
            i32_from_indexed_get_lowered(ctx, lowered)
        }
        Expr::BufferIndexGet { buffer, index } => {
            let lowered = super::arrays_finds::lower_buffer_index_get_i32(ctx, buffer, index)?;
            i32_from_indexed_get_lowered(ctx, lowered)
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
    let boxed_local_id = match e {
        Expr::LocalGet(id)
            if ctx.boxed_vars.contains(id)
                && !ctx.closure_captures.contains_key(id)
                && !ctx.module_globals.contains_key(id) =>
        {
            Some(*id)
        }
        _ => None,
    };
    let bits = if let Some(id) = boxed_local_id {
        if let Some(slot) = ctx.locals.get(&id).cloned() {
            let box_ptr = ctx.block().load(I64, &slot);
            ctx.block().call(I64, "js_box_get_bits", &[(I64, &box_ptr)])
        } else {
            let value = lower_expr(ctx, e)?;
            materialize_js_value_bits(
                ctx,
                LoweredValue::js_value(value),
                MaterializationReason::FunctionAbi,
            )
        }
    } else if let Some(lowered) = crate::expr::lower_expr_value(ctx, e)? {
        materialize_js_value_bits(ctx, lowered, MaterializationReason::FunctionAbi)
    } else {
        let value = lower_expr(ctx, e)?;
        materialize_js_value_bits(
            ctx,
            LoweredValue::js_value(value),
            MaterializationReason::FunctionAbi,
        )
    };
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
    if let Some(lowered) = lower_packed_u32_loop_index_get(ctx, e)? {
        return Ok(lowered);
    }
    if let Some(lowered) = crate::expr::lower_expr_value(ctx, e)? {
        let value = match lowered.rep {
            NativeRep::I32 | NativeRep::U32 | NativeRep::BufferLen => Some(lowered.value),
            NativeRep::U8 | NativeRep::I1 => {
                Some(ctx.block().zext(lowered.llvm_ty, &lowered.value, I32))
            }
            NativeRep::F64 => Some(ctx.block().toint32(&lowered.value)),
            NativeRep::F32 => {
                let widened = ctx.block().fpext(F32, &lowered.value, DOUBLE);
                Some(ctx.block().toint32(&widened))
            }
            _ => None,
        };
        if let Some(value) = value {
            let lowered = u32_lowered(value);
            ctx.record_lowered_value(
                native_expr_kind(e),
                None,
                "lower_expr_native_u32.from_lowered_value",
                &lowered,
                None,
                None,
                None,
                false,
                false,
                Vec::new(),
            );
            return Ok(lowered);
        }
    }
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
    if matches!(e, Expr::IterResultGetValue) {
        let value = ctx
            .block()
            .call(DOUBLE, "js_iter_result_get_value_f64", &[]);
        let lowered = f64_lowered(value);
        ctx.record_lowered_value(
            native_expr_kind(e),
            None,
            "compiler_private_async_iter_result_get_f64",
            &lowered,
            None,
            None,
            None,
            false,
            false,
            vec!["slot_kind=raw_f64_or_coerced_jsvalue".to_string()],
        );
        return Ok(lowered);
    }
    if let Some(value) =
        crate::expr::property_get::lower_raw_f64_class_field_get_for_number_context(ctx, e)?
    {
        let lowered = f64_lowered(value);
        ctx.record_lowered_value(
            native_expr_kind(e),
            None,
            "lower_expr_native_f64.class_field_number_context",
            &lowered,
            None,
            None,
            None,
            false,
            false,
            Vec::new(),
        );
        return Ok(lowered);
    }
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
