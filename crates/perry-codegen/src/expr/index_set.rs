//! IndexSet (arr[i] = v).
//!
//! Extracted from `expr/mod.rs` to keep that file under the 2000-line cap.
//! Pure mechanical move — match arm bodies are verbatim copies, called from
//! `lower_expr`'s outer dispatch.
//!
//! # Rooting (Layer 1, slice 4)
//!
//! Migrated onto [`crate::rooting`]; this module names no `expr::temp_root`
//! symbol. Every arm is the same window — `o[k] = v` evaluates the reference
//! before the value (spec order), so the receiver, and on the dynamic arms the
//! key too, sit in SSA registers while arbitrary user code runs.
//!
//! Each arm is one call to [`crate::rooting::with_operands_rooted`] over
//! `[object, index, value]` (the value is lowered by `lower_expr`) or to
//! [`crate::rooting::with_operands_rooted_across`] over `[object]` / `[object,
//! index]` with `[index, value]` as the window (the value is lowered by
//! `lower_value_for_dynamic_index_set`, which the operand list cannot produce).
//! The point of the operand-list form is that it *derives* the per-operand
//! window rather than restating it: #7201's disjunction — the receiver is live
//! across BOTH the key and the value — falls out of the list order instead of
//! being a hand-written `recv_collects`, and the two arms that got that flag
//! wrong are exactly the two bugs this slice fixed (#7638, #7639).
//!
//! Nesting note that no longer applies: the dynamic-string-key arm used to push
//! two guards and release them inner-to-outer, because `temp_root_truncate` is a
//! stack CUT. One operand group has one guard, so that ordering obligation is
//! gone rather than merely documented.

use anyhow::Result;
use perry_hir::{BinaryOp, Expr};

use crate::nanbox::{double_literal, POINTER_MASK_I64};
use crate::native_value::{
    BoundsState, BufferAccessMode, ExpectedNativeRep, LoweredValue, MaterializationReason,
    NativeRep, SemanticKind,
};
use crate::rooting;
use crate::type_analysis::{is_array_expr, is_numeric_expr, is_string_expr, receiver_class_name};
use crate::types::{DOUBLE, I32, I64};

use super::index_set_packed_loop::{
    lower_packed_f64_range_loop_index_set, lower_packed_numeric_loop_index_set,
};
use super::index_set_typed_array::lower_inline_dyn_typed_array_set;
use super::{
    array_kind_fact, array_store_needs_layout_note, array_store_needs_write_barrier,
    attach_buffer_view_pointer_state_for_expr, buffer_access_materialization_reason,
    emit_array_numeric_write_note_on_block, emit_jsvalue_slot_store_on_block,
    emit_root_nanbox_store_on_block, emit_typed_feedback_register_site, emit_write_barrier,
    expr_has_numeric_pointer_free_array_layout, int_range_expr, lower_buffer_store, lower_expr,
    lower_expr_as_i32, lower_expr_native, lower_index_set_fast, lower_typed_array_store,
    materialize_js_value, nanbox_pointer_inline, raw_f64_layout_fact, unbox_str_handle,
    unbox_to_i64, BufferAccessSpec, FnCtx, PackedF64LoopFact, PackedNumericLoopKind,
    TypedFeedbackContract, TypedFeedbackKind,
};

pub(super) fn canonicalize_raw_f64_numeric_store_value(
    blk: &mut crate::block::LlBlock,
    value_double: &str,
) -> String {
    blk.call(
        DOUBLE,
        "js_array_numeric_value_to_raw_f64",
        &[(DOUBLE, value_double)],
    )
}

fn lower_value_for_optional_barrier(
    ctx: &mut FnCtx<'_>,
    value: &Expr,
    write_barrier_needed: bool,
) -> Result<(String, Option<String>)> {
    if !write_barrier_needed {
        let value_double = lower_expr(ctx, value)?;
        let lowered_js = LoweredValue::js_value(value_double.clone());
        ctx.record_lowered_value_with_access_mode(
            "WriteBarrierElided",
            None,
            "write_barrier.elided_non_pointer_child",
            &lowered_js,
            None,
            None,
            None,
            None,
            false,
            false,
            vec!["reason=statically_non_pointer_child".to_string()],
        );
        return Ok((value_double, None));
    }
    let value_bits = lower_expr_native(ctx, value, ExpectedNativeRep::JsValueBits)?.value;
    let value_double = ctx.block().bitcast_i64_to_double(&value_bits);
    Ok((value_double, Some(value_bits)))
}

fn lower_value_for_dynamic_index_set(
    ctx: &mut FnCtx<'_>,
    value: &Expr,
    consumer: &str,
    boxed_at: &str,
) -> Result<(String, String)> {
    let lowered = lower_expr_native(ctx, value, ExpectedNativeRep::JsValueBits)?;
    let value_bits = lowered.value.clone();
    let value_double = ctx.block().bitcast_i64_to_double(&value_bits);
    ctx.record_lowered_value(
        "IndexSet",
        None,
        consumer,
        &lowered,
        None,
        None,
        None,
        false,
        false,
        vec![format!("boxed_at={boxed_at}")],
    );
    Ok((value_double, value_bits))
}

/// #7494: `static_type_of`, not `receiver_class_name` — see the sibling in
/// `index_get.rs` for the full rationale. In short: every tier this predicate
/// gates is either `ctx.buffer_view_slots`-tracked (which reassignment
/// already invalidates on its own) or a genuinely dynamic runtime call that
/// re-validates the object's actual GC kind, so `receiver_class_name`'s
/// blanket "reassigned local → unknown" answer only broke the dynamic-
/// fallback arm's own documented promise ("aliases, reassigned locals, and
/// unknown bounds stay on the runtime helper") by never letting execution
/// reach it — sending a reassigned typed array's `arr[i] = v` on to
/// `is_array_expr`'s PLAIN-array element layout (byte 8) against a real
/// typed-array object (data at byte 16): a type-confused write, not a missed
/// optimization.
fn is_width_tracked_typed_array_receiver(ctx: &FnCtx<'_>, object: &Expr) -> bool {
    if matches!(object, Expr::LocalGet(id) if ctx.buffer_view_slots.contains_key(id)) {
        return true;
    }
    let ty = match object {
        Expr::LocalGet(id) => ctx.local_type_hint(id).cloned(),
        _ => crate::type_analysis::static_type_of(ctx, object),
    };
    matches!(
        ty,
        Some(perry_hir::types::Type::Named(name)) if matches!(
            name.as_str(),
            "Int8Array"
                | "Uint8ClampedArray"
                | "Int16Array"
                | "Uint16Array"
                | "Int32Array"
                | "Uint32Array"
                | "Float16Array"
                | "Float32Array"
                | "Float64Array"
        )
    )
}

fn is_uint8array_receiver(ctx: &FnCtx<'_>, object: &Expr) -> bool {
    matches!(
        receiver_class_name(ctx, object).as_deref(),
        Some("Uint8Array")
    )
}

fn numeric_index_has_integer_array_index_proof(ctx: &FnCtx<'_>, index: &Expr) -> bool {
    fn range_is_nonnegative_i32(ctx: &FnCtx<'_>, index: &Expr) -> bool {
        int_range_expr(ctx, index)
            .is_some_and(|range| range.min >= 0 && range.max <= i32::MAX as i64)
    }

    match index {
        Expr::Integer(i) => (0..=i32::MAX as i64).contains(i),
        Expr::Number(n) => n.is_finite() && n.fract() == 0.0 && *n >= 0.0 && *n <= i32::MAX as f64,
        Expr::Binary { op, left, right } if matches!(op, BinaryOp::BitAnd) => {
            bitand_has_nonnegative_i32_mask(left, right)
        }
        Expr::LocalGet(id) => {
            ctx.integer_locals.contains(id)
                && ctx.i32_counter_slots.contains_key(id)
                && (ctx.nonnegative_integer_locals.contains(id)
                    || ctx
                        .int_range_facts
                        .iter()
                        .any(|fact| fact.local_id == *id && fact.range.min >= 0))
                || range_is_nonnegative_i32(ctx, index)
        }
        _ => range_is_nonnegative_i32(ctx, index),
    }
}

fn bitand_has_nonnegative_i32_mask(left: &Expr, right: &Expr) -> bool {
    fn mask(expr: &Expr) -> Option<i64> {
        match expr {
            Expr::Integer(i) => Some(*i),
            Expr::Number(n) if n.is_finite() && n.fract() == 0.0 => Some(*n as i64),
            _ => None,
        }
    }
    mask(left)
        .or_else(|| mask(right))
        .is_some_and(|mask| (0..=i32::MAX as i64).contains(&mask))
}

pub(super) fn packed_f64_loop_fact(
    ctx: &FnCtx<'_>,
    arr_id: u32,
    idx_id: u32,
) -> Option<PackedF64LoopFact> {
    ctx.packed_f64_loop_facts
        .iter()
        .find(|fact| fact.array_local_id == arr_id && fact.index_local_id == idx_id)
        .cloned()
}

/// #6011: fact lookup for `(arr, index-expr)` where the index may be
/// `i` or `i ± c`. Non-zero offsets only match hole-tolerant (range-guarded)
/// facts — see the twin helper in `index_get.rs`.
fn packed_f64_loop_fact_for_index(
    ctx: &FnCtx<'_>,
    arr_id: u32,
    index: &Expr,
) -> Option<(PackedF64LoopFact, u32, i32)> {
    let (idx_id, offset) = super::packed_f64_loop_index_parts(index)?;
    let fact = packed_f64_loop_fact(ctx, arr_id, idx_id)?;
    if offset != 0 && !fact.allow_holes {
        return None;
    }
    Some((fact, idx_id, offset))
}

fn load_packed_loop_index_i32(ctx: &mut FnCtx<'_>, i32_slot: &str, offset: i32) -> String {
    let idx_i32 = ctx.block().load(I32, i32_slot);
    match offset.cmp(&0) {
        std::cmp::Ordering::Equal => idx_i32,
        std::cmp::Ordering::Greater => ctx.block().add(I32, &idx_i32, &offset.to_string()),
        std::cmp::Ordering::Less => ctx.block().sub(I32, &idx_i32, &(-offset).to_string()),
    }
}

fn numeric_index_has_loop_array_index_proof(ctx: &FnCtx<'_>, object: &Expr, index: &Expr) -> bool {
    let Expr::LocalGet(arr_id) = object else {
        return false;
    };
    let Some((idx_id, offset)) = super::packed_f64_loop_index_parts(index) else {
        return false;
    };
    if !ctx.i32_counter_slots.contains_key(&idx_id) {
        return false;
    }
    if packed_f64_loop_fact_for_index(ctx, *arr_id, index).is_some() {
        return true;
    }
    offset == 0
        && ctx
            .bounded_index_pairs
            .iter()
            .any(|fact| fact.array_local_id == *arr_id && fact.index_local_id == idx_id)
}

fn numeric_index_needs_runtime_key(ctx: &FnCtx<'_>, object: &Expr, index: &Expr) -> bool {
    // The inline array fast paths take an i32 index, so the conversion is only
    // sound after proving JS array-index semantics. A dynamic numeric value like
    // `let k = 1.5; arr[k] = v` must reach the runtime key helper and write the
    // property "1.5" instead of truncating to element 1 before a guard can see it.
    is_numeric_expr(ctx, index)
        && !numeric_index_has_integer_array_index_proof(ctx, index)
        && !numeric_index_has_loop_array_index_proof(ctx, object, index)
}

/// Whether a value is worth routing through the numeric-array store guard.
///
/// A source type is only a candidate here: both guarded store tiers validate
/// the live JSValue before a raw-f64 store and retain the boxed fallback.  It
/// must never be reused to elide layout notes or write barriers.
fn guarded_numeric_array_store_candidate(ctx: &FnCtx<'_>, value: &Expr) -> bool {
    crate::codegen::typed_arg_is_guard_candidate(ctx, crate::codegen::TypedParamRep::F64, value)
}

fn typed_array_index_needs_runtime_key(ctx: &FnCtx<'_>, object: &Expr, index: &Expr) -> bool {
    !numeric_index_has_integer_array_index_proof(ctx, index)
        && !numeric_index_has_loop_array_index_proof(ctx, object, index)
}

fn lower_array_index_set_via_runtime_key(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    index: &Expr,
    value: &Expr,
    source_label: &str,
) -> Result<String> {
    // #7341, same hazard as the packed path: the receiver is live across both
    // `index` and `value` lowering, and an allocating RHS is a collection
    // point. Without this the store writes through a pre-evacuation address.
    //
    // `across` rather than the plain operand form: the value is lowered by
    // `lower_value_for_dynamic_index_set`, a native-rep lowering the operand
    // list cannot produce. Passing `[index, value]` as the window keeps its
    // extent answered by `operand_protection` — the #7201 disjunction — rather
    // than by a `recv_collects` flag this function re-derives.
    rooting::with_operands_rooted_across(
        ctx,
        &[object],
        &[index, value],
        |ctx| {
            let idx_double = lower_expr(ctx, index)?;
            let value_needs_barrier = array_store_needs_write_barrier(ctx, value);
            let (val_double, val_bits) = lower_value_for_dynamic_index_set(
                ctx,
                value,
                "index_set.array_runtime_key_value_bits",
                "array_runtime_key_set_helper_edge",
            )?;
            Ok((idx_double, value_needs_barrier, val_double, val_bits))
        },
        |ctx, vals, (idx_double, value_needs_barrier, val_double, val_bits)| {
            let arr_box = &vals[0];
            let arr_handle = {
                let blk = ctx.block();
                unbox_to_i64(blk, arr_box)
            };
            let site_id = emit_typed_feedback_register_site(
                ctx,
                TypedFeedbackKind::ArrayElement,
                source_label,
                TypedFeedbackContract::array_set_index_or_string(),
            );
            let new_handle = ctx.block().call(
                I64,
                "js_typed_feedback_array_set_index_or_string",
                &[
                    (I64, &site_id),
                    (I64, &arr_handle),
                    (DOUBLE, &idx_double),
                    (DOUBLE, &val_double),
                ],
            );
            if let Expr::LocalGet(id) = object {
                if let Some(slot) = ctx.locals.get(id).cloned() {
                    let new_box = nanbox_pointer_inline(ctx.block(), &new_handle);
                    ctx.block().store(DOUBLE, &new_box, &slot);
                } else if let Some(global_name) = ctx.module_globals.get(id).cloned() {
                    let new_box = nanbox_pointer_inline(ctx.block(), &new_handle);
                    let g_ref = format!("@{}", global_name);
                    emit_root_nanbox_store_on_block(ctx.block(), &new_box, &g_ref);
                }
            }
            if value_needs_barrier {
                let arr_bits = ctx.block().bitcast_double_to_i64(arr_box);
                emit_write_barrier(ctx, &arr_bits, &val_bits);
            }
            Ok(val_double)
        },
    )
}

pub(crate) fn lower(
    ctx: &mut FnCtx<'_>,
    expr: &Expr,
    // #7590: THIS expression's value is discarded (not merely the statement's).
    value_discarded: bool,
) -> Result<String> {
    match expr {
        Expr::IndexSet {
            object,
            index,
            value,
        } => {
            if let Some(result) =
                super::typed_array_rmw::try_lower_guarded_uint32_add(ctx, object, index, value)?
            {
                if value_discarded {
                    return Ok(double_literal(0.0));
                }
                return Ok(result);
            }
            // Issue #611: `globalThis[<key>] = value` writes to the
            // persistent global-this singleton (see the matching IndexGet
            // arm above for context).
            if matches!(object.as_ref(), Expr::GlobalGet(_))
                && (matches!(index.as_ref(), Expr::String(_)) || is_string_expr(ctx, index))
            {
                let global_box = ctx.block().call(DOUBLE, "js_get_global_this", &[]);
                // #7640 section A: `key_box` is a heap string by construction
                // on this arm (`is_string_expr`) and `global_box` a copy of
                // the (registered-root-backed, but movable) globalThis
                // singleton — both were held as bare registers across
                // `value`'s lowering, which is arbitrary user code and can
                // allocate. Root both in one group; `index`'s own window ends
                // at `value` (nothing else is lowered after it), so `value`
                // itself takes no slot.
                return rooting::with_rooted_group(ctx, 2, |ctx, group| {
                    let global_idx =
                        group.adopt_emitted(ctx, rooting::Repr::Boxed, &global_box, true);
                    let key_idx = group.lower(ctx, index, true)?;
                    let val_double = lower_expr(ctx, value)?;
                    let key_box = group.reread(ctx, key_idx)?;
                    let global_box = group.reread_emitted(ctx, global_idx);
                    let (obj_handle, key_handle) = {
                        let blk = ctx.block();
                        // #7640 section D: key first — `unbox_str_handle` can
                        // allocate (SSO materialisation), and a raw receiver
                        // pointer taken above it is unrootable.
                        let key_handle = unbox_str_handle(blk, &key_box);
                        let obj_handle = unbox_to_i64(blk, &global_box);
                        (obj_handle, key_handle)
                    };
                    let site_id = emit_typed_feedback_register_site(
                        ctx,
                        TypedFeedbackKind::PropertySet,
                        "globalThis[index]",
                        TypedFeedbackContract::object_set_by_name(),
                    );
                    ctx.block().call_void(
                        "js_typed_feedback_object_set_field_by_name",
                        &[
                            (I64, &site_id),
                            (I64, &obj_handle),
                            (I64, &key_handle),
                            (DOUBLE, &val_double),
                        ],
                    );
                    Ok(val_double)
                });
            }
            if is_width_tracked_typed_array_receiver(ctx, object) {
                // A non-numeric index (a Symbol, or a string property name) is
                // never an integer-indexed element. The width-tracked native
                // store coerces the index with `fptosi`, which truncates a
                // NaN-boxed Symbol to 0 and clobbers element 0 instead of
                // storing the symbol property (test262 TypedArray symbol-key
                // internals, #5735). Route such keys through the runtime
                // dispatcher, which triages symbol / string / numeric keys —
                // mirroring the symmetric IndexGet guard (index_get.rs). A
                // literal / loop-counter index stays `is_numeric_expr`, so every
                // proven element fast path below is preserved.
                if !is_numeric_expr(ctx, index) {
                    // #7640 section A: receiver AND key were both lowered
                    // before `value`, with no rooting decision at all. The
                    // typed-array object itself is old-arena/non-movable
                    // (CLAUDE.md's GC section), but the KEY is not — a
                    // non-numeric index here is a Symbol or string, and a
                    // `Symbol()`/computed-key expression can allocate the
                    // interned symbol, leaving a from-space key by the time
                    // `value`'s own (arbitrary, allocating) evaluation runs.
                    return rooting::with_operands_rooted(
                        ctx,
                        &[object, index, value],
                        |ctx, vals| {
                            let (arr_box, idx_double, val_double) = (&vals[0], &vals[1], &vals[2]);
                            let blk = ctx.block();
                            let arr_bits = blk.bitcast_double_to_i64(arr_box);
                            let arr_i64 = blk.and(I64, &arr_bits, POINTER_MASK_I64);
                            Ok(blk.call(
                                DOUBLE,
                                "js_typed_array_index_set_dynamic",
                                &[(I64, &arr_i64), (DOUBLE, idx_double), (DOUBLE, val_double)],
                            ))
                        },
                    );
                }
                if let Some(store) = lower_typed_array_store(ctx, object, index, value)? {
                    if value_discarded {
                        return Ok(double_literal(0.0));
                    }
                    return Ok(materialize_js_value(
                        ctx,
                        store.result,
                        MaterializationReason::FunctionAbi,
                    ));
                }
                // Phase 2: storage-proven view with a dynamic (bounds-unproven)
                // exact-i32 index — inline checked store (OOB = silent no-op),
                // no kind guard, no runtime call.
                if let Some(stored) =
                    super::try_lower_proven_view_checked_store(ctx, object, index, value)?
                {
                    if value_discarded {
                        return Ok(double_literal(0.0));
                    }
                    return Ok(materialize_js_value(
                        ctx,
                        stored,
                        MaterializationReason::FunctionAbi,
                    ));
                }
                if typed_array_index_needs_runtime_key(ctx, object.as_ref(), index.as_ref()) {
                    return rooting::with_operands_rooted(
                        ctx,
                        &[object, index, value],
                        |ctx, vals| {
                            let blk = ctx.block();
                            let arr_bits = blk.bitcast_double_to_i64(&vals[0]);
                            let arr_i64 = blk.and(I64, &arr_bits, POINTER_MASK_I64);
                            let result = blk.call(
                                DOUBLE,
                                "js_typed_array_index_set_dynamic",
                                &[(I64, &arr_i64), (DOUBLE, &vals[1]), (DOUBLE, &vals[2])],
                            );
                            let slow = LoweredValue::js_value(result.clone());
                            ctx.record_lowered_value_with_access_mode(
                                "TypedArraySet",
                                None,
                                "TypedArraySet.slow_path",
                                &slow,
                                Some(BoundsState::Unknown),
                                None,
                                Some(BufferAccessMode::DynamicFallback),
                                Some(buffer_access_materialization_reason(ctx, object)),
                                false,
                                false,
                                vec!["typed_array_fallback=untracked_or_unproven".to_string()],
                            );
                            attach_buffer_view_pointer_state_for_expr(ctx, object);
                            Ok(result)
                        },
                    );
                }

                // Stores fall back for untracked views, unknown bounds, unsafe
                // conversions, and Uint8ClampedArray's ToUint8Clamp semantics.
                return rooting::with_operands_rooted(ctx, &[object, index, value], |ctx, vals| {
                    let blk = ctx.block();
                    let arr_bits = blk.bitcast_double_to_i64(&vals[0]);
                    let arr_i64 = blk.and(I64, &arr_bits, POINTER_MASK_I64);
                    let idx_i32 = blk.fptosi(DOUBLE, &vals[1], I32);
                    blk.call_void(
                        "js_typed_array_set",
                        &[(I64, &arr_i64), (I32, &idx_i32), (DOUBLE, &vals[2])],
                    );
                    let slow = LoweredValue::js_value(vals[2].clone());
                    ctx.record_lowered_value_with_access_mode(
                        "TypedArraySet",
                        None,
                        "TypedArraySet.slow_path",
                        &slow,
                        Some(BoundsState::Unknown),
                        None,
                        Some(BufferAccessMode::DynamicFallback),
                        Some(buffer_access_materialization_reason(ctx, object)),
                        false,
                        false,
                        vec!["typed_array_fallback=untracked_or_unproven".to_string()],
                    );
                    attach_buffer_view_pointer_state_for_expr(ctx, object);
                    Ok(vals[2].clone())
                });
            }
            if is_uint8array_receiver(ctx, object) && is_numeric_expr(ctx, index) {
                if let Some(store) = lower_buffer_store(
                    ctx,
                    object,
                    index,
                    value,
                    BufferAccessSpec::uint8array_set(),
                )? {
                    if value_discarded {
                        return Ok(double_literal(0.0));
                    }
                    return Ok(materialize_js_value(
                        ctx,
                        store.result,
                        MaterializationReason::FunctionAbi,
                    ));
                }
                if typed_array_index_needs_runtime_key(ctx, object.as_ref(), index.as_ref()) {
                    return rooting::with_operands_rooted(
                        ctx,
                        &[object, index, value],
                        |ctx, vals| {
                            let blk = ctx.block();
                            let arr_bits = blk.bitcast_double_to_i64(&vals[0]);
                            let arr_i64 = blk.and(I64, &arr_bits, POINTER_MASK_I64);
                            Ok(blk.call(
                                DOUBLE,
                                "js_typed_array_index_set_dynamic",
                                &[(I64, &arr_i64), (DOUBLE, &vals[1]), (DOUBLE, &vals[2])],
                            ))
                        },
                    );
                }
            }
            // #5525: when the receiver's static type is genuinely unknown
            // (`Type::Any`/`Type::Unknown`) and the index is numeric, route the
            // write through `js_dyn_index_set` — the exact symmetric counterpart
            // of the IndexGet `recv_unknown` arm (index_get.rs), which routes
            // reads through `js_dyn_index_get`. Both helpers carry the #5525
            // process-global typed-array kind cache + inline `typed_array_fast_
            // index_{get,set}` fast path, so a hot monomorphic `S[i]`/`P[i] = v`
            // on an `Int32Array` reaching a function through an untyped
            // `Array.<number>` parameter (bcryptjs's Blowfish P/S boxes) lands on
            // a cached load/store instead of the polymorphic feedback helper's
            // thread-local registry dispatch (`typed_array_owner_*` →
            // `_tlv_get_addr`). Pre-fix this fell all the way through to
            // `js_typed_feedback_object_set_index_polymorphic`, whose
            // `typed_array_set_numeric_index` path dominated the bcrypt profile.
            // The gate is narrow (only Any/Unknown receiver + numeric index) so
            // every statically-typed array / typed-array / object fast path below
            // is preserved.
            let recv_ty = crate::type_analysis::static_type_of(ctx, object);
            let recv_unknown = matches!(
                recv_ty,
                None | Some(perry_hir::types::Type::Any) | Some(perry_hir::types::Type::Unknown)
            );
            // The index may be numeric, a runtime string, or (rarely) a runtime
            // symbol — `js_dyn_index_set` triages all three. We only keep the
            // statically-known string-literal / symbol keys on their dedicated
            // (interned-handle / symbol-side-table) routes below; everything else
            // on an unknown receiver goes through the cached fast path. bcryptjs's
            // `lr[off]`/`lr[off + 1]` writes have an `off` param typed `any`, so
            // `off + 1` is NOT provably numeric — gating on `is_numeric_expr`
            // (the original #5525 attempt) missed exactly those ~4M hot writes
            // and they kept falling through to `js_put_value_set`.
            let index_is_static_string_or_symbol = matches!(
                index.as_ref(),
                Expr::String(_) | Expr::WtfString(_) | Expr::SymbolFor(_)
            ) || is_string_expr(ctx, index);
            if recv_unknown && !index_is_static_string_or_symbol {
                return rooting::with_operands_rooted_across(
                    ctx,
                    &[object, index],
                    &[value],
                    |ctx| {
                        // Keep the RHS on the js_value_bits evidence contract even
                        // on the #5525 inline typed-array route — the slow edge
                        // hands the boxed value to `js_dyn_index_set` unchanged.
                        lower_value_for_dynamic_index_set(
                            ctx,
                            value,
                            "index_set.dynamic_value_bits",
                            "polymorphic_index_set_helper_edge",
                        )
                    },
                    |ctx, vals, (val_double, _val_bits)| {
                        // #5525 follow-up: guarded inline typed-array element STORE
                        // at the access site, falling back to `js_dyn_index_set` on
                        // any guard miss. #7640: both the receiver and key are
                        // re-read after the allocating RHS.
                        Ok(lower_inline_dyn_typed_array_set(
                            ctx,
                            &vals[0],
                            &vals[1],
                            &val_double,
                        ))
                    },
                );
            }
            // Issue #637 / hono r2 followup: `arr[stringKey] = val` where
            // the index is statically string-typed (e.g. `for (const i in
            // sparseArr)` produces string i; then `out[i] = val`). Pre-fix
            // the array fast path below ran `fptosi(double, i32)` on the
            // NaN-boxed string, producing garbage indices that collapsed
            // every iteration's write onto slot 0. Route to the runtime
            // helper which parses the string as an integer and dispatches
            // to `js_array_set_f64_extend`, falling back to object-property
            // set on non-numeric keys per spec.
            if is_array_expr(ctx, object) && is_string_expr(ctx, index) {
                // #7638: neither operand was guarded. The receiver AND the key
                // are lowered before the value, and the key is a heap string by
                // construction on this arm (`is_string_expr`) with no
                // registered root of its own — `unbox_str_handle` below would
                // hand the setter a pre-move `StringHeader*`. `for (const i in
                // src) out[i] = mk();` is the shape, which is the very pattern
                // the arm was added for (#637).
                let value_needs_barrier = array_store_needs_write_barrier(ctx, value);
                return rooting::with_operands_rooted_across(
                    ctx,
                    &[object.as_ref(), index.as_ref()],
                    &[value.as_ref()],
                    |ctx| lower_value_for_optional_barrier(ctx, value, value_needs_barrier),
                    |ctx, vals, (val_double, val_bits)| {
                        let (arr_box, key_box) = (vals[0].clone(), vals[1].clone());
                        let (arr_handle, key_handle) = {
                            let blk = ctx.block();
                            // #7640 section D: key first (see the globalThis arm).
                            let key_handle = unbox_str_handle(blk, &key_box);
                            let arr_handle = unbox_to_i64(blk, &arr_box);
                            (arr_handle, key_handle)
                        };
                        let site_id = emit_typed_feedback_register_site(
                            ctx,
                            TypedFeedbackKind::ArrayElement,
                            "array[string_index]",
                            TypedFeedbackContract::array_set_string_key(),
                        );
                        ctx.block().call(
                            I64,
                            "js_typed_feedback_array_set_string_key",
                            &[
                                (I64, &site_id),
                                (I64, &arr_handle),
                                (I64, &key_handle),
                                (DOUBLE, &val_double),
                            ],
                        );
                        if value_needs_barrier {
                            let arr_bits = ctx.block().bitcast_double_to_i64(&arr_box);
                            let val_bits = val_bits
                                .unwrap_or_else(|| ctx.block().bitcast_double_to_i64(&val_double));
                            emit_write_barrier(ctx, &arr_bits, &val_bits);
                        }
                        Ok(val_double)
                    },
                );
            }
            // Issue #637 followup: `arr[k] = X` where receiver is array
            // but index is dynamically-typed (Any) — most commonly a
            // forEach callback's `(item, k)` parameter where `k` could
            // be a string (for-in over object keys, replace callback
            // capture-group params, etc.). The array fast-path's
            // `fptosi(double, i32)` collapses NaN-boxed strings to slot 0.
            // Route to a runtime helper that detects the tag at runtime:
            // string → parse + array-extend; numeric → fptosi + extend.
            // Only fires when index isn't statically numeric (otherwise
            // the existing fast path is correct and avoids the runtime
            // dispatch overhead).
            if is_array_expr(ctx, object) && !is_numeric_expr(ctx, index) {
                // #7341: receiver live across `index` and `value` lowering.
                //
                // #7638: the KEY was live across `value` too, and was not
                // rooted. This arm fires exactly when the index is NOT
                // statically numeric — a `forEach` callback's `(item, k)` where
                // `k` came from a for-in over object keys, say — so `idx_double`
                // is routinely a NaN-boxed heap string, and
                // `js_typed_feedback_array_set_index_or_string` parses it as a
                // key. An evacuating minor inside the RHS therefore left the
                // store reading a from-space `StringHeader*` and the element
                // landed under a garbage key. The dynamic-string-key arm below
                // has guarded exactly this since #7154; this one was missed
                // because its key is not *statically* a string.
                //
                // One operand group derives both windows rather than restating
                // either: the receiver's spans `index` and `value`, the key's
                // spans `value`, and `value` is lowered last so it takes
                // nothing.
                let value_needs_barrier = array_store_needs_write_barrier(ctx, value);
                return rooting::with_operands_rooted(ctx, &[object, index, value], |ctx, vals| {
                    let (arr_box, idx_double, val_double) = (&vals[0], &vals[1], &vals[2]);
                    let arr_handle = {
                        let blk = ctx.block();
                        unbox_to_i64(blk, arr_box)
                    };
                    let site_id = emit_typed_feedback_register_site(
                        ctx,
                        TypedFeedbackKind::ArrayElement,
                        "array[dynamic_index]",
                        TypedFeedbackContract::array_set_index_or_string(),
                    );
                    ctx.block().call(
                        I64,
                        "js_typed_feedback_array_set_index_or_string",
                        &[
                            (I64, &site_id),
                            (I64, &arr_handle),
                            (DOUBLE, idx_double),
                            (DOUBLE, val_double),
                        ],
                    );
                    if value_needs_barrier {
                        let val_bits = ctx.block().bitcast_double_to_i64(val_double);
                        let arr_bits = ctx.block().bitcast_double_to_i64(arr_box);
                        emit_write_barrier(ctx, &arr_bits, &val_bits);
                    }
                    Ok(val_double.clone())
                });
            }
            if is_array_expr(ctx, object)
                && numeric_index_needs_runtime_key(ctx, object.as_ref(), index.as_ref())
            {
                return lower_array_index_set_via_runtime_key(
                    ctx,
                    object.as_ref(),
                    index.as_ref(),
                    value.as_ref(),
                    "array[dynamic_numeric_index]",
                );
            }
            // Same dispatch tree as IndexGet: known array → fast inline,
            // string key on dynamic receiver → object field set, otherwise
            // bail with a clear error.
            if is_array_expr(ctx, object) {
                // Repsel Phase 4a.3: guard-free `Ptr<NumArray>` store — the
                // local proof (raw-f64-or-hole slots forever, length never
                // shrinks, binding never stale) + a per-site in-bounds proof
                // + a canonical-raw-f64 RHS lower `a[i] = v` to slot reload →
                // mask → gep → `store double`, with no guard tier, no bounds
                // arms, no barrier, no note, no length bump (in-bounds ⇒
                // length unchanged). Anything unproven falls to the guarded
                // tiers below, which maintain the same invariants.
                if let Some(value) = super::ptr_numarray_access::try_lower_num_array_guard_free_set(
                    ctx,
                    object.as_ref(),
                    index,
                    value,
                )? {
                    return Ok(value);
                }
                // Bounded-index fast-fast path: when the surrounding
                // for-loop has registered `(counter_id, arr_id)` as a
                // bounded pair (via `lower_for`'s
                // `classify_for_length_hoist` analysis) and this
                // IndexSet matches it, we can skip the bound check +
                // capacity check + realloc fallback entirely. The
                // for-loop already proved `i < arr.length` and the
                // body provably can't change `arr.length`, so the
                // IndexSet at `arr[i]` is statically inbounds.
                if let Expr::LocalGet(arr_id) = object.as_ref() {
                    if let Some((fact, idx_id, offset)) =
                        packed_f64_loop_fact_for_index(ctx, *arr_id, index.as_ref())
                    {
                        // Packed-U32 typed-slot stores are not implemented; rather
                        // than abort codegen, let U32 facts fall through to the
                        // generic/bounded array-store path below (correct, just
                        // not the packed fast path).
                        if !matches!(fact.array_kind, PackedNumericLoopKind::U32) {
                            if let Some(i32_slot) = ctx.i32_counter_slots.get(&idx_id).cloned() {
                                let idx_i32 = load_packed_loop_index_i32(ctx, &i32_slot, offset);
                                return lower_packed_numeric_loop_index_set(
                                    ctx,
                                    *arr_id,
                                    &idx_i32,
                                    value.as_ref(),
                                    &fact.guard_id,
                                    &fact.store_side_exit_label,
                                    fact.array_kind,
                                    fact.allow_holes,
                                );
                            }
                        }
                    }
                }
                if let (Expr::LocalGet(arr_id), Expr::LocalGet(idx_id)) =
                    (object.as_ref(), index.as_ref())
                {
                    if ctx.bounded_index_pairs.iter().any(|fact| {
                        fact.index_local_id == *idx_id && fact.array_local_id == *arr_id
                    }) {
                        let Some(i32_slot) = ctx.i32_counter_slots.get(idx_id).cloned() else {
                            return lower_array_index_set_via_runtime_key(
                                ctx,
                                object.as_ref(),
                                index.as_ref(),
                                value.as_ref(),
                                "array[dynamic_numeric_index]",
                            );
                        };
                        let layout_note_needed = array_store_needs_layout_note(ctx, object, value);
                        let write_barrier_needed = array_store_needs_write_barrier(ctx, value);
                        let value_is_numeric = is_numeric_expr(ctx, value);
                        let require_numeric_layout =
                            guarded_numeric_array_store_candidate(ctx, value)
                                && expr_has_numeric_pointer_free_array_layout(ctx, object);
                        // #7640 section A: the receiver was lowered, then the
                        // index, then — the hazard — the VALUE, with no
                        // rooting decision at all. `classify_for_length_hoist`'s
                        // body predicate accepts an allocating RHS
                        // (`Expr::Object` / `Array` / `ArraySpread`) at exactly
                        // this call site, so `for (let i=0;i<a.length;i++) a[i]
                        // = {v:i};` left `arr_box` a bare SSA register across
                        // the object literal's allocation — sharpened by
                        // sitting immediately above the generic array-store arm
                        // #7341 already closed. Root [object, index, value] in
                        // one group; `idx_i32` is a plain loop-counter i32 read
                        // (never GC-managed) and stays exactly where it was —
                        // between the index and value lowerings — so the
                        // counter's observed value is unchanged even if `value`
                        // has side effects.
                        return rooting::with_rooted_group(ctx, 3, |ctx, group| {
                            let recv_idx = group.lower(ctx, object, true)?;
                            let index_idx = group.lower(ctx, index, true)?;
                            let idx_i32 = ctx.block().load(I32, &i32_slot);
                            let value_idx = group.lower(ctx, value, true)?;
                            let arr_box = group.reread(ctx, recv_idx)?;
                            let idx_double = group.reread(ctx, index_idx)?;
                            let val_double = group.reread(ctx, value_idx)?;
                            if require_numeric_layout {
                                let feedback_site_id = emit_typed_feedback_register_site(
                                    ctx,
                                    TypedFeedbackKind::ArrayElement,
                                    "array[index]=",
                                    TypedFeedbackContract::numeric_array_set_index(),
                                );
                                let fast_idx = ctx.new_block("idxset.bounded_numeric_fast");
                                let fallback_idx = ctx.new_block("idxset.bounded_numeric_fallback");
                                let merge_idx = ctx.new_block("idxset.bounded_numeric_merge");
                                let fast_label = ctx.block_label(fast_idx);
                                let fallback_label = ctx.block_label(fallback_idx);
                                let merge_label = ctx.block_label(merge_idx);

                                let guard_ok = {
                                    let blk = ctx.block();
                                    let guard_i32 = blk.call(
                                        I32,
                                        "js_typed_feedback_numeric_array_index_set_guard",
                                        &[
                                            (I64, &feedback_site_id),
                                            (DOUBLE, &arr_box),
                                            (I32, &idx_i32),
                                            (DOUBLE, &val_double),
                                            (I32, "1"),
                                        ],
                                    );
                                    blk.icmp_ne(I32, &guard_i32, "0")
                                };
                                ctx.block().cond_br(&guard_ok, &fast_label, &fallback_label);

                                ctx.current_block = fallback_idx;
                                {
                                    let fallback_box = ctx.block().call(
                                        DOUBLE,
                                        "js_typed_feedback_array_index_set_fallback_boxed",
                                        &[
                                            (I64, &feedback_site_id),
                                            (DOUBLE, &arr_box),
                                            (DOUBLE, &idx_double),
                                            (DOUBLE, &val_double),
                                        ],
                                    );
                                    if let Some(slot) = ctx.locals.get(arr_id).cloned() {
                                        ctx.block().store(DOUBLE, &fallback_box, &slot);
                                    }
                                    ctx.block().br(&merge_label);
                                    let fallback = LoweredValue {
                                        semantic: SemanticKind::JsValue,
                                        rep: NativeRep::JsValue,
                                        llvm_ty: DOUBLE,
                                        value: fallback_box,
                                    };
                                    ctx.record_lowered_value_with_access_mode_and_facts(
                                        "NumericArrayIndexSet",
                                        Some(*arr_id),
                                        "js_typed_feedback_array_index_set_fallback_boxed",
                                        &fallback,
                                        Some(BoundsState::Unknown),
                                        None,
                                        Some(BufferAccessMode::DynamicFallback),
                                        Some(MaterializationReason::RuntimeApi),
                                        None,
                                        None,
                                        Vec::new(),
                                        vec![
                                            raw_f64_layout_fact(
                                                Some(*arr_id),
                                                "rejected",
                                                "numeric_array_index_set_guard",
                                                Some(MaterializationReason::RuntimeApi),
                                            ),
                                            raw_f64_layout_fact(
                                                Some(*arr_id),
                                                "invalidated",
                                                "runtime_api",
                                                Some(MaterializationReason::RuntimeApi),
                                            ),
                                        ],
                                        false,
                                        false,
                                        Vec::new(),
                                    );
                                }

                                ctx.current_block = fast_idx;
                                let value_is_canonical_raw_f64 =
                                    crate::type_analysis::expr_produces_canonical_raw_f64(
                                        ctx, value,
                                    );
                                {
                                    let blk = ctx.block();
                                    let arr_bits = blk.bitcast_double_to_i64(&arr_box);
                                    let arr_handle = blk.and(I64, &arr_bits, POINTER_MASK_I64);
                                    // The numeric-array set guard above was called with
                                    // `in_bounds=true`, so it has already proved a live,
                                    // non-forwarded plain Array in raw-f64 layout, a numeric
                                    // RHS, and an in-bounds index. Store the f64 slot inline
                                    // instead of calling the helper that re-validates the same
                                    // facts before doing this store.
                                    let idx_i64 = blk.zext(I32, &idx_i32, I64);
                                    let byte_offset = blk.shl(I64, &idx_i64, "3");
                                    let with_header = blk.add(I64, &byte_offset, "8");
                                    let element_addr = blk.add(I64, &arr_handle, &with_header);
                                    let element_ptr = blk.inttoptr(I64, &element_addr);
                                    // GC_STORE_AUDIT(POINTER_FREE): guarded raw-f64
                                    // numeric store — the (canonical) value is a
                                    // plain f64, never a GC pointer, so no barrier.
                                    if value_is_canonical_raw_f64 {
                                        // Repsel 4a.0: canonical by construction —
                                        // skip js_array_numeric_value_to_raw_f64.
                                        blk.store(DOUBLE, &val_double, &element_ptr);
                                    } else {
                                        let numeric_value =
                                            canonicalize_raw_f64_numeric_store_value(
                                                blk,
                                                &val_double,
                                            );
                                        // GC_STORE_AUDIT(POINTER_FREE): the
                                        // canonicalizer returns a plain unboxed
                                        // f64, never a GC pointer — no barrier.
                                        blk.store(DOUBLE, &numeric_value, &element_ptr);
                                    }
                                    blk.br(&merge_label);
                                }
                                let stored = LoweredValue {
                                    semantic: SemanticKind::JsNumber,
                                    rep: NativeRep::F64,
                                    llvm_ty: DOUBLE,
                                    value: val_double.clone(),
                                };
                                ctx.record_lowered_value_with_access_mode_and_facts(
                                    "NumericArrayIndexSet",
                                    Some(*arr_id),
                                    "js_array_numeric_set_f64_unboxed",
                                    &stored,
                                    Some(BoundsState::Guarded {
                                        guard_id: "numeric_array_index_set_guard".to_string(),
                                    }),
                                    None,
                                    Some(BufferAccessMode::CheckedNative),
                                    None,
                                    None,
                                    None,
                                    vec![raw_f64_layout_fact(
                                        Some(*arr_id),
                                        "consumed",
                                        "numeric_array_index_set_guard",
                                        None,
                                    )],
                                    Vec::new(),
                                    false,
                                    false,
                                    Vec::new(),
                                );

                                ctx.current_block = merge_idx;
                                return Ok(val_double);
                            }
                            let blk = ctx.block();
                            let arr_bits = blk.bitcast_double_to_i64(&arr_box);
                            let arr_handle = blk.and(I64, &arr_bits, POINTER_MASK_I64);
                            // ptr = arr_handle + 8 + idx*8
                            let idx_i64 = blk.zext(I32, &idx_i32, I64);
                            let byte_offset = blk.shl(I64, &idx_i64, "3");
                            let with_header = blk.add(I64, &byte_offset, "8");
                            let element_addr = blk.add(I64, &arr_handle, &with_header);
                            let element_ptr = blk.inttoptr(I64, &element_addr);
                            let value_bits = emit_jsvalue_slot_store_on_block(
                                blk,
                                &element_ptr,
                                &val_double,
                                &arr_handle,
                                &idx_i32,
                                layout_note_needed,
                                &arr_handle,
                                &element_addr,
                                write_barrier_needed,
                            );
                            if !value_is_numeric {
                                let value_bits = value_bits
                                    .unwrap_or_else(|| blk.bitcast_double_to_i64(&val_double));
                                emit_array_numeric_write_note_on_block(
                                    blk,
                                    &arr_handle,
                                    &value_bits,
                                );
                            }
                            Ok(val_double)
                        });
                    }
                }

                let layout_note_needed = array_store_needs_layout_note(ctx, object, value);
                let write_barrier_needed = array_store_needs_write_barrier(ctx, value);
                let value_is_numeric = is_numeric_expr(ctx, value);
                let require_numeric_layout = guarded_numeric_array_store_candidate(ctx, value)
                    && expr_has_numeric_pointer_free_array_layout(ctx, object);
                let local_id = if let Expr::LocalGet(id) = object.as_ref() {
                    Some(*id)
                } else {
                    None
                };
                // #7341: this array path skipped the store-operand guard the
                // generic object paths below already apply (#7154). `a[i] = v`
                // evaluates the receiver first and the value last — spec order
                // — so the receiver sits in an SSA register while an allocating
                // RHS runs. The *slot* it was loaded from is a registered root
                // and evacuation rewrites it; the register is not, so the store
                // lands in retired from-space.
                //
                // Reproduced deterministically (6/6) by a module-level array
                // written in a loop from inside a function:
                //
                //     const sink: unknown[] = new Array(1024);
                //     function churn(n) { for (...) sink[i & 1023] = { a: i }; }
                //
                // The same loop at top level is clean, and a *local* array is
                // clean, which is why this went unnoticed. Both root backends
                // fault identically — the value was never given a root slot at
                // all, so neither lowering could have covered it.
                //
                // The window is the disjunction over `index` and `value`
                // (#7201): the receiver is live across both. The operand group
                // derives that from the list order; the index is numeric on
                // this path, so it takes no slot and the IR is unchanged.
                return rooting::with_operands_rooted(ctx, &[object, index, value], |ctx, vals| {
                    let (arr_box, idx_double, val_double) =
                        (vals[0].clone(), vals[1].clone(), vals[2].clone());
                    let feedback_site_id = emit_typed_feedback_register_site(
                        ctx,
                        TypedFeedbackKind::ArrayElement,
                        "array[index]=",
                        if require_numeric_layout {
                            TypedFeedbackContract::numeric_array_set_index()
                        } else {
                            TypedFeedbackContract::array_set_index()
                        },
                    );
                    // Use the fast inlined IndexSet path only when the
                    // receiver is a local that's actually in ctx.locals
                    // (stack slot). Module-level arrays accessed from inside
                    // a function are in ctx.module_globals instead — for
                    // those we use js_array_set_f64_extend (the realloc-
                    // capable variant) and write the new pointer back to
                    // the global slot. Issue #221: the previous code
                    // funneled module globals through js_array_set_f64
                    // which returns silently when `index >= length` — so
                    // every `arr[i] = v` against a `const A: T[] = []`
                    // declared empty was a silent no-op, both the value
                    // and the implicit length update vanishing.
                    if let Some(id) = local_id {
                        if ctx.locals.contains_key(&id) {
                            let value_is_canonical_raw_f64 =
                                crate::type_analysis::expr_produces_canonical_raw_f64(ctx, value);
                            lower_index_set_fast(
                                ctx,
                                &arr_box,
                                &idx_double,
                                &val_double,
                                id,
                                layout_note_needed,
                                write_barrier_needed,
                                value_is_numeric,
                                require_numeric_layout,
                                value_is_canonical_raw_f64,
                                &feedback_site_id,
                            )?;
                        } else if let Some(global_name) = ctx.module_globals.get(&id).cloned() {
                            let blk = ctx.block();
                            let arr_bits = blk.bitcast_double_to_i64(&arr_box);
                            let arr_handle = blk.and(I64, &arr_bits, POINTER_MASK_I64);
                            let idx_i32 = blk.fptosi(DOUBLE, &idx_double, I32);
                            let new_handle = blk.call(
                                I64,
                                "js_typed_feedback_array_set_f64_extend",
                                &[
                                    (I64, &feedback_site_id),
                                    (I64, &arr_handle),
                                    (I32, &idx_i32),
                                    (DOUBLE, &val_double),
                                ],
                            );
                            let new_box = nanbox_pointer_inline(blk, &new_handle);
                            let g_ref = format!("@{}", global_name);
                            // GC_STORE_AUDIT(ROOT): module global array slot is a registered mutable GC root.
                            emit_root_nanbox_store_on_block(ctx.block(), &new_box, &g_ref);
                            // Gen-GC Phase C2: write barrier on array element store.
                            if write_barrier_needed {
                                let val_bits = ctx.block().bitcast_double_to_i64(&val_double);
                                emit_write_barrier(ctx, &arr_bits, &val_bits);
                            }
                        } else {
                            // Closure-captured array, or local without a
                            // stack slot (rare). Issue #637 followup / hono r2:
                            // pre-fix this called `js_array_set_f64` (non-
                            // extending), which silently returned when `index
                            // >= length` (matching `js_array_set_f64`'s in-
                            // bounds gate at array.rs:571). For an empty
                            // captured array (common pattern: closure body
                            // does `arr[++i] = X` to populate from outer
                            // scope), this dropped every write. Switch to
                            // `js_array_set_f64_extend` — the forwarding-
                            // pointer mechanism (issue #233) handles realloc
                            // visibility for the caller, so we don't need a
                            // writeback target here. Discard the returned
                            // pointer; downstream reads via clean_arr_ptr
                            // follow the forwarding chain to the new head.
                            let blk = ctx.block();
                            let arr_bits = blk.bitcast_double_to_i64(&arr_box);
                            let arr_handle = blk.and(I64, &arr_bits, POINTER_MASK_I64);
                            let idx_i32 = blk.fptosi(DOUBLE, &idx_double, I32);
                            blk.call(
                                I64,
                                "js_typed_feedback_array_set_f64_extend",
                                &[
                                    (I64, &feedback_site_id),
                                    (I64, &arr_handle),
                                    (I32, &idx_i32),
                                    (DOUBLE, &val_double),
                                ],
                            );
                            // Gen-GC Phase C2: write barrier on array element store.
                            if write_barrier_needed {
                                let val_bits = ctx.block().bitcast_double_to_i64(&val_double);
                                emit_write_barrier(ctx, &arr_bits, &val_bits);
                            }
                        }
                    } else {
                        let idx_i32 = {
                            let blk = ctx.block();
                            blk.fptosi(DOUBLE, &idx_double, I32)
                        };
                        // The receiver is not a stack local (`this.vals[i] = v`,
                        // `obj.arr[i] = v`), so `lower_index_set_fast` cannot
                        // serve it — it needs a slot to write a realloc'd head
                        // back to. A STRICTLY in-bounds store changes no head
                        // and no length, so it needs no writeback and can be
                        // inlined here; everything else still goes to the
                        // extend helper below. Feedback-emission builds keep
                        // the out-of-line call so observation stays complete.
                        if !super::typed_feedback_emission_enabled() {
                            let arr_box = arr_box.clone();
                            let val_double = val_double.clone();
                            let idx_i32c = idx_i32.clone();
                            let feedback_site_id = feedback_site_id.clone();
                            super::index_set_guarded::emit_guarded_inbounds_array_store(
                                ctx,
                                &arr_box,
                                &idx_i32,
                                &val_double,
                                "idxset.recv_prop",
                                layout_note_needed,
                                write_barrier_needed,
                                value_is_numeric,
                                |ctx| {
                                    let blk = ctx.block();
                                    let arr_bits = blk.bitcast_double_to_i64(&arr_box);
                                    let arr_handle = blk.and(I64, &arr_bits, POINTER_MASK_I64);
                                    blk.call(
                                        I64,
                                        "js_typed_feedback_array_set_f64_extend",
                                        &[
                                            (I64, &feedback_site_id),
                                            (I64, &arr_handle),
                                            (I32, &idx_i32c),
                                            (DOUBLE, &val_double),
                                        ],
                                    );
                                    if write_barrier_needed {
                                        let val_bits =
                                            ctx.block().bitcast_double_to_i64(&val_double);
                                        emit_write_barrier(ctx, &arr_bits, &val_bits);
                                    }
                                    Ok(())
                                },
                            )?;
                            return Ok(val_double);
                        }
                        let blk = ctx.block();
                        let arr_bits = blk.bitcast_double_to_i64(&arr_box);
                        let arr_handle = blk.and(I64, &arr_bits, POINTER_MASK_I64);
                        // Issue #637 followup / hono r2: use the extend variant
                        // so `arr[i] = X` for i >= length grows the array per
                        // JS spec, instead of silently no-op'ing (which the
                        // non-extend `js_array_set_f64` did via `if index >=
                        // length { return; }`). The hono Trie's
                        // `indexReplacementMap[++captureIndex] = N` pattern
                        // (sparse-extend from a closure capturing the array)
                        // was the load-bearing site — pre-fix the array stayed
                        // length 0 inside the closure, so `for (const i in
                        // indexReplacementMap)` outside the closure iterated
                        // zero times and `handlerMap` ended up empty.
                        blk.call(
                            I64,
                            "js_typed_feedback_array_set_f64_extend",
                            &[
                                (I64, &feedback_site_id),
                                (I64, &arr_handle),
                                (I32, &idx_i32),
                                (DOUBLE, &val_double),
                            ],
                        );
                        // Gen-GC Phase C2: write barrier on array element store.
                        if write_barrier_needed {
                            let val_bits = ctx.block().bitcast_double_to_i64(&val_double);
                            emit_write_barrier(ctx, &arr_bits, &val_bits);
                        }
                    }
                    // The group is released after `body` returns, never before:
                    // every branch above ends in a helper that can itself allocate
                    // (element-array growth, the extend path's realloc).
                    Ok(val_double)
                });
            }
            if let Expr::String(literal) = index.as_ref() {
                // #7154: the value expression can collect, and an evacuating
                // minor inside it relocates the receiver out from under
                // `obj_box`. Root it across the evaluation and re-read below.
                // The key is a literal, so it is not an operand here at all —
                // it is interned into the string pool below.
                return rooting::with_operands_rooted_across(
                    ctx,
                    &[object.as_ref()],
                    &[value.as_ref()],
                    |ctx| {
                        lower_value_for_dynamic_index_set(
                            ctx,
                            value,
                            "index_set.literal_string_value_bits",
                            "literal_string_index_set_helper_edge",
                        )
                    },
                    |ctx, vals, (val_double, _val_bits)| {
                        let obj_box = vals[0].clone();
                        let key_idx = ctx.strings.intern(literal);
                        let key_handle_global =
                            format!("@{}", ctx.strings.entry(key_idx).handle_global);
                        let obj_bits = ctx.block().bitcast_double_to_i64(&obj_box);
                        super::property_set::emit_nullish_write_guard(
                            ctx,
                            &obj_bits,
                            literal,
                            "iset.literal",
                        );
                        let static_classref = super::index_get::index_object_is_class_or_proto_ref(
                            ctx,
                            object.as_ref(),
                        );
                        let (obj_handle, key_raw) = {
                            let blk = ctx.block();
                            let obj_handle = super::index_get::classref_preserving_handle(
                                blk,
                                &obj_bits,
                                static_classref,
                            );
                            let key_box = blk.load(DOUBLE, &key_handle_global);
                            let key_bits = blk.bitcast_double_to_i64(&key_box);
                            let key_raw = blk.and(I64, &key_bits, POINTER_MASK_I64);
                            (obj_handle, key_raw)
                        };
                        let site_id = emit_typed_feedback_register_site(
                            ctx,
                            TypedFeedbackKind::PropertySet,
                            literal,
                            TypedFeedbackContract::object_set_by_name(),
                        );
                        ctx.block().call_void(
                            "js_typed_feedback_object_set_field_by_name",
                            &[
                                (I64, &site_id),
                                (I64, &obj_handle),
                                (I64, &key_raw),
                                (DOUBLE, &val_double),
                            ],
                        );
                        Ok(val_double)
                    },
                );
            }
            if is_string_expr(ctx, index) {
                // #7154: see the literal-key arm above, plus the KEY, which
                // sits in the same window. A non-literal string key is an
                // ordinary heap string with no registered root of its own, so
                // an evacuating minor inside the value's evaluation relocates
                // it and leaves the register naming from-space —
                // `unbox_str_handle` below would hand the setter a pre-move
                // `StringHeader*` and the field would land under a garbage key.
                //
                // #7639: the receiver's window used to be derived from `value`
                // ALONE, which is the half-measure #7201 named. The receiver is
                // lowered before the KEY as well, so `o[f()] = 1` — a literal
                // RHS that cannot collect, an allocating key that can — left it
                // unguarded. As one operand group the receiver's window is the
                // disjunction over everything after it, which is what #7201
                // established and what `guard_store_operand`'s two-argument
                // form structurally could not say.
                return rooting::with_operands_rooted_across(
                    ctx,
                    &[object.as_ref(), index.as_ref()],
                    &[value.as_ref()],
                    |ctx| {
                        lower_value_for_dynamic_index_set(
                            ctx,
                            value,
                            "index_set.string_value_bits",
                            "string_index_set_helper_edge",
                        )
                    },
                    |ctx, vals, (val_double, _val_bits)| {
                        let (obj_box, key_box) = (vals[0].clone(), vals[1].clone());
                        let obj_bits = ctx.block().bitcast_double_to_i64(&obj_box);
                        super::property_set::emit_nullish_write_guard(
                            ctx,
                            &obj_bits,
                            "index",
                            "iset.string",
                        );
                        let static_classref = super::index_get::index_object_is_class_or_proto_ref(
                            ctx,
                            object.as_ref(),
                        );
                        let (obj_handle, key_handle) = {
                            let blk = ctx.block();
                            // #7640 section D: the SSO-safe key unbox can
                            // allocate, so it goes FIRST — `obj_bits` is a
                            // NaN-boxed double the group re-read, but
                            // `obj_handle` is a raw `i64` no root can name.
                            let key_handle = unbox_str_handle(blk, &key_box);
                            let obj_handle = super::index_get::classref_preserving_handle(
                                blk,
                                &obj_bits,
                                static_classref,
                            );
                            (obj_handle, key_handle)
                        };
                        let site_id = emit_typed_feedback_register_site(
                            ctx,
                            TypedFeedbackKind::PropertySet,
                            "object[string_index]",
                            TypedFeedbackContract::object_set_by_name(),
                        );
                        ctx.block().call_void(
                            "js_typed_feedback_object_set_field_by_name",
                            &[
                                (I64, &site_id),
                                (I64, &obj_handle),
                                (I64, &key_handle),
                                (DOUBLE, &val_double),
                            ],
                        );
                        // One group, one release, below the store. The inner-to-outer
                        // ordering obligation the two hand-written guards carried —
                        // `temp_root_truncate` is a stack CUT, so releasing the
                        // receiver first silently dropped the key's slot as well — is
                        // gone rather than merely documented.
                        Ok(val_double)
                    },
                );
            }
            // Fallback with runtime STRING_TAG check, matching IndexGet.
            // Layout: first runtime-check whether the index is a Symbol
            // (POINTER_TAG with SYMBOL_MAGIC). If so, dispatch to the
            // symbol-property side table. Otherwise fall through to the
            // string/numeric dispatch.
            //
            // #7639: this arm had NO store-operand guard on either operand,
            // while every sibling arm above has had one since #7154. It is the
            // most exposed of them all — it is reached precisely when NOTHING
            // about the receiver or the key is statically known, so both are
            // ordinary heap values by default, and both are consumed after
            // `lower_value_for_dynamic_index_set` has lowered arbitrary user
            // code. `js_object_set_symbol_property` takes the receiver box
            // directly and the string arm masks the key to a `StringHeader*`,
            // so an evacuating minor in the RHS lands the write on a stale
            // object under a stale key. `m[k] = f()` on an `any`-typed `m`
            // reaches it.
            return rooting::with_operands_rooted_across(
                ctx,
                &[object.as_ref(), index.as_ref()],
                &[value.as_ref()],
                |ctx| {
                    lower_value_for_dynamic_index_set(
                        ctx,
                        value,
                        "index_set.dynamic_value_bits",
                        "polymorphic_index_set_helper_edge",
                    )
                },
                |ctx, vals, (val_double, _val_bits)| {
                    let (obj_box, idx_box) = (vals[0].clone(), vals[1].clone());
                    let obj_bits = ctx.block().bitcast_double_to_i64(&obj_box);
                    super::property_set::emit_nullish_write_guard(ctx, &obj_bits, "index", "iset");
                    let static_classref =
                        super::index_get::index_object_is_class_or_proto_ref(ctx, object.as_ref());
                    let obj_handle = {
                        let blk = ctx.block();
                        super::index_get::classref_preserving_handle(
                            blk,
                            &obj_bits,
                            static_classref,
                        )
                    };
                    let feedback_site_id = emit_typed_feedback_register_site(
                        ctx,
                        TypedFeedbackKind::ArrayElement,
                        "index_set",
                        TypedFeedbackContract::polymorphic_index_set(),
                    );
                    // Symbol check: js_is_symbol returns 1 if idx_box is a Symbol.
                    let is_sym_i32 = ctx.block().call(I32, "js_is_symbol", &[(DOUBLE, &idx_box)]);
                    let is_sym_bit = ctx.block().icmp_ne(I32, &is_sym_i32, "0");
                    let sym_set = ctx.new_block("iset.sym");
                    let nonsym_set = ctx.new_block("iset.nonsym");
                    let str_set = ctx.new_block("iset.str");
                    let num_set = ctx.new_block("iset.num");
                    let set_merge = ctx.new_block("iset.merge");
                    let sym_lbl = ctx.block_label(sym_set);
                    let nonsym_lbl = ctx.block_label(nonsym_set);
                    let str_lbl = ctx.block_label(str_set);
                    let num_lbl = ctx.block_label(num_set);
                    let merge_lbl = ctx.block_label(set_merge);
                    ctx.block().cond_br(&is_sym_bit, &sym_lbl, &nonsym_lbl);
                    // Symbol key → side-table set.
                    ctx.current_block = sym_set;
                    ctx.block().call(
                        DOUBLE,
                        "js_object_set_symbol_property",
                        &[
                            (DOUBLE, &obj_box),
                            (DOUBLE, &idx_box),
                            (DOUBLE, &val_double),
                        ],
                    );
                    ctx.block().br(&merge_lbl);
                    // Not a symbol — recompute idx_bits in this block (LLVM SSA, no
                    // dominance issue: each branch starts fresh).
                    ctx.current_block = nonsym_set;
                    let blk = ctx.block();
                    let idx_bits = blk.bitcast_double_to_i64(&idx_box);
                    let top16 = blk.lshr(I64, &idx_bits, "48");
                    // STRING_TAG (0x7FFF) heap pointer + SHORT_STRING_TAG (0x7FF9) SSO.
                    // See IndexGet path comment / issue #434 for the SSO rationale.
                    let is_str_tag_heap = blk.icmp_eq(I64, &top16, "32767");
                    let lower48 = blk.and(I64, &idx_bits, POINTER_MASK_I64);
                    let is_valid_ptr = blk.icmp_ugt(I64, &lower48, "4095");
                    let is_str_heap = blk.and(crate::types::I1, &is_str_tag_heap, &is_valid_ptr);
                    let is_str_tag_sso = blk.icmp_eq(I64, &top16, "32761");
                    let is_str = blk.or(crate::types::I1, &is_str_heap, &is_str_tag_sso);
                    ctx.block().cond_br(&is_str, &str_lbl, &num_lbl);
                    // String key → polymorphic helper that detects array receivers
                    // and parses numeric-string keys as array indices, falling
                    // through to `js_object_set_field_by_name` for Object/Closure
                    // receivers. Issue #637: pre-fix this called the object setter
                    // unconditionally, which silently no-op'd `arr[stringKey] = X`
                    // on captured arrays whose static type was lost across the
                    // closure boundary (forEach callbacks, replace callbacks, etc.).
                    ctx.current_block = str_set;
                    // #7640 section D, the cross-block half. `unbox_str_handle`
                    // can allocate (SSO materialisation), and the entry block's
                    // `obj_handle` is a RAW `i64` computed two conditional
                    // branches above it — nothing can name it across that
                    // allocation. Re-derive it here, below the key unbox.
                    // A distinct name, not a shadow: the NUMERIC sibling block
                    // below uses the entry block's `obj_handle`, which a
                    // definition in THIS block does not dominate.
                    let (key_handle, str_obj_handle) = {
                        let blk = ctx.block();
                        let key_handle = unbox_str_handle(blk, &idx_box);
                        let str_obj_handle = super::index_get::classref_preserving_handle(
                            blk,
                            &obj_bits,
                            static_classref,
                        );
                        (key_handle, str_obj_handle)
                    };
                    ctx.block().call(
                        I64,
                        "js_typed_feedback_array_set_string_key",
                        &[
                            (I64, &feedback_site_id),
                            (I64, &str_obj_handle),
                            (I64, &key_handle),
                            (DOUBLE, &val_double),
                        ],
                    );
                    ctx.block().br(&merge_lbl);
                    // Numeric key → polymorphic dispatch.
                    //
                    // Closes #471: the previous fallback emitted an inline
                    // `obj_handle + 8 + idx*8` store on the assumption that the
                    // receiver had an ArrayHeader (8-byte header) layout. That's
                    // a load-bearing assumption for `arr[i] = v` against an
                    // unknown-typed receiver where `is_array_expr` couldn't
                    // narrow it statically — but the header spans object_header_size_bytes(...) bytes, then inline slots, plus
                    // `max(field_count, 8)` inline slots, so writing at offset
                    // `8 + idx*8` for any `idx ≥ 7` overflows the object's
                    // allocation and corrupts the adjacent heap object. The
                    // @perryts/mongodb #471 repro hit this with `idMap[i] = …`
                    // (a `Record<number, unknown>`) and trampled the keys_array
                    // of an unrelated object that the BSON encoder later read
                    // as an empty doc, producing structurally-truncated wire data.
                    //
                    // Route through the runtime which checks the receiver's GC
                    // type and dispatches: arrays/buffers/typed-arrays through
                    // js_array_set_f64_extend (handles forwarding + per-kind
                    // stores), plain objects through stringify-the-index +
                    // js_object_set_field_by_name. The forwarding-chain handling
                    // that the previous code's inline-vs-fwd branch did is now
                    // inside js_array_set_f64_extend's clean_arr_ptr_mut.
                    ctx.current_block = num_set;
                    {
                        let blk = ctx.block();
                        blk.call_void(
                            "js_typed_feedback_object_set_index_polymorphic",
                            &[
                                (I64, &feedback_site_id),
                                (I64, &obj_handle),
                                (DOUBLE, &idx_box),
                                (DOUBLE, &val_double),
                            ],
                        );
                    }
                    ctx.block().br(&merge_lbl);
                    // The group is released after `body` returns, which lands the
                    // truncate in the merge block — below every arm's setter, each of
                    // which can run a user setter and therefore collect.
                    ctx.current_block = set_merge;
                    Ok(val_double)
                },
            );
        }

        // `obj.field = v` — generic object field write.
        _ => unreachable!("expr/mod.rs dispatched a variant not handled by this submodule"),
    }
}
