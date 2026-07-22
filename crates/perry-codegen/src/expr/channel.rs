//! Channel-vector reduction detection + emission (extracted from `expr.rs`,
//! issue #1098). Pure move — no logic changes.

use anyhow::{anyhow, Result};
use perry_hir::Expr;

use super::{can_lower_expr_as_i32, lower_expr, lower_expr_as_i32, FnCtx};
use crate::types::{DOUBLE, I32, I8, PTR};

/// Return the HIR enum variant name for an expression. Uses Debug
/// formatting and extracts the leading identifier so we get the actual
/// variant name (e.g. `"ArrayMap"`, `"BufferAlloc"`, `"RegExpExec"`)
/// without having to maintain an exhaustive match against ~200 HIR
/// variants. The result is used in "X not yet supported" error messages
/// to tell the user exactly which HIR variant the LLVM backend is
/// missing — critical for prioritizing the next slice.
pub(crate) fn variant_name(e: &Expr) -> String {
    let dbg = format!("{:?}", e);
    let end = dbg.find([' ', '(', '{']).unwrap_or(dbg.len());
    dbg[..end].to_string()
}

/// Issue #179 typed-parse, Step 1b codegen helper.
///
/// Given the `ty` from `JsonParseTyped`, return the packed-keys bytes
/// and field count if `ty` is `Array<Object>` with a declared field
/// list we can specialize on. Returns `None` otherwise — caller falls
/// through to the generic `js_json_parse`.
///
/// Packed format matches `js_build_class_keys_array`: null-separated
/// UTF-8 field names, trailing `\0` optional. Only primitive/leaf
/// field types are allowed in the MVP (number, string, boolean,
/// bigint, null, number-or-string unions) — nested objects and arrays
/// inside a record still parse through the generic path, which is fine:
/// the outer record is still pre-shaped, and nested values go through
/// `parse_value_generic` inside `parse_object_shaped`.
pub(crate) fn extract_array_of_object_shape(
    ty: &perry_hir::types::Type,
    ordered_keys: Option<&[String]>,
) -> Option<(Vec<u8>, u32)> {
    use perry_hir::types::Type;
    let elem = match ty {
        Type::Array(inner) => &**inner,
        Type::Generic { base, type_args } if base == "Array" && type_args.len() == 1 => {
            &type_args[0]
        }
        _ => return None,
    };
    let obj = match elem {
        Type::Object(o) => o,
        _ => return None,
    };
    if obj.properties.is_empty() {
        return None;
    }
    // Prefer the AST-source order (matches typical JSON.stringify
    // output layout — enables the fast-path per-field compare in
    // `parse_object_shaped`). Fall back to alphabetical if unavailable.
    // Runtime correctness is order-independent either way — the slow
    // path handles mismatches.
    let keys: Vec<String> = if let Some(ord) = ordered_keys {
        // Filter to only keys that are actually in the ObjectType
        // properties (defensive against AST/type mismatch).
        ord.iter()
            .filter(|k| obj.properties.contains_key(k.as_str()))
            .cloned()
            .collect()
    } else {
        let mut v: Vec<String> = obj.properties.keys().cloned().collect();
        v.sort();
        v
    };
    if keys.is_empty() {
        return None;
    }
    let mut packed: Vec<u8> = Vec::new();
    for (i, k) in keys.iter().enumerate() {
        if i > 0 {
            packed.push(0);
        }
        packed.extend_from_slice(k.as_bytes());
    }
    Some((packed, keys.len() as u32))
}

/// Channel-vector reduction match on a sequence of consecutive
/// `Stmt::Expr(LocalSet)` statements. The canonical hot shape is
/// image_convolution's blur kernel inner body:
///
/// ```ts
/// rAcc += src[idx]     * k;
/// gAcc += src[idx + 1] * k;
/// bAcc += src[idx + 2] * k;
/// ```
///
/// Each statement decomposes to:
///   `LocalSet(acc_i, Binary{Add, LocalGet(acc_i),
///                          Binary{Mul, Uint8ArrayGet{arr, idx_i}, k}})`
///
/// where `arr` and `k` are identical across the group, the targets
/// (`acc_i`) are distinct, and the indices form a length-N consecutive
/// integer progression starting from a common `base_idx`. The match
/// detects N = 3 (RGB) and N = 4 (RGBA) cases — the specific shapes that
/// fit cleanly into ARM NEON's 128-bit vector lane (`<4 x i32>`).
///
/// Returned data is what the emitter needs to lay down the vector form:
/// the target slot ids, the shared array LocalId, the per-channel
/// integer offsets (e.g. `[0, 1, 2]`), the base index expression
/// (whatever `idx` is), and the common factor `k` expression.
pub(crate) struct ChannelReduction {
    pub acc_ids: Vec<u32>,
    pub array_id: u32,
    pub base_idx: Box<Expr>,
    pub offsets: Vec<i32>,
    pub k_expr: Box<Expr>,
}

/// Try to fuse `stmts[at..]` into a `ChannelReduction`. Returns `Some` if
/// the next 3 or 4 statements match exactly the shape described in the
/// `ChannelReduction` doc; `None` otherwise. The caller advances by
/// `acc_ids.len()` on a hit so the matched statements aren't lowered
/// scalar.
pub(crate) fn try_match_channel_reduction(
    stmts: &[perry_hir::Stmt],
    at: usize,
    integer_locals: &std::collections::HashSet<u32>,
) -> Option<ChannelReduction> {
    use perry_hir::{BinaryOp, Stmt};
    // Try N = 4 first so an RGBA workload picks the wider lane; fall
    // back to N = 3.
    for n in [4usize, 3] {
        if at + n > stmts.len() {
            continue;
        }
        let mut acc_ids: Vec<u32> = Vec::with_capacity(n);
        let mut array_id_opt: Option<u32> = None;
        let mut base_idx_opt: Option<Box<Expr>> = None;
        let mut offsets: Vec<i32> = Vec::with_capacity(n);
        let mut k_opt: Option<Box<Expr>> = None;
        let mut all_match = true;
        for i in 0..n {
            let s = &stmts[at + i];
            let (target_id, value) = match s {
                Stmt::Expr(Expr::LocalSet(id, v)) => (*id, v.as_ref()),
                _ => {
                    all_match = false;
                    break;
                }
            };
            // Targets must be integer-stable so the i32 lanes are valid.
            if !integer_locals.contains(&target_id) {
                all_match = false;
                break;
            }
            // Targets must be distinct (no two channels writing the same
            // slot — that would be aliasing the reduction).
            if acc_ids.contains(&target_id) {
                all_match = false;
                break;
            }
            // Shape: Add(LocalGet(target_id), Mul(Uint8ArrayGet(arr, idx), k))
            let (lhs, rhs) = match value {
                Expr::Binary {
                    op: BinaryOp::Add,
                    left,
                    right,
                } => (left.as_ref(), right.as_ref()),
                _ => {
                    all_match = false;
                    break;
                }
            };
            // lhs must be LocalGet of the target (acc += ...)
            if !matches!(lhs, Expr::LocalGet(id) if *id == target_id) {
                all_match = false;
                break;
            }
            let (mul_left, mul_right) = match rhs {
                Expr::Binary {
                    op: BinaryOp::Mul,
                    left,
                    right,
                } => (left.as_ref(), right.as_ref()),
                _ => {
                    all_match = false;
                    break;
                }
            };
            // The Uint8ArrayGet is on the left of the Mul as perry's
            // HIR lowers it. Don't try the reverse — keeping the match
            // narrow avoids false positives on `k * src[i]` shapes that
            // would need a different operand ordering.
            let (arr_id, idx_expr) = match mul_left {
                Expr::Uint8ArrayGet { array, index } => match array.as_ref() {
                    Expr::LocalGet(id) => (*id, index.as_ref()),
                    _ => {
                        all_match = false;
                        break;
                    }
                },
                _ => {
                    all_match = false;
                    break;
                }
            };
            // Decompose the index: either bare `LocalGet(idx)` (offset 0)
            // or `Add(LocalGet(idx), Integer(N))` (offset N).
            let (this_base, this_offset): (Box<Expr>, i32) = match idx_expr {
                Expr::LocalGet(_) => (Box::new(idx_expr.clone()), 0i32),
                Expr::Binary {
                    op: BinaryOp::Add,
                    left,
                    right,
                } => {
                    if let Expr::Integer(n) = right.as_ref() {
                        match i32::try_from(*n) {
                            Ok(v) => (Box::new((**left).clone()), v),
                            Err(_) => {
                                all_match = false;
                                break;
                            }
                        }
                    } else {
                        all_match = false;
                        break;
                    }
                }
                _ => {
                    all_match = false;
                    break;
                }
            };
            // First channel pins array, base, k. Subsequent channels must
            // match on all three. We use a structural comparison via
            // `format!("{:?}", expr)` — the expr trees we see here are
            // shallow LocalGet / Integer / Binary and the Debug output is
            // stable across runs.
            match (&array_id_opt, &base_idx_opt, &k_opt) {
                (None, None, None) => {
                    array_id_opt = Some(arr_id);
                    base_idx_opt = Some(this_base);
                    k_opt = Some(Box::new(mul_right.clone()));
                }
                (Some(prev_arr), Some(prev_base), Some(prev_k)) => {
                    if *prev_arr != arr_id {
                        all_match = false;
                        break;
                    }
                    if format!("{:?}", prev_base.as_ref()) != format!("{:?}", this_base.as_ref()) {
                        all_match = false;
                        break;
                    }
                    if format!("{:?}", prev_k.as_ref()) != format!("{:?}", mul_right) {
                        all_match = false;
                        break;
                    }
                }
                _ => unreachable!(),
            }
            acc_ids.push(target_id);
            offsets.push(this_offset);
        }
        if !all_match {
            continue;
        }
        // Offsets must be a length-N consecutive integer progression
        // (any starting point — the canonical case is `[0, 1, 2]` but
        // `[1, 2, 3]` would fuse identically). Require strictly
        // monotonic increase by 1 to keep the gather pattern a contiguous
        // load instead of a scatter.
        let mut sorted = offsets.clone();
        sorted.sort();
        let mut consecutive = true;
        for i in 1..sorted.len() {
            if sorted[i] != sorted[i - 1] + 1 {
                consecutive = false;
                break;
            }
        }
        if !consecutive {
            continue;
        }
        // Order acc_ids and offsets by offset so the emitter walks them
        // in lane order.
        let mut paired: Vec<(i32, u32)> = offsets
            .iter()
            .zip(acc_ids.iter())
            .map(|(o, a)| (*o, *a))
            .collect();
        paired.sort_by_key(|p| p.0);
        let offsets: Vec<i32> = paired.iter().map(|p| p.0).collect();
        let acc_ids: Vec<u32> = paired.iter().map(|p| p.1).collect();
        return Some(ChannelReduction {
            acc_ids,
            array_id: array_id_opt.unwrap(),
            base_idx: base_idx_opt.unwrap(),
            offsets,
            k_expr: k_opt.unwrap(),
        });
    }
    None
}

/// Emit the vectorized form of a `ChannelReduction`. Produces a
/// `<4 x i32>` SIMD multiply-add — the widest lane that fits ARM NEON's
/// 128-bit register and AVX-256/SSE2's narrowest int-vector mode. The
/// 4th lane is unused for the N = 3 (RGB) case and zeroed for safety;
/// LLVM's instcombine collapses it on platforms where partial-vector
/// stores cost more than the dead lane.
///
/// Requires the array to have a `buffer_data_slot` entry — without the
/// pre-computed data ptr we'd have to re-derive it inline, which costs
/// the same as the scalar Uint8ArrayGet path and gives up the win.
/// Caller (in `lower_stmts`) checks this and falls back to scalar
/// lowering when absent.
pub(crate) fn lower_channel_reduction(ctx: &mut FnCtx<'_>, r: &ChannelReduction) -> Result<()> {
    let Some((ptr_slot, scope_idx)) = ctx.buffer_data_slots.get(&r.array_id).cloned() else {
        return Err(anyhow!(
            "lower_channel_reduction: array {} has no buffer_data_slot — caller should have skipped",
            r.array_id
        ));
    };
    // Lower the base index to i32 ahead of time — the gather reads N
    // consecutive bytes from `data_ptr + base_idx + offset[c]`.
    let i32_slots = ctx.i32_counter_slots.clone();
    let flat_ca = ctx.flat_const_arrays.clone();
    let ara = ctx.array_row_aliases.clone();
    let int_locals = ctx.integer_locals.clone();
    let base_idx_can_i32 = can_lower_expr_as_i32(
        &r.base_idx,
        &i32_slots,
        &flat_ca,
        &ara,
        &int_locals,
        ctx.clamp3_functions,
        ctx.clamp_u8_functions,
        ctx.integer_returning_functions,
        ctx.i32_identity_functions,
    );
    let base_idx_i32 = if base_idx_can_i32 {
        lower_expr_as_i32(ctx, &r.base_idx)?
    } else {
        let d = lower_expr(ctx, &r.base_idx)?;
        ctx.block().fptosi(DOUBLE, &d, I32)
    };
    let k_can_i32 = can_lower_expr_as_i32(
        &r.k_expr,
        &i32_slots,
        &flat_ca,
        &ara,
        &int_locals,
        ctx.clamp3_functions,
        ctx.clamp_u8_functions,
        ctx.integer_returning_functions,
        ctx.i32_identity_functions,
    );
    let k_i32 = if k_can_i32 {
        lower_expr_as_i32(ctx, &r.k_expr)?
    } else {
        let d = lower_expr(ctx, &r.k_expr)?;
        ctx.block().fptosi(DOUBLE, &d, I32)
    };
    let blk = ctx.block();
    // Buffer length-load via the data ptr's preceding header. The 4-byte
    // i32 length sits 8 bytes before the data start (BufferHeader
    // layout, identical to the scalar Uint8ArrayGet path).
    let data_ptr = blk.load(PTR, &ptr_slot);
    let header_ptr = blk.gep(I8, &data_ptr, &[(I32, "-8")]);
    let len_i32 = blk.load_invariant(I32, &header_ptr);
    // Tell LLVM the highest channel offset is in-bounds. The
    // `Uint8ArrayGet` scalar path emits one assume per access; one
    // assume covering the highest offset is sufficient because
    // `inbounds(base + max_offset)` implies `inbounds(base + i)` for
    // `0 <= i <= max_offset`.
    let max_off = *r.offsets.last().unwrap();
    let max_idx = blk.add(I32, &base_idx_i32, &max_off.to_string());
    let in_bounds = blk.icmp_ult(I32, &max_idx, &len_i32);
    blk.emit_raw(format!("call void @llvm.assume(i1 {})", in_bounds));
    // Build the byte vector by N consecutive loads + insertelement. LLVM
    // SLP combines the scalar loads into a single vector load when the
    // addresses are contiguous (which they are after the assume above).
    let alias_meta = crate::expr::buffer_alias_metadata_suffix(scope_idx);
    let _n = r.offsets.len();
    // Lane width: <4 x i32> regardless of N, padded with 0s in unused lanes.
    // The first insertelement seeds from a constant 0-vector; subsequent
    // ones chain through register values.
    let mut vec_i32 = "<i32 0, i32 0, i32 0, i32 0>".to_string();
    for (lane, &offset) in r.offsets.iter().enumerate() {
        let off_i32 = blk.add(I32, &base_idx_i32, &offset.to_string());
        let byte_ptr = blk.gep_inbounds(I8, &data_ptr, &[(I32, &off_i32)]);
        let byte_val = blk.fresh_reg();
        blk.emit_raw(format!(
            "{} = load i8, ptr {}{}",
            byte_val, byte_ptr, alias_meta
        ));
        let i32_val = blk.zext(I8, &byte_val, I32);
        let new_vec = blk.fresh_reg();
        blk.emit_raw(format!(
            "{} = insertelement <4 x i32> {}, i32 {}, i32 {}",
            new_vec, vec_i32, i32_val, lane
        ));
        vec_i32 = new_vec;
    }
    // Splat k across the vector. Insertelement at lane 0, then
    // shufflevector with an all-zero mask copies it to every lane.
    let k_at_0 = blk.fresh_reg();
    blk.emit_raw(format!(
        "{} = insertelement <4 x i32> poison, i32 {}, i32 0",
        k_at_0, k_i32
    ));
    let k_splat = blk.fresh_reg();
    blk.emit_raw(format!(
        "{} = shufflevector <4 x i32> {}, <4 x i32> poison, <4 x i32> zeroinitializer",
        k_splat, k_at_0
    ));
    // <4 x i32> mul = bytes * k_splat.
    let mul_vec = blk.fresh_reg();
    blk.emit_raw(format!(
        "{} = mul <4 x i32> {}, {}",
        mul_vec, vec_i32, k_splat
    ));
    // Load the per-channel accumulators into a vector. Each acc has an
    // i32 slot from `needs_i32_slot` (asserted via integer_locals
    // membership during detection) — this is the load that scalar code
    // would do anyway, just packed into a single vector value.
    let mut acc_vec = "<i32 0, i32 0, i32 0, i32 0>".to_string();
    for (lane, &acc_id) in r.acc_ids.iter().enumerate() {
        let i32_slot = ctx.i32_counter_slots.get(&acc_id).cloned().ok_or_else(|| {
            anyhow!(
                "channel reduction acc {} missing i32 slot — should be in integer_locals",
                acc_id
            )
        })?;
        let blk = ctx.block();
        let acc_val = blk.load(I32, &i32_slot);
        let new_vec = blk.fresh_reg();
        blk.emit_raw(format!(
            "{} = insertelement <4 x i32> {}, i32 {}, i32 {}",
            new_vec, acc_vec, acc_val, lane
        ));
        acc_vec = new_vec;
    }
    // <4 x i32> add = acc + (bytes * k).
    let blk = ctx.block();
    let new_acc_vec = blk.fresh_reg();
    blk.emit_raw(format!(
        "{} = add <4 x i32> {}, {}",
        new_acc_vec, acc_vec, mul_vec
    ));
    // Extract per-lane and store back. Mirror writes to both the i32
    // and double slots so downstream readers see consistent values.
    for (lane, &acc_id) in r.acc_ids.iter().enumerate() {
        let i32_slot = ctx
            .i32_counter_slots
            .get(&acc_id)
            .cloned()
            .ok_or_else(|| anyhow!("acc {} missing i32 slot", acc_id))?;
        let dbl_slot = ctx
            .locals
            .get(&acc_id)
            .cloned()
            .ok_or_else(|| anyhow!("acc {} missing double slot", acc_id))?;
        let blk = ctx.block();
        let lane_val = blk.fresh_reg();
        blk.emit_raw(format!(
            "{} = extractelement <4 x i32> {}, i32 {}",
            lane_val, new_acc_vec, lane
        ));
        blk.store(I32, &lane_val, &i32_slot);
        let dbl_val = blk.sitofp(I32, &lane_val, DOUBLE);
        blk.store(DOUBLE, &dbl_val, &dbl_slot);
    }
    Ok(())
}
