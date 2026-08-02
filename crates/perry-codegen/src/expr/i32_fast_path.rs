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
use crate::types::{DOUBLE, F32, I1, I16, I32, I64, I8};

#[cfg(test)]
mod bits_tests;

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
        &ctx.const_number_locals,
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
        &ctx.const_number_locals,
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
/// An integer literal is usable as an i32 leaf of an i32-native chain when its
/// value fits in 32 bits under EITHER a signed or an unsigned interpretation.
/// The i32 lowering truncates to the low 32 bits (`*n as i32`), and every
/// combining operator in an i32 chain — add/sub/mul, bitwise, shift, and
/// `Math.imul` — preserves low-32-bit two's-complement semantics, so a `>i32::MAX`
/// bit-mask/hash multiplier such as `0x9e3779b1` (2654435761) lowers to the
/// correct `mul i32` operand instead of falling off the fast path. Values that
/// exceed 32 bits (e.g. `2**32+3`) stay off the fast path so the runtime helper
/// applies JS `ToUint32`/`ToInt32` first.
fn integer_is_i32_bit_representable(n: i64) -> bool {
    i32::try_from(n).is_ok() || u32::try_from(n).is_ok()
}

/// A `Math.imul` operand is i32-lowerable in the current region when it is any
/// ordinary i32-native expression OR a 32-bit-representable integer literal.
/// The literal relaxation is confined to `Math.imul` — whose result is defined
/// as `ToInt32(ToUint32 * ToUint32 mod 2^32)` — because only there is the low-32
/// truncation of a `>i32::MAX` literal exact (unlike a plain `*`, whose product
/// is evaluated in f64 and loses precision above 2^53).
pub(crate) fn imul_operand_i32_lowerable_in_current_region(ctx: &FnCtx<'_>, e: &Expr) -> bool {
    matches!(e, Expr::Integer(n) if integer_is_i32_bit_representable(*n))
        || can_lower_expr_as_i32_in_current_region(ctx, e)
}

/// Lower a `Math.imul` operand to an i32 SSA value. A 32-bit-representable
/// integer literal is truncated directly (`*n as i32` == its ToInt32, since a
/// JS numeric literal that fits in 32 bits is exact in f64) so the multiply
/// stays a clean `mul i32` with a constant operand instead of a fold-only
/// `fptosi`; every other operand routes through the normal i32-native lowering.
pub(crate) fn lower_imul_operand_i32(ctx: &mut FnCtx<'_>, e: &Expr) -> Result<String> {
    if let Expr::Integer(n) = e {
        if integer_is_i32_bit_representable(*n) {
            return Ok((*n as i32).to_string());
        }
    }
    Ok(lower_expr_native_i32(ctx, e)?.value)
}

/// Every integer with `|v| <= 2^53` is exactly representable as an IEEE-754
/// double; past that the ulp exceeds 1 and a JS `*`/`+` result is a *rounded*
/// integer. This is the constant that makes an i32-native chain agree with JS —
/// see [`i32_chain_magnitude_bits`].
const F64_EXACT_INTEGER_BITS: u32 = 53;

/// Magnitude bound of a leaf that is only known to be i32/u32-shaped.
const I32_CHAIN_LEAF_BITS: u32 = 32;

/// `|n| < 2^bits`.
fn integer_magnitude_bits(n: i64) -> u32 {
    64 - n.unsigned_abs().leading_zeros()
}

/// Cap an intermediate at the double's exact-integer range. `None` once the
/// value could reach 2^53, which is where JS rounds and exact integer
/// arithmetic does not.
fn f64_exact_bits(bits: u32) -> Option<u32> {
    (bits <= F64_EXACT_INTEGER_BITS).then_some(bits)
}

/// The combining operators an i32-native chain admits.
fn is_i32_chain_op(op: BinaryOp) -> bool {
    matches!(
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
    )
}

/// Magnitude bound of `left <op> right` from the operands' bounds.
///
/// `Add`/`Sub` grow the bound by one bit and `Mul` sums them — the same
/// composition [`known_finite_magnitude_bits`] uses — but capped at 2^53
/// instead of 2^63, because this bound gates *exact integer arithmetic* rather
/// than a single `fptosi`.
///
/// The ToInt32/ToUint32-wrapped operators reset the bound to 32. Two of them
/// carry a tighter one, which is what keeps masked/shifted hash mixing on the
/// fast path once the cap exists: `x & m` with a non-negative literal mask
/// lands in `[0, m]`, and `x >> k` / `x >>> k` by a literal `k` in `1..32` drop
/// `k` bits off a 32-bit value. (Shift counts outside that range take the
/// untightened 32 — JS masks the count to 5 bits, which this does not model.)
fn combine_i32_chain_bits(op: BinaryOp, left: &Expr, right: &Expr, l: u32, r: u32) -> Option<u32> {
    match op {
        BinaryOp::Add | BinaryOp::Sub => f64_exact_bits(l.max(r) + 1),
        BinaryOp::Mul => f64_exact_bits(l + r),
        BinaryOp::BitAnd => {
            let mask_bits = |e: &Expr| match e {
                Expr::Integer(m) if *m >= 0 => Some(integer_magnitude_bits(*m)),
                _ => None,
            };
            Some(
                mask_bits(left)
                    .into_iter()
                    .chain(mask_bits(right))
                    .min()
                    .unwrap_or(I32_CHAIN_LEAF_BITS),
            )
        }
        BinaryOp::Shr | BinaryOp::UShr => Some(match right {
            Expr::Integer(k) if (1..32).contains(k) => I32_CHAIN_LEAF_BITS - *k as u32,
            _ => I32_CHAIN_LEAF_BITS,
        }),
        BinaryOp::BitOr | BinaryOp::BitXor | BinaryOp::Shl => Some(I32_CHAIN_LEAF_BITS),
        _ => None,
    }
}

/// Magnitude bound of a `const` local bound to a numeric literal — an exact
/// integer, or `None` to leave the caller on the untightened 32-bit default.
/// Never *widens* past 32: the i32 chain reads the local's i32 slot, so the
/// value it computes with is ToInt32-shaped whatever the literal was.
fn const_number_magnitude_bits(v: f64) -> Option<u32> {
    if !v.is_finite() || v.trunc() != v {
        return None;
    }
    let as_i64 = v as i64;
    (as_i64 as f64 == v).then(|| integer_magnitude_bits(as_i64).min(I32_CHAIN_LEAF_BITS))
}

/// Borrowed view of the fact tables the i32-chain rules consult, so the
/// recursion carries one argument instead of nine.
#[derive(Clone, Copy)]
struct I32ChainEnv<'a> {
    i32_slots: &'a std::collections::HashMap<u32, String>,
    flat_const_arrays: &'a std::collections::HashMap<u32, FlatConstInfo>,
    array_row_aliases: &'a std::collections::HashMap<u32, (u32, Box<Expr>)>,
    integer_locals: &'a std::collections::HashSet<u32>,
    const_number_locals: &'a std::collections::HashMap<u32, f64>,
    clamp3_fns: &'a std::collections::HashSet<u32>,
    clamp_u8_fns: &'a std::collections::HashSet<u32>,
    integer_returning_fns: &'a std::collections::HashSet<u32>,
    i32_identity_fns: &'a std::collections::HashSet<u32>,
}

/// (Issue #49) `Some(bits)` when `e` can be lowered as an i32-native
/// expression — every leaf sourced from an i32 slot, a typed-array byte load,
/// or an integer literal, combined by `Add/Sub/Mul` and the bitwise ops — where
/// `bits` additionally proves the node's exact integer value satisfies
/// `|v| < 2^bits`. `None` means "do not take the fast path".
///
/// ## The invariant (#7232)
///
/// An i32-native chain computes the **exact** two's-complement low 32 bits of
/// the integer result. JS evaluates the same chain in doubles, rounding at
/// *every* operator. The two agree only while each intermediate is exactly
/// representable as a double, i.e. `|v| <= 2^53`: below that ceiling the JS
/// double *is* the exact integer and `low32(exact) == ToInt32(double)`, above
/// it the double has already discarded low bits the exact chain still carries.
///
/// `(x * 1103515245 + 12345) & 0x7fffffff` — an LCG step — is the shape that
/// exposed this: the product is ~2^61, so Node's mask reads a rounded product
/// (654583808) while an exact `mul i32` reads the true low bits (654583775).
/// Capping the bound at 53 pushes such a chain onto the f64 path, whose
/// `fmul`/`fadd` round exactly where the spec says to.
///
/// The old rule only required every *literal* to fit in i32, which is neither
/// necessary (`Math.imul` is exempt) nor sufficient: `1103515245` fits, and its
/// product with an i32-range local does not.
fn i32_chain_magnitude_bits(e: &Expr, env: I32ChainEnv<'_>) -> Option<u32> {
    match e {
        // Strict i32 range for a general leaf: a `>i32::MAX` literal must NOT
        // enter an arbitrary i32 chain, because the i32 lowering truncates it
        // to the low 32 bits while JS `*` sees the full value. Only
        // `Math.imul` (below) and the runtime helper interpret an operand under
        // exact-low-32 semantics.
        Expr::Integer(n) => i32::try_from(*n).ok().map(|_| integer_magnitude_bits(*n)),
        Expr::LocalGet(id) => {
            if !(env.i32_slots.contains_key(id) || env.integer_locals.contains(id)) {
                return None;
            }
            // A `const` bound to a numeric literal has an exactly-known
            // magnitude, and that is what keeps the dominant strided-index
            // shape on the exact path once the 2^53 cap exists: in
            // `buf[y * WIDTH + x]` the product is measured against WIDTH's
            // actual width, not the 32-bit default a plain local gets.
            Some(
                env.const_number_locals
                    .get(id)
                    .copied()
                    .and_then(const_number_magnitude_bits)
                    .unwrap_or(I32_CHAIN_LEAF_BITS),
            )
        }
        Expr::Uint8ArrayGet { .. } | Expr::BufferIndexGet { .. } => Some(8),
        Expr::MathImul(a, b) => {
            // `Math.imul(x, y) == ToInt32(ToUint32(x) * ToUint32(y) mod 2^32)`
            // — defined as an exact low-32 multiply, so it is NOT subject to
            // the 2^53 rule and an integer literal operand is exact under
            // low-32 truncation even past `i32::MAX` (the `0x9e3779b1` mixer
            // constant). Its *result* is an i32.
            let operand_ok = |e: &Expr| {
                matches!(e, Expr::Integer(n) if integer_is_i32_bit_representable(*n))
                    || i32_chain_magnitude_bits(e, env).is_some()
            };
            (operand_ok(a) && operand_ok(b)).then_some(I32_CHAIN_LEAF_BITS)
        }
        Expr::Binary {
            op: BinaryOp::BitOr,
            left,
            right,
        } if matches!(right.as_ref(), Expr::Integer(0)) => {
            i32_chain_magnitude_bits(left, env).map(|l| l.min(I32_CHAIN_LEAF_BITS))
        }
        Expr::Binary { op, left, right } if is_i32_chain_op(*op) => {
            let l = i32_chain_magnitude_bits(left, env)?;
            let r = i32_chain_magnitude_bits(right, env)?;
            combine_i32_chain_bits(*op, left, right, l, r)
        }
        Expr::Call { callee, args, .. } => {
            let Expr::FuncRef(fid) = callee.as_ref() else {
                return None;
            };
            if !((env.clamp3_fns.contains(fid) && args.len() == 3)
                || (env.clamp_u8_fns.contains(fid) && args.len() == 1)
                || env.integer_returning_fns.contains(fid))
            {
                return None;
            }
            if env.integer_returning_fns.contains(fid)
                && !env.clamp3_fns.contains(fid)
                && !env.clamp_u8_fns.contains(fid)
                && !env.i32_identity_fns.contains(fid)
            {
                return None;
            }
            args.iter()
                .all(|a| i32_chain_magnitude_bits(a, env).is_some())
                .then_some(I32_CHAIN_LEAF_BITS)
        }
        // Issue #50 bridge: element of a flat-const 2D int table.
        Expr::IndexGet { object, .. } => match object.as_ref() {
            Expr::IndexGet { object: inner, .. } => {
                matches!(inner.as_ref(), Expr::LocalGet(id) if env.flat_const_arrays.contains_key(id))
            }
            Expr::LocalGet(id) => env
                .array_row_aliases
                .get(id)
                .is_some_and(|(cid, _)| env.flat_const_arrays.contains_key(cid)),
            _ => false,
        }
        .then_some(I32_CHAIN_LEAF_BITS),
        _ => None,
    }
}

/// (Issue #49) Return `true` if `e` can be lowered as an i32-native
/// expression. Used by the `LocalSet` fast path to decide whether the rhs can
/// bypass the fp round-trip.
///
/// The fallback `lower_expr_as_i32` path is `toint32(lower_expr())`, which is
/// always correct — it evaluates the chain in doubles exactly as the spec
/// requires — so returning `false` is always the safe direction. We only commit
/// to the fast path when every leaf is recognizably int-sourced AND the whole
/// chain is provably f64-exact ([`i32_chain_magnitude_bits`]).
#[allow(clippy::too_many_arguments)]
pub(crate) fn can_lower_expr_as_i32(
    e: &Expr,
    i32_slots: &std::collections::HashMap<u32, String>,
    flat_const_arrays: &std::collections::HashMap<u32, FlatConstInfo>,
    array_row_aliases: &std::collections::HashMap<u32, (u32, Box<Expr>)>,
    integer_locals: &std::collections::HashSet<u32>,
    const_number_locals: &std::collections::HashMap<u32, f64>,
    clamp3_fns: &std::collections::HashSet<u32>,
    clamp_u8_fns: &std::collections::HashSet<u32>,
    integer_returning_fns: &std::collections::HashSet<u32>,
    i32_identity_fns: &std::collections::HashSet<u32>,
) -> bool {
    i32_chain_magnitude_bits(
        e,
        I32ChainEnv {
            i32_slots,
            flat_const_arrays,
            array_row_aliases,
            integer_locals,
            const_number_locals,
            clamp3_fns,
            clamp_u8_fns,
            integer_returning_fns,
            i32_identity_fns,
        },
    )
    .is_some()
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

/// Element kind of a statically-typed **integer** typed-array receiver eligible
/// for the *checked* inline i32 element load. Returns
/// `(runtime_kind_tag, elem_llvm_ty, signed, elem_size_bytes)` for the integer
/// kinds whose element widens into a signed i32 (I8/U8/U8Clamped/I16/U16/I32);
/// `None` for U32 / the float kinds and for any non-typed-array / non-local
/// receiver.
///
/// Unlike [`ta_int_elem_load_is_i32_provable`], this requires NEITHER a tracked
/// buffer view NOR a static bounds proof — which is exactly what an
/// `Int32Array` **parameter** (`function f(S: Int32Array){ S[i] }`) lacks, since
/// its length and inline-vs-view storage are unknown at compile time. Soundness
/// comes from the *checked* emission ([`lower_checked_typed_array_i32_load`]): a
/// runtime guard (pointer + inline-storage `PERRY_TA_VIEW_GUARD==0` + kind-cache
/// match) and a header-length bounds check gate a bare load, an in-kind
/// out-of-bounds read yields `0` (`== ToInt32(undefined)`), and every rejected
/// shape (view/detached/resizable backing, wrong runtime kind) defers to the
/// full runtime `[[Get]]`+`ToInt32`. Returning `0` on OOB is exact *only* in the
/// i32/`ToInt32` consumer context this predicate participates in — the sole
/// observable value there — so it is confined to the i32-native fast path.
fn checked_typed_array_i32_kind(
    ctx: &FnCtx<'_>,
    object: &Expr,
) -> Option<(u8, crate::types::LlvmType, bool, u32)> {
    if ctx.disable_buffer_fast_path {
        return None;
    }
    // Must be a plain local/param read so the receiver value is re-fetched at
    // every access — reassignment / closure capture stay correct because the
    // emission caches nothing across accesses.
    let Expr::LocalGet(id) = object else {
        return None;
    };
    // A tracked buffer view (proven-bounds unchecked path, or a Buffer param)
    // owns this receiver; don't shadow it.
    if ctx.buffer_view_slots.contains_key(id) {
        return None;
    }
    match crate::type_analysis::receiver_class_name(ctx, object).as_deref()? {
        "Int8Array" => Some((0, I8, true, 1)),
        "Uint8Array" => Some((1, I8, false, 1)),
        "Uint8ClampedArray" => Some((8, I8, false, 1)),
        "Int16Array" => Some((2, I16, true, 2)),
        "Uint16Array" => Some((3, I16, false, 2)),
        "Int32Array" => Some((4, I32, false, 4)),
        _ => None,
    }
}

/// Emit a *checked* inline i32 typed-array element load for an integer-kind
/// receiver whose storage/length is not statically known (a typed-array
/// parameter). Mirrors the runtime `TypedArrayHeader` layout (length `u32` at
/// offset 0, inline data at offset 16) and the process-global fast-path facts
/// (`PERRY_TA_VIEW_GUARD`, `PERRY_TA_KIND_CACHE`). Hot path is a bare native
/// load; a genuine in-kind out-of-bounds read merges in `0`; every guard miss
/// defers to `js_typed_array_read_int32`. See [`checked_typed_array_i32_kind`]
/// for the soundness argument. Callers must have proven the receiver eligible
/// via that predicate.
fn lower_checked_typed_array_i32_load(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    index: &Expr,
    kind: u8,
    elem_ty: crate::types::LlvmType,
    signed: bool,
    elem_size: u32,
) -> Result<String> {
    let obj_box = lower_expr(ctx, object)?;
    let idx_i32 = lower_expr_as_i32(ctx, index)?;

    let chk_idx = ctx.new_block("cta.get.chk");
    let load_idx = ctx.new_block("cta.get.load");
    let oob_idx = ctx.new_block("cta.get.oob");
    let slow_idx = ctx.new_block("cta.get.slow");
    let merge_idx = ctx.new_block("cta.get.merge");
    let chk_label = ctx.block_label(chk_idx);
    let load_label = ctx.block_label(load_idx);
    let oob_label = ctx.block_label(oob_idx);
    let slow_label = ctx.block_label(slow_idx);
    let merge_label = ctx.block_label(merge_idx);

    let tag_mask = crate::nanbox::i64_literal(crate::nanbox::TAG_MASK);

    // ---- entry guard: pointer + inline-storage + kind-cache addr/kind ----
    let raw = {
        let blk = ctx.block();
        let obj_bits = blk.bitcast_double_to_i64(&obj_box);
        let raw = blk.and(I64, &obj_bits, crate::nanbox::POINTER_MASK_I64);
        let tagged = blk.and(I64, &obj_bits, &tag_mask);
        let is_ptr = blk.icmp_eq(I64, &tagged, crate::nanbox::POINTER_TAG_I64);
        // View guard 0 => every live typed array uses inline storage, so
        // `data == header + 16`. Any view/native-arena backing bumps it,
        // routing such receivers to the slow path.
        let vg = blk.load(I64, "@PERRY_TA_VIEW_GUARD");
        let vg_zero = blk.icmp_eq(I64, &vg, "0");
        // Kind-cache probe: slot = (raw >> 3) & 63; entry = (addr << 8) | kind.
        let slot = blk.lshr(I64, &raw, "3");
        let slot = blk.and(I64, &slot, "63");
        let entry_ptr = blk.gep(
            "[64 x i64]",
            "@PERRY_TA_KIND_CACHE",
            &[(I64, "0"), (I64, &slot)],
        );
        let entry_val = blk.load(I64, &entry_ptr);
        let entry_addr = blk.lshr(I64, &entry_val, "8");
        let addr_match = blk.icmp_eq(I64, &entry_addr, &raw); // also rejects empty slot 0
        let kind_bits = blk.and(I64, &entry_val, "255");
        let kind_ok = blk.icmp_eq(I64, &kind_bits, &kind.to_string());
        let g = blk.and(I1, &is_ptr, &vg_zero);
        let g = blk.and(I1, &g, &addr_match);
        let g = blk.and(I1, &g, &kind_ok);
        blk.cond_br(&g, &chk_label, &slow_label);
        raw
    };

    // ---- chk: bounds check against header length (u32 at offset 0) ----
    ctx.current_block = chk_idx;
    {
        let blk = ctx.block();
        let hdr_ptr = blk.inttoptr(I64, &raw);
        let len = blk.load(I32, &hdr_ptr);
        // `ult` also rejects a negative i32 index (wraps to a huge unsigned) —
        // matching JS: `S[-1]` is undefined -> ToInt32 -> 0 (via the oob arm).
        let in_bounds = blk.icmp_ult(I32, &idx_i32, &len);
        blk.cond_br(&in_bounds, &load_label, &oob_label);
    }

    // ---- load: bare per-kind element load (data base = raw + 16) ----
    ctx.current_block = load_idx;
    let (load_val, load_end) = {
        let blk = ctx.block();
        let data_base = blk.add(I64, &raw, "16");
        let idx_i64 = blk.zext(I32, &idx_i32, I64);
        let shift = elem_size.trailing_zeros().to_string();
        let off = blk.shl(I64, &idx_i64, &shift);
        let addr = blk.add(I64, &data_base, &off);
        let ptr = blk.inttoptr(I64, &addr);
        let raw_elem = blk.load(elem_ty, &ptr);
        let val = if elem_size == 4 {
            raw_elem // i32 element: already the target width
        } else if signed {
            blk.sext(elem_ty, &raw_elem, I32)
        } else {
            blk.zext(elem_ty, &raw_elem, I32)
        };
        let end = blk.label.clone();
        blk.br(&merge_label);
        (val, end)
    };

    // ---- oob: in-kind out-of-bounds -> 0 (== ToInt32(undefined)) ----
    ctx.current_block = oob_idx;
    let oob_end = {
        let blk = ctx.block();
        let end = blk.label.clone();
        blk.br(&merge_label);
        end
    };

    // ---- slow: view / detached / wrong-kind -> full runtime read+ToInt32 ----
    ctx.current_block = slow_idx;
    let (slow_val, slow_end) = {
        let blk = ctx.block();
        let v = blk.call(
            I32,
            "js_typed_array_read_int32",
            &[(I64, &raw), (I32, &idx_i32)],
        );
        let end = blk.label.clone();
        blk.br(&merge_label);
        (v, end)
    };

    // ---- merge ----
    ctx.current_block = merge_idx;
    Ok(ctx.block().phi(
        I32,
        &[
            (load_val.as_str(), load_end.as_str()),
            ("0", oob_end.as_str()),
            (slow_val.as_str(), slow_end.as_str()),
        ],
    ))
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

/// The fact tables [`i32_chain_magnitude_bits`] consults, taken from `ctx`.
fn ctx_i32_chain_env<'a>(ctx: &'a FnCtx<'_>) -> I32ChainEnv<'a> {
    I32ChainEnv {
        i32_slots: &ctx.i32_counter_slots,
        flat_const_arrays: ctx.flat_const_arrays,
        array_row_aliases: &ctx.array_row_aliases,
        integer_locals: ctx.native_facts.integer_locals(),
        const_number_locals: &ctx.const_number_locals,
        clamp3_fns: ctx.clamp3_functions,
        clamp_u8_fns: ctx.clamp_u8_functions,
        integer_returning_fns: ctx.integer_returning_functions,
        i32_identity_fns: ctx.i32_identity_functions,
    }
}

pub(crate) fn can_lower_expr_as_i32_in_current_region(ctx: &FnCtx<'_>, e: &Expr) -> bool {
    region_i32_chain_magnitude_bits(ctx, e).is_some()
}

/// Region-aware [`i32_chain_magnitude_bits`]: the ctx-free leaf set plus the
/// per-scope proofs (packed-loop element reads, bounds-proven typed-array
/// loads, masked-window loads), all of which are i32-valued leaves. The 2^53
/// exactness cap (#7232) is the same one, applied through the same combiner —
/// a region leaf is not a licence to evaluate past double precision.
fn region_i32_chain_magnitude_bits(ctx: &FnCtx<'_>, e: &Expr) -> Option<u32> {
    if matches!(e, Expr::IterResultGetValue) {
        return Some(I32_CHAIN_LEAF_BITS);
    }
    if let Some(bits) = i32_chain_magnitude_bits(e, ctx_i32_chain_env(ctx)) {
        return Some(bits);
    }
    if packed_i32_loop_index_get_fact(ctx, e).is_some() {
        return Some(I32_CHAIN_LEAF_BITS);
    }
    match e {
        Expr::MathImul(left, right) => (imul_operand_i32_lowerable_in_current_region(ctx, left)
            && imul_operand_i32_lowerable_in_current_region(ctx, right))
        .then_some(I32_CHAIN_LEAF_BITS),
        Expr::Binary {
            op: BinaryOp::BitOr,
            left,
            right,
        } if matches!(right.as_ref(), Expr::Integer(0)) => {
            region_i32_chain_magnitude_bits(ctx, left).map(|l| l.min(I32_CHAIN_LEAF_BITS))
        }
        Expr::Binary { op, left, right } if is_i32_chain_op(*op) => {
            let l = region_i32_chain_magnitude_bits(ctx, left)?;
            let r = region_i32_chain_magnitude_bits(ctx, right)?;
            combine_i32_chain_bits(*op, left, right, l, r)
        }
        Expr::Call { callee, args, .. } => {
            let Expr::FuncRef(fid) = callee.as_ref() else {
                return None;
            };
            (((ctx.clamp3_functions.contains(fid) && args.len() == 3)
                || (ctx.clamp_u8_functions.contains(fid) && args.len() == 1)
                || ctx.i32_identity_functions.contains(fid))
                && args
                    .iter()
                    .all(|arg| can_lower_expr_as_i32_in_current_region(ctx, arg)))
            .then_some(I32_CHAIN_LEAF_BITS)
        }
        Expr::IndexGet { object, index } => (ta_int_elem_load_is_i32_provable(ctx, object, index)
            || super::masked_window::masked_window_i32_load_is_provable(ctx, object, index)
            // The checked-kind fast path lowers `index` through `fptosi`
            // (ToInt32), so a fractional index like `S[3.9]` would read
            // element 3 — JS reads a fractional typed-array index as
            // `undefined` (→ 0 in this ToInt32 consumer). Only take it with a
            // proven integer index (the same gate the sibling typed-array
            // read paths use in `index_get.rs`).
            || (checked_typed_array_i32_kind(ctx, object).is_some()
                && super::index_get::numeric_index_has_integer_array_index_proof(ctx, index)))
        .then_some(I32_CHAIN_LEAF_BITS),
        _ => None,
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
            let l = lower_imul_operand_i32(ctx, a)?;
            let r = lower_imul_operand_i32(ctx, b)?;
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
            } else if let Some(v) =
                super::masked_window::lower_masked_window_index_get_i32(ctx, object, index)?
            {
                Some(v)
            } else if let Some((kind, elem_ty, signed, elem_size)) =
                checked_typed_array_i32_kind(ctx, object)
            {
                Some(lower_checked_typed_array_i32_load(
                    ctx, object, index, kind, elem_ty, signed, elem_size,
                )?)
            } else {
                None
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
            let l = lower_imul_operand_i32(ctx, a)?;
            let r = lower_imul_operand_i32(ctx, b)?;
            ctx.block().mul(I32, &l, &r)
        }
        Expr::Binary {
            op: BinaryOp::BitOr,
            left,
            right,
        } if matches!(right.as_ref(), Expr::Integer(0)) => lower_expr_native_i32(ctx, left)?.value,
        // Last-resort ARITHMETIC, reached when `lower_expr_value` could not
        // produce a value at all. It is a SECOND emitter of the same
        // `add/sub/mul i32` the structural path above emits, so it carries the
        // same #7232 exactness proof — otherwise a shape that reaches here
        // would evaluate past double precision behind the fixed gate. Without
        // the proof the chain is evaluated in doubles and ToInt32-wrapped,
        // which is what the spec asks for.
        //
        // Scoped to `Add`/`Sub`/`Mul`, the only operators whose exact integer
        // result can leave the double's exact range. The bitwise arm below is
        // ToInt32-wrapped by definition and needs no proof — and must NOT get
        // one: its operands here are untyped-but-ToInt32-consumed values
        // (bcryptjs's `S[l >>> 24]` reaches exactly this arm), and routing
        // those through `lower_expr` swaps a native `lshr` for a
        // `js_dynamic_ushr` call. Measured on
        // `benchmarks/suite/bench_typed_array_untyped_access.ts`.
        Expr::Binary { op, .. }
            if matches!(op, BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul)
                && region_i32_chain_magnitude_bits(ctx, e).is_none() =>
        {
            let d = lower_expr(ctx, e)?;
            ctx.block().toint32(&d)
        }
        Expr::Binary { op, left, right } if is_i32_chain_op(*op) => {
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
