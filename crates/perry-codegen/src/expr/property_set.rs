//! PropertySet (obj.prop = v).
//!
//! Extracted from `expr/mod.rs` to keep that file under the 2000-line cap.
//! Pure mechanical move — match arm bodies are verbatim copies, called from
//! `lower_expr`'s outer dispatch.
//!
//! # Rooting (Layer 1, slice 4)
//!
//! Migrated onto [`crate::rooting`]; this module names no `expr::temp_root`
//! symbol. Every store here is the same shape — the receiver is lowered first
//! because `Set(O, k, v)` evaluates the reference before the value, so it sits
//! in an SSA register while arbitrary user code runs — and that shape is
//! exactly one call to [`crate::rooting::with_operands_rooted`] (the value is
//! lowered by `lower_expr`) or
//! [`crate::rooting::with_operands_rooted_across`] (the value is lowered by
//! `lower_value_for_dynamic_property_set`, which this API cannot produce).
//! The class-field family has one deliberate split: `LocalGet` / `This`
//! receivers keep their existing zero-cost `root_reload` repair, while a
//! compound receiver uses an operand group because that pass cannot rederive
//! its phi result (#7640 section C).
//!
//! The migration found one arm that had no guard at all: `arr.length = f()`
//! (#7637). It is the same #7154 window every sibling arm already closed, and
//! it is closed here by putting the receiver in an operand group rather than by
//! a fourth hand-written guard.

use anyhow::Result;
use perry_hir::Expr;

use crate::nanbox::POINTER_MASK_I64;
use crate::native_value::{
    BoundsState, BufferAccessMode, ExpectedNativeRep, LoweredValue, MaterializationReason,
    NativeRep, SemanticKind,
};
use crate::rooting;
use crate::type_analysis::{
    expr_may_return_boxed_value_from_raw_f64_fallback, is_numeric_expr, receiver_class_name,
};
use crate::types::{DOUBLE, I1, I32, I64, I8, PTR};

use super::{
    class_field_store_layout_note_is_conforming, class_field_store_needs_layout_note,
    class_field_store_needs_string_addref, emit_jsvalue_slot_store_pointer_tested,
    emit_typed_feedback_register_site, expr_produces_non_pointer_bits_by_construction, lower_expr,
    lower_expr_native, raw_f64_layout_fact, try_lower_pod_field_set, unbox_to_i64, FnCtx,
    TypedFeedbackContract, TypedFeedbackKind,
};

/// Metadata-only class candidate for the runtime-guarded plain-field store.
/// Accessor calls still require a real receiver proof; a lying annotation is
/// routed to the by-name setter path instead.
fn guarded_declared_class_store_candidate(ctx: &FnCtx<'_>, object: &Expr) -> Option<String> {
    let Expr::LocalGet(id) = object else {
        return None;
    };
    let perry_hir::types::Type::Named(name) = ctx.local_type_hint(id)? else {
        return None;
    };
    ctx.classes.contains_key(name).then(|| name.clone())
}

fn canonicalize_raw_f64_numeric_store_value(
    blk: &mut crate::block::LlBlock,
    value_double: &str,
) -> String {
    blk.call(
        DOUBLE,
        "js_array_numeric_value_to_raw_f64",
        &[(DOUBLE, value_double)],
    )
}

/// Lower the receiver/value pair for the class-field and setter fast paths.
///
/// `root_reload` already repairs the common bare-local / `this` receiver, and
/// keeping that path direct preserves its hot IR. A compound receiver is a call
/// result/phi with no storage the reload pass can name, so #7640 requires an
/// explicit operand group whenever the RHS can collect. The group itself keeps
/// inert RHSs byte-identical by answering `Reuse`.
fn with_class_store_operands<'f, R>(
    ctx: &mut FnCtx<'f>,
    object: &Expr,
    value: &Expr,
    body: impl FnOnce(&mut FnCtx<'f>, String, String) -> Result<R>,
) -> Result<R> {
    if matches!(object, Expr::LocalGet(_) | Expr::This) {
        let recv_box = lower_expr(ctx, object)?;
        let val_double = lower_expr(ctx, value)?;
        return body(ctx, recv_box, val_double);
    }
    rooting::with_operands_rooted(ctx, &[object, value], |ctx, vals| {
        body(ctx, vals[0].clone(), vals[1].clone())
    })
}

pub(crate) fn class_has_computed_runtime_members(ctx: &FnCtx<'_>, class_name: &str) -> bool {
    ctx.classes
        .get(class_name)
        .is_some_and(|class| !class.computed_members.is_empty())
}

/// #7288: the SLOPPY-mode arm of the #5093 class-field raw-f64 store.
///
/// `put_value_static_property_fast_path` bars sloppy code from the whole
/// class-field route (#6542) because that route's terminal fallback is
/// `js_object_set_field_by_name`, which throws unconditionally on a
/// non-writable slot — correct for strict `PutValue`, wrong for sloppy, where a
/// rejected write is a silent no-op.
///
/// That bail is far wider than the hazard, and the width is user-visible: an
/// identical `.ts` file compiles to a 46× slower object depending only on
/// whether an upward walk from the source finds a `package.json` with
/// `"type": "module"` (which makes the module ESM, hence strict). Inside the
/// Perry checkout it does; in a user's scratch directory it does not, so
/// `benchmarks/suite/09_method_calls.ts` measured 83 ms in-tree and 3.8 s
/// anywhere else.
///
/// The fast arm never needed the bail. The #5093 inline precheck
/// (`emit_class_field_inline_precheck`) already rejects every receiver whose
/// store could be *rejected* — `OBJ_FLAG_FROZEN`, `OBJ_FLAG_HAS_DESCRIPTORS`, a
/// mismatched class id or keys token, a cleared typed-layout-intact bit — plus
/// every value that is not a plain finite number, and the process-global gate
/// is flipped by any prototype-level descriptor install naming a declared
/// field. A store that reaches the raw slot is therefore one that could not
/// have been rejected in either mode, so the fast arm is mode-independent.
/// Only the fallback needed strict-awareness, and this sends every miss to
/// `js_put_value_set(..., strict = 0)` — the sloppy-correct runtime the
/// surrounding `PutValueSet` lowering already uses — instead of the throwing
/// by-name setter.
///
/// Scope: a declared field on a known class, receiver == target — raw-f64
/// (`number`) slots and boxed slots alike.
///
/// The boxed half is P1 (#5094). #7288 originally took only the raw-f64 slots
/// because "boxed slots need the layout note and write barrier that the
/// guard-call path emits" — but those are emitted by
/// [`emit_jsvalue_slot_store_pointer_tested`], not by the guard, and this arm
/// calls it with the identical value-side predicates the strict arm uses. What
/// the guard call actually contributes is descriptor-aware dispatch and the
/// setter-in-chain walk, and the inline precheck refuses every receiver that
/// needs either.
///
/// Leaving the boxed slots out was the more expensive half of the omission:
/// a `next: LNode | null` store fell through to the `PutValue` write IC
/// (`expr/proxy_reflect.rs`), whose miss path is `js_put_value_set` →
/// `js_object_set_field_by_name` — by-name dispatch, a `RuntimeHandleScope`,
/// and a per-object side-table touch, for a store whose slot index is a
/// compile-time constant. On `deeplist.ts` that one store was the benchmark.
pub(crate) fn try_lower_sloppy_class_field_store(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    property: &str,
    value: &Expr,
) -> Result<Option<String>> {
    // Oversized modules full-outline the whole IC diamond into one call
    // (#5334 lever B); that outlined runtime has no sloppy variant, so leave
    // those modules on the unchanged path.
    if crate::codegen::full_outline_ic_enabled() {
        return Ok(None);
    }
    let Some(class_name) = receiver_class_name(ctx, object)
        .or_else(|| guarded_declared_class_store_candidate(ctx, object))
    else {
        return Ok(None);
    };
    if class_has_computed_runtime_members(ctx, &class_name) {
        return Ok(None);
    }
    // A compiled setter owns the name; never store into the slot behind it.
    // (`class_field_global_index` also rejects accessors anywhere in the
    // chain — this is the same check the strict arm makes first, kept so the
    // two arms agree on which shapes are eligible.)
    if ctx
        .methods
        .contains_key(&(class_name.clone(), format!("__set_{}", property)))
    {
        return Ok(None);
    }
    let Some(field_index) =
        crate::type_analysis::class_field_global_index(ctx, &class_name, property)
    else {
        return Ok(None);
    };
    let (Some(&expected_class_id), Some(keys_global_name)) = (
        ctx.class_ids.get(&class_name),
        ctx.class_keys_globals.get(&class_name).cloned(),
    ) else {
        return Ok(None);
    };
    let requires_raw_f64 =
        crate::type_analysis::class_field_declared_type(ctx, &class_name, property)
            .as_ref()
            .is_some_and(crate::typed_shape::type_is_raw_f64_candidate);
    if !requires_raw_f64 {
        return try_lower_sloppy_class_field_boxed_store(
            ctx,
            object,
            property,
            value,
            field_index,
            expected_class_id,
            &keys_global_name,
            &class_name,
        );
    }

    // Operand order mirrors the strict class-field arm below verbatim: the
    // assignment reference is evaluated before the RHS.
    //
    // #7640 section C: this used to claim the receiver's relocation across an
    // allocating RHS was "handled by the same statepoint re-read that arm
    // relies on". That mechanism doesn't exist — RS4GC only relocates a value
    // that is still `ptr addrspace(1)`-typed and live across the safepoint,
    // and `recv_box` crosses it as a plain `double` (a `bitcast`/`ptrtoint`
    // chain, dead before the call, per `function/precise_roots.rs`). The
    // repair is deliberately split by receiver shape:
    //
    //  * `object` a bare `Expr::LocalGet`/`Expr::This` — its value IS a load
    //    out of a shadow slot, and `root_reload.rs` (#7280) re-materialises
    //    that load (plus any pure `bitcast`/`ptrtoint`/`and`/… derived from
    //    it) below any collection point it doesn't dominate. Unconditional
    //    on RS4GC — it runs before either root lowering sees the IR, so it
    //    protects shadow (`PERRY_RS4GC=0`) and native (`=1`, default)
    //    identically. Verified: `scripts/gc_root_dominance_check.py
    //    --stale-registers`/`--statepoints`, both lowerings, on
    //    `test-files/test_gap_gc_class_field_receiver_rooting.ts`'s
    //    `setRawF64`/`setBoxed`/`setViaSetter` — zero hazards.
    //  * `object` anything else — e.g. `this.target.x = allocPoint(n).x`,
    //    where the receiver is itself a class-field READ — cannot use that
    //    repair: the receiver
    //    is a `phi` over two field-get paths, not a direct shadow-slot load,
    //    so `root_reload` has no root to re-derive from, and
    //    `--stale-registers`' pattern match only anchors on a direct
    //    `load double, ptr <root>` source. Confirmed by hand on this exact
    //    shape (`Holder.setOnThis` in the test above): the field-get result
    //    register is reused, unreloaded, after `allocPoint`'s call in the
    //    emitted IR. `with_class_store_operands` closes exactly this residual
    //    with an explicit operand group, while routing bare locals / `this`
    //    through the unchanged direct path. Its own collection predicate keeps
    //    a compound receiver with an inert RHS byte-identical too.
    with_class_store_operands(ctx, object, value, |ctx, recv_box, val_double| {
        // #7287: inside the fast clone of a #5093 class-field versioned loop, this
        // store is covered by the preheader's hoisted shape check — emit the same
        // inline plain-finite check + bare slot store the STRICT arm emits (see
        // `lower`'s class-field arm), instead of the per-access diamond.
        //
        // Sound in sloppy mode for the same reason #7423 made the fast arm
        // mode-independent: the preheader proved not-frozen, no per-receiver
        // descriptors, matching class id and keys token, and an intact typed
        // layout, and the loop's body is call-free so none of that can change while
        // the clone runs. A store that reaches the raw slot could not have been
        // *rejected* in either mode, so there is no sloppy/strict divergence to
        // preserve. Everything else — a non-finite or NaN-boxed value — side-exits
        // to the slow clone BEFORE storing, and the slow clone re-executes the whole
        // iteration through this unchanged sloppy lowering.
        if let Expr::LocalGet(recv_id) = object {
            if let Some((fact, _)) = crate::expr::class_field_loop_fact_lookup(
                &ctx.class_field_loop_facts,
                *recv_id,
                &class_name,
                property,
            )
            .filter(|(_, loop_idx)| *loop_idx == field_index)
            {
                let obj_ptr = fact.obj_ptr.clone();
                let side_exit_label = fact.side_exit_label.clone();
                let store_idx = ctx.new_block("class_field_loop_store.sloppy_fast");
                let store_label = ctx.block_label(store_idx);
                {
                    let blk = ctx.block();
                    let val_bits = blk.bitcast_double_to_i64(&val_double);
                    let finite =
                        crate::expr::class_field_inline_guard::emit_plain_finite_number_check(
                            blk, &val_bits,
                        );
                    blk.cond_br(&finite, &store_label, &side_exit_label);
                }
                ctx.current_block = store_idx;
                {
                    let header_skip =
                        crate::target_layout::object_header_size_bytes(ctx.target_triple)
                            .to_string();
                    let blk = ctx.block();
                    let fields_base = blk.gep(I8, &obj_ptr, &[(I64, &header_skip)]);
                    let field_ptr =
                        blk.gep(DOUBLE, &fields_base, &[(I64, &field_index.to_string())]);
                    // No `js_array_numeric_value_to_raw_f64` canonicalization is
                    // needed: INT32-boxed and NaN values — the only inputs it
                    // rewrites — cannot pass the finite check above.
                    //
                    // GC_STORE_AUDIT(POINTER_FREE): the finite check proved
                    // `val_double` is a genuine unboxed double, never a heap
                    // pointer — no edge, no write barrier.
                    blk.store(DOUBLE, &val_double, &field_ptr);
                }
                return Ok(Some(val_double));
            }
        }

        let key_idx = ctx.strings.intern(property);
        let key_handle_global = format!("@{}", ctx.strings.entry(key_idx).handle_global);
        let field_idx_str = field_index.to_string();
        let expected_class_id_str = expected_class_id.to_string();
        let expected_shape_id =
            crate::lower_call::new_alloc::load_class_shape_id(ctx, &class_name, &keys_global_name);

        let (obj_bits, obj_handle, key_box, val_bits) = {
            let blk = ctx.block();
            let obj_bits = blk.bitcast_double_to_i64(&recv_box);
            let obj_handle = blk.and(I64, &obj_bits, POINTER_MASK_I64);
            let key_box = blk.load(DOUBLE, &key_handle_global);
            let val_bits = blk.bitcast_double_to_i64(&val_double);
            (obj_bits, obj_handle, key_box, val_bits)
        };

        let fast_idx = ctx.new_block("class_field_sloppy_set.fast");
        let merge_idx = ctx.new_block("class_field_sloppy_set.merge");
        let fast_label = ctx.block_label(fast_idx);
        let merge_label = ctx.block_label(merge_idx);

        // Emits the shape/flags/value precheck and branches to `fast_label` on a
        // hit; leaves `ctx.current_block` on the freshly created miss block.
        let subclass_arms = crate::expr::class_field_inline_guard::class_field_subclass_arms(
            ctx,
            &class_name,
            property,
            field_index,
            true,
        );
        let _miss_label = crate::expr::class_field_inline_guard::emit_class_field_inline_precheck(
            ctx,
            &obj_bits,
            &obj_handle,
            &expected_class_id_str,
            &expected_shape_id,
            field_index,
            true,
            Some(&val_bits),
            &fast_label,
            &subclass_arms,
        );

        // Miss: the strict-aware runtime with `strict = 0`, so a rejected write
        // stays a silent no-op exactly as sloppy `PutValue` requires.
        {
            let blk = ctx.block();
            let _ = blk.call(
                DOUBLE,
                "js_put_value_set",
                &[
                    (DOUBLE, &recv_box),
                    (DOUBLE, &key_box),
                    (DOUBLE, &val_double),
                    (DOUBLE, &recv_box),
                    (I32, "0"),
                ],
            );
            blk.br(&merge_label);
        }

        ctx.current_block = fast_idx;
        {
            // arm64_32 watchOS: the fields region starts at `size_of::<ObjectHeader>()`
            // past the user pointer (24 on 64-bit, 20 on ILP32) — same derivation as
            // the strict arm and the runtime setter.
            let header_skip =
                crate::target_layout::object_header_size_bytes(ctx.target_triple).to_string();
            let blk = ctx.block();
            let obj_ptr = blk.inttoptr(I64, &obj_handle);
            let fields_base = blk.gep(I8, &obj_ptr, &[(I64, &header_skip)]);
            let field_ptr = blk.gep(DOUBLE, &fields_base, &[(I64, &field_idx_str)]);
            // GC_STORE_AUDIT(POINTER_FREE): a guarded raw-f64 class slot holds
            // numbers only, and the precheck rejected every value that is not a
            // plain finite double, so no write barrier and no layout note are due.
            let numeric_value = canonicalize_raw_f64_numeric_store_value(blk, &val_double);
            blk.store(DOUBLE, &numeric_value, &field_ptr);
            blk.br(&merge_label);
        }

        ctx.current_block = merge_idx;
        Ok(Some(val_double))
    })
}

/// The boxed-slot half of [`try_lower_sloppy_class_field_store`] — P1 (#5094).
///
/// Same shape as the raw-f64 half: the #5093 inline precheck decides, a hit
/// stores straight into the packed slot, a miss goes to `js_put_value_set(...,
/// strict = 0)` so a rejected sloppy write stays a silent no-op.
///
/// # Why the precheck alone licenses a guard-free boxed store
///
/// `emit_class_field_inline_precheck` is a strict subset of the runtime's
/// `class_field_fast_contract`: on a hit, the guard call would have answered
/// "fast" too. For a SET it additionally proves the receiver is not frozen and
/// carries no per-object descriptors, and the process-global latch it reads
/// first is flipped by any prototype-level descriptor or accessor install. Add
/// the `__set_<property>` refusal the caller already made, and every way a
/// `[[Set]]` could be *rejected* or *diverted* is excluded — which is the only
/// thing sloppy and strict `PutValue` disagree about. The value plays no part:
/// unlike the raw-f64 arm, a boxed slot accepts any `JSValue`, so this arm
/// passes `require_raw_f64 = false` and the plain-finite test is not emitted.
///
/// # GC obligations
///
/// All three are discharged by [`emit_jsvalue_slot_store_pointer_tested`], with
/// the same value-side predicates the strict guarded arm computes — the write
/// barrier (`expr_produces_non_pointer_bits_by_construction`), the layout note
/// (`class_field_store_needs_layout_note`) and the string demote
/// (`class_field_store_needs_string_addref`). Whatever survives those static
/// proofs is decided by ONE live test of the stored bits (#7511), so a genuine
/// pointer store still reaches the remembered set. Nothing here is keyed on
/// strictness, so this arm's GC behaviour is byte-identical to the strict one.
#[allow(clippy::too_many_arguments)]
fn try_lower_sloppy_class_field_boxed_store(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    property: &str,
    value: &Expr,
    field_index: u32,
    expected_class_id: u32,
    keys_global_name: &str,
    class_name: &str,
) -> Result<Option<String>> {
    // The direct local/`this` path keeps the existing root-reload repair; the
    // compound path gets the explicit operand root the #7640 note above says it
    // lacked.
    with_class_store_operands(ctx, object, value, |ctx, recv_box, val_double| {
        // Computed before the block builder is borrowed below.
        let barrier_needed = !expr_produces_non_pointer_bits_by_construction(ctx, value);
        let layout_note_needed = class_field_store_needs_layout_note(ctx, value);
        let string_addref_needed = class_field_store_needs_string_addref(ctx, value);

        let key_idx = ctx.strings.intern(property);
        let key_handle_global = format!("@{}", ctx.strings.entry(key_idx).handle_global);
        let field_idx_str = field_index.to_string();
        let expected_class_id_str = expected_class_id.to_string();
        let expected_shape_id =
            crate::lower_call::new_alloc::load_class_shape_id(ctx, class_name, keys_global_name);

        let (obj_bits, obj_handle, key_box, val_bits) = {
            let blk = ctx.block();
            let obj_bits = blk.bitcast_double_to_i64(&recv_box);
            let obj_handle = blk.and(I64, &obj_bits, POINTER_MASK_I64);
            let key_box = blk.load(DOUBLE, &key_handle_global);
            let val_bits = blk.bitcast_double_to_i64(&val_double);
            (obj_bits, obj_handle, key_box, val_bits)
        };

        let fast_idx = ctx.new_block("class_field_sloppy_set.boxed_fast");
        let merge_idx = ctx.new_block("class_field_sloppy_set.boxed_merge");
        let fast_label = ctx.block_label(fast_idx);
        let merge_label = ctx.block_label(merge_idx);

        // `set_value_bits` is `Some` so the not-frozen check is emitted;
        // `require_raw_f64` is false, so the plain-finite value check is not.
        let subclass_arms = crate::expr::class_field_inline_guard::class_field_subclass_arms(
            ctx,
            class_name,
            property,
            field_index,
            false,
        );
        let _miss_label = crate::expr::class_field_inline_guard::emit_class_field_inline_precheck(
            ctx,
            &obj_bits,
            &obj_handle,
            &expected_class_id_str,
            &expected_shape_id,
            field_index,
            false,
            Some(&val_bits),
            &fast_label,
            &subclass_arms,
        );

        {
            let blk = ctx.block();
            let _ = blk.call(
                DOUBLE,
                "js_put_value_set",
                &[
                    (DOUBLE, &recv_box),
                    (DOUBLE, &key_box),
                    (DOUBLE, &val_double),
                    (DOUBLE, &recv_box),
                    (I32, "0"),
                ],
            );
            blk.br(&merge_label);
        }

        ctx.current_block = fast_idx;
        {
            // arm64_32 watchOS: the fields region starts at
            // `size_of::<ObjectHeader>()` past the user pointer — same derivation
            // as every sibling arm and the runtime setter.
            let header_skip =
                crate::target_layout::object_header_size_bytes(ctx.target_triple).to_string();
            let (field_ptr, field_addr) = {
                let blk = ctx.block();
                let obj_ptr = blk.inttoptr(I64, &obj_handle);
                let fields_base = blk.gep(I8, &obj_ptr, &[(I64, &header_skip)]);
                let field_ptr = blk.gep(DOUBLE, &fields_base, &[(I64, &field_idx_str)]);
                let field_addr = blk.ptrtoint(&field_ptr, I64);
                (field_ptr, field_addr)
            };
            emit_jsvalue_slot_store_pointer_tested(
                ctx,
                &field_ptr,
                &val_double,
                &obj_handle,
                &field_idx_str,
                string_addref_needed,
                layout_note_needed,
                &obj_bits,
                &field_addr,
                barrier_needed,
                class_field_store_layout_note_is_conforming(ctx, class_name, field_index),
            );
            ctx.block().br(&merge_label);
        }

        ctx.current_block = merge_idx;
        let stored = LoweredValue {
            semantic: SemanticKind::JsValue,
            rep: NativeRep::JsValue,
            llvm_ty: DOUBLE,
            value: val_double.clone(),
        };
        ctx.record_lowered_value_with_access_mode(
            "ClassFieldSet",
            None,
            "class_field_set.sloppy_boxed_store",
            &stored,
            Some(BoundsState::Guarded {
                guard_id: "class_field_inline_precheck".to_string(),
            }),
            None,
            Some(BufferAccessMode::CheckedNative),
            None,
            false,
            false,
            vec![
                format!("field={}", property),
                format!("field_index={}", field_idx_str),
                "receiver_proof=inline_precheck_exact_class".to_string(),
                "field_layout_raw_f64=false".to_string(),
                "store_guard_failure=js_put_value_set_sloppy".to_string(),
            ],
        );
        Ok(Some(val_double))
    })
}

fn lower_runtime_property_set_by_name(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    property: &str,
    value: &Expr,
) -> Result<String> {
    // #7154: root the receiver across the value's evaluation, which allocates.
    // The group re-reads it as part of emitting the store, so no register of
    // the receiver exists across the window.
    rooting::with_operands_rooted(ctx, &[object, value], |ctx, vals| {
        let (recv_box, val_double) = (&vals[0], &vals[1]);
        let key_idx = ctx.strings.intern(property);
        let dispatch_global = ctx.strings.static_dispatch_global(key_idx);
        let blk = ctx.block();
        let obj_bits = blk.bitcast_double_to_i64(recv_box);
        let property_id = crate::strings::emit_static_dispatch_id(blk, &dispatch_global);
        blk.call_void(
            "js_object_set_field_by_property_id",
            &[(I64, &obj_bits), (I64, &property_id), (DOUBLE, val_double)],
        );
        Ok(val_double.clone())
    })
}

fn lower_value_for_dynamic_property_set(
    ctx: &mut FnCtx<'_>,
    value: &Expr,
    consumer: &str,
    boxed_at: &str,
) -> Result<(String, String)> {
    let lowered = lower_expr_native(ctx, value, ExpectedNativeRep::JsValueBits)?;
    let value_bits = lowered.value.clone();
    let value_double = ctx.block().bitcast_i64_to_double(&value_bits);
    ctx.record_lowered_value(
        "PropertySet",
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

pub(crate) fn emit_nullish_write_guard(
    ctx: &mut FnCtx<'_>,
    obj_bits: &str,
    property: &str,
    label_prefix: &str,
) {
    let is_undef = ctx
        .block()
        .icmp_eq(I64, obj_bits, crate::nanbox::TAG_UNDEFINED_I64);
    let is_null = ctx
        .block()
        .icmp_eq(I64, obj_bits, crate::nanbox::TAG_NULL_I64);
    let is_nullish = ctx.block().or(I1, &is_undef, &is_null);
    let throw_idx = ctx.new_block(&format!("{}.throw_nullish", label_prefix));
    let ok_idx = ctx.new_block(&format!("{}.recv_ok", label_prefix));
    let throw_label = ctx.block_label(throw_idx);
    let ok_label = ctx.block_label(ok_idx);
    ctx.block().cond_br(&is_nullish, &throw_label, &ok_label);

    ctx.current_block = throw_idx;
    let key_idx = ctx.strings.intern(property);
    let prop_entry = ctx.strings.entry(key_idx);
    let prop_bytes_global = format!("@{}", prop_entry.bytes_global);
    let prop_len_str = prop_entry.byte_len.to_string();
    let is_null_i32 = ctx.block().zext(I1, &is_null, I32);
    ctx.block().call_void(
        "js_throw_type_error_property_access",
        &[
            (I32, &is_null_i32),
            (PTR, &prop_bytes_global),
            (I64, &prop_len_str),
        ],
    );
    ctx.block().unreachable();

    ctx.current_block = ok_idx;
}

pub(crate) fn lower(ctx: &mut FnCtx<'_>, expr: &Expr) -> Result<String> {
    match expr {
        Expr::PropertySet {
            object,
            property,
            value,
        } => {
            if let Expr::LocalGet(id) = object.as_ref() {
                if ctx.pod_records.get(id).is_some_and(|local| {
                    local
                        .layout
                        .fields
                        .iter()
                        .any(|field| field.name == *property)
                }) {
                    if let Some(value) = try_lower_pod_field_set(ctx, *id, property, value)? {
                        return Ok(value);
                    }
                }
            }
            // Closes #304: `arr.length = N` must mutate the ArrayHeader, not
            // set a "length" field in the object dispatch. Pre-fix the generic
            // `js_object_set_field_by_name(arr, "length", N)` path silently
            // recorded a property on the array's hidden dispatch object but
            // never touched the real ArrayHeader.length, so subsequent reads
            // of `arr.length` returned the stale original count and the
            // elements stayed live. Statically Array-typed receivers route to
            // `js_array_set_length` which truncates / extends the header.
            // Open question: dynamic `Any`-typed receivers that happen to be
            // arrays at runtime still hit the generic path and miss the fix —
            // they'd need a runtime-side check inside js_object_set_field_by_name
            // (route to js_array_set_length when the target is registered as
            // an array). Deliberately out of scope here; the static-typed
            // case covers the issue's repro.
            if property == "length" && crate::type_analysis::is_array_expr(ctx, object) {
                // #7637: this arm had NO store-operand guard, while every other
                // `PropertySet` arm in this file has had one since #7154. It is
                // the same window: `arr.length = f()` lowers the receiver first
                // (spec order), `f()` allocates and can drive an evacuating
                // minor, and `js_array_set_length_strict` then truncates through
                // a pre-move `ArrayHeader*` — the array the program keeps is
                // left at its old length. The receiver's own slot is a root the
                // collector rewrites; the register is not.
                return rooting::with_operands_rooted(ctx, &[object, value], |ctx, vals| {
                    let (arr_box, val_double) = (&vals[0], &vals[1]);
                    let blk = ctx.block();
                    let arr_bits = blk.bitcast_double_to_i64(arr_box);
                    let arr_handle = blk.and(I64, &arr_bits, POINTER_MASK_I64);
                    // `arr.length = v` is a strict `Set(O,"length",v,true)`: a
                    // frozen array's `length` is non-writable, so route to the
                    // throwing variant instead of the silent internal helper.
                    blk.call_void(
                        "js_array_set_length_strict",
                        &[(I64, &arr_handle), (DOUBLE, val_double)],
                    );
                    Ok(val_double.clone())
                });
            }
            // #1344: `process.env.X = v` must persist to the real OS
            // environment, not just a cached ProcessEnv object backing.
            // Pre-fix the generic `js_object_set_field_by_name` path
            // stored on the cached dict but `process.env.X` (`EnvGet`)
            // reads from `std::env::var` directly, so the value never
            // round-tripped and child processes inherited the
            // unmodified parent env.
            //
            // Route the store through `js_setenv(key, value)` (writes
            // via `std::env::set_var`, coerces non-string values to
            // strings via `js_jsvalue_to_string`). Reads still go
            // through `js_getenv_value`, so the round-trip works.
            if matches!(object.as_ref(), Expr::ProcessEnv) {
                let key_idx = ctx.strings.intern(property);
                let key_handle_global = format!("@{}", ctx.strings.entry(key_idx).handle_global);
                let val_double = lower_expr(ctx, value)?;
                let blk = ctx.block();
                let key_box = blk.load(DOUBLE, &key_handle_global);
                let key_handle = unbox_to_i64(blk, &key_box);
                blk.call_void("js_setenv", &[(I64, &key_handle), (DOUBLE, &val_double)]);
                return Ok(val_double);
            }
            // Scalar replacement fast path: store to the field's alloca.
            if let Expr::LocalGet(id) = object.as_ref() {
                if let Some(slot) = ctx
                    .scalar_replaced
                    .get(id)
                    .and_then(|fs| fs.get(property.as_str()))
                    .cloned()
                {
                    let raw_f64_field = crate::type_analysis::scalar_replaced_field_is_raw_f64(
                        ctx,
                        object.as_ref(),
                        property,
                    );
                    let numeric_store = raw_f64_field
                        && is_numeric_expr(ctx, value)
                        && !expr_may_return_boxed_value_from_raw_f64_fallback(ctx, value);
                    let val_double = lower_expr(ctx, value)?;
                    let stored_value = if numeric_store {
                        canonicalize_raw_f64_numeric_store_value(ctx.block(), &val_double)
                    } else {
                        val_double.clone()
                    };
                    ctx.block().store(DOUBLE, &stored_value, &slot);
                    // #6968: bind the field alloca as a precise GC root, the
                    // same treatment `emit_shadow_slot_update_for_expr` gives
                    // an ordinary pointer-typed local. Skipped for a
                    // `numeric_store`, whose stored bits are a canonicalized
                    // raw `f64` by construction.
                    if !numeric_store {
                        crate::expr::root_scalar_replaced_slot(ctx, &slot, value);
                    }
                    // String-alias fix (mirror of `let y = x` in stmt/let_stmt.rs):
                    // a string-typed local stored into a scalar-replaced field's
                    // alloca slot aliases the same heap buffer. The runtime
                    // write-barrier choke point (runtime_store_jsvalue_slot) can't
                    // see this store because scalar replacement elides the real
                    // heap object, so mark the buffer shared here. Otherwise a
                    // later `s = s + suffix` mutates it in-place via
                    // js_string_append's refcount==1 fast path and corrupts this
                    // field. The helper checks the runtime tag, so apply it to
                    // every local source: an erased non-string annotation is
                    // not proof that the current value cannot be a string.
                    if matches!(&**value, Expr::LocalGet(_)) {
                        ctx.block()
                            .call_void("js_string_addref_if_heap_string", &[(DOUBLE, &val_double)]);
                    }
                    let lowered_js = LoweredValue {
                        semantic: SemanticKind::JsValue,
                        rep: NativeRep::JsValue,
                        llvm_ty: DOUBLE,
                        value: val_double.clone(),
                    };
                    ctx.record_lowered_value_with_access_mode(
                        "ScalarObjectFieldSet",
                        Some(*id),
                        "scalar_object_field_store",
                        &lowered_js,
                        None,
                        None,
                        None,
                        None,
                        false,
                        false,
                        vec![
                            format!("field={}", property),
                            format!("raw_f64_field={}", raw_f64_field as u8),
                        ],
                    );
                    if numeric_store {
                        let lowered_f64 = LoweredValue::f64(stored_value.clone());
                        ctx.record_lowered_value_with_access_mode(
                            "ScalarObjectFieldSet",
                            Some(*id),
                            "scalar_object_field_store.raw_f64",
                            &lowered_f64,
                            None,
                            None,
                            None,
                            None,
                            false,
                            false,
                            vec![format!("field={}", property), "raw_f64_field=1".to_string()],
                        );
                    }
                    return Ok(val_double);
                }
            }
            // Handle `this` during scalar-replaced constructor inlining:
            if let Expr::This = object.as_ref() {
                if let Some(target_id) = ctx.scalar_ctor_target.last().copied() {
                    let maybe_slot = ctx
                        .scalar_replaced
                        .get(&target_id)
                        .and_then(|slots| slots.get(property.as_str()).cloned());
                    let raw_f64_field = crate::type_analysis::scalar_replaced_field_is_raw_f64(
                        ctx,
                        object.as_ref(),
                        property,
                    );
                    let numeric_store = raw_f64_field
                        && is_numeric_expr(ctx, value)
                        && !expr_may_return_boxed_value_from_raw_f64_fallback(ctx, value);
                    let val_double = lower_expr(ctx, value)?;
                    if let Some(slot) = maybe_slot {
                        let stored_value = if numeric_store {
                            canonicalize_raw_f64_numeric_store_value(ctx.block(), &val_double)
                        } else {
                            val_double.clone()
                        };
                        ctx.block().store(DOUBLE, &stored_value, &slot);
                        // #6968: see the `ScalarObjectFieldSet` path above —
                        // an inlined constructor's `this.f = …` writes the
                        // same kind of unrooted per-field alloca.
                        if !numeric_store {
                            crate::expr::root_scalar_replaced_slot(ctx, &slot, value);
                        }
                        // String-alias fix: see the ScalarObjectFieldSet path
                        // above. `this.field = s` into a scalar-replaced ctor slot
                        // aliases the string buffer; mark it shared so a later
                        // self-append doesn't mutate it in-place and corrupt the
                        // field.
                        if matches!(&**value, Expr::LocalGet(_)) {
                            ctx.block().call_void(
                                "js_string_addref_if_heap_string",
                                &[(DOUBLE, &val_double)],
                            );
                        }
                        let lowered_js = LoweredValue {
                            semantic: SemanticKind::JsValue,
                            rep: NativeRep::JsValue,
                            llvm_ty: DOUBLE,
                            value: val_double.clone(),
                        };
                        ctx.record_lowered_value_with_access_mode(
                            "ScalarThisFieldSet",
                            Some(target_id),
                            "scalar_object_field_store",
                            &lowered_js,
                            None,
                            None,
                            None,
                            None,
                            false,
                            false,
                            vec![
                                format!("field={}", property),
                                format!("raw_f64_field={}", raw_f64_field as u8),
                            ],
                        );
                        if numeric_store {
                            let lowered_f64 = LoweredValue::f64(stored_value.clone());
                            ctx.record_lowered_value_with_access_mode(
                                "ScalarThisFieldSet",
                                Some(target_id),
                                "scalar_object_field_store.raw_f64",
                                &lowered_f64,
                                None,
                                None,
                                None,
                                None,
                                false,
                                false,
                                vec![format!("field={}", property), "raw_f64_field=1".to_string()],
                            );
                        }
                    }
                    return Ok(val_double);
                }
            }
            // Setter dispatch: if the receiver is a known class and the
            // property is registered as a setter, call the synthesized
            // __set_<property> method instead of doing a raw field
            // store. The setter takes (this, value) and returns
            // undefined; we forward `value` as the expression result.
            //
            // #7640 section C: `recv_box` below is lowered before `value`
            // exactly like the class-field arms. Keep the zero-cost
            // `LocalGet`/`This` path, and conditionally root a compound
            // receiver across an allocating value expression.
            let proven_class_name = receiver_class_name(ctx, object);
            if let Some(class_name) = proven_class_name
                .clone()
                .or_else(|| guarded_declared_class_store_candidate(ctx, object))
            {
                if class_has_computed_runtime_members(ctx, &class_name) {
                    return lower_runtime_property_set_by_name(ctx, object, property, value);
                }
                let setter_key = (class_name.clone(), format!("__set_{}", property));
                // STATIC accessors compile under the static (no-`this`)
                // convention — see the matching gate in property_get.rs.
                let is_static_accessor = ctx
                    .classes
                    .get(&class_name)
                    .map(|c| c.static_accessor_names.iter().any(|n| n == property))
                    .unwrap_or(false);
                if !is_static_accessor {
                    if let Some(fn_name) = ctx.methods.get(&setter_key).cloned() {
                        if proven_class_name.is_none() {
                            return lower_runtime_property_set_by_name(
                                ctx, object, property, value,
                            );
                        }
                        return with_class_store_operands(
                            ctx,
                            object,
                            value,
                            |ctx, recv_box, val_double| {
                                let _ = ctx.block().call(
                                    DOUBLE,
                                    &fn_name,
                                    &[(DOUBLE, &recv_box), (DOUBLE, &val_double)],
                                );
                                Ok(val_double)
                            },
                        );
                    }
                }
                // Fast path: known class instance + plain instance field.
                // The runtime guard checks the receiver's class/shape and
                // descriptor state before this block touches the raw slot.
                //
                // This is "the strict class-field arm below" the sloppy arms
                // in `try_lower_sloppy_class_field_store` name — same
                // `recv_box`-before-`value` order, same #7640 section C split:
                // direct `LocalGet`/`This` stays on root-reload, while a compound
                // receiver is explicitly rooted by `with_class_store_operands`.
                if let Some(field_index) =
                    crate::type_analysis::class_field_global_index(ctx, &class_name, property)
                {
                    if let (Some(&expected_class_id), Some(keys_global_name)) = (
                        ctx.class_ids.get(&class_name),
                        ctx.class_keys_globals.get(&class_name).cloned(),
                    ) {
                        return with_class_store_operands(
                            ctx,
                            object,
                            value,
                            |ctx, recv_box, val_double| {
                                let key_idx = ctx.strings.intern(property);
                                let key_handle_global =
                                    format!("@{}", ctx.strings.entry(key_idx).handle_global);
                                let site_id = emit_typed_feedback_register_site(
                                    ctx,
                                    TypedFeedbackKind::PropertySet,
                                    property,
                                    TypedFeedbackContract::class_field_set(),
                                );
                                let field_idx_str = field_index.to_string();
                                let expected_class_id_str = expected_class_id.to_string();
                                let requires_raw_f64 =
                                    crate::type_analysis::class_field_declared_type(
                                        ctx,
                                        &class_name,
                                        property,
                                    )
                                    .as_ref()
                                    .is_some_and(crate::typed_shape::type_is_raw_f64_candidate);
                                let requires_raw_f64_str = if requires_raw_f64 { "1" } else { "0" };
                                let expected_shape_id =
                                    crate::lower_call::new_alloc::load_class_shape_id(
                                        ctx,
                                        &class_name,
                                        &keys_global_name,
                                    );
                                // #5093 loop versioning: inside the fast clone of a
                                // class-field versioned loop, a tracked raw-f64 field
                                // store on the proven receiver lowers to an inline
                                // plain-finite value check + bare slot store on the
                                // preheader-cached object pointer. A value that is
                                // not a plain finite double (±Inf/NaN, or any NaN-box
                                // tag — including INT32-boxed integers) side-exits to
                                // the slow clone's preheader BEFORE the store, so the
                                // slow clone re-executes the whole iteration and
                                // routes the value through the runtime guard exactly
                                // as today (downgrade semantics preserved).
                                if requires_raw_f64 {
                                    let loop_fact =
                                        match object.as_ref() {
                                            Expr::LocalGet(recv_id) => {
                                                crate::expr::class_field_loop_fact_lookup(
                                                    &ctx.class_field_loop_facts,
                                                    *recv_id,
                                                    &class_name,
                                                    property,
                                                )
                                                .filter(|(_, loop_idx)| *loop_idx == field_index)
                                                .map(|(fact, _)| {
                                                    (
                                                        fact.obj_ptr.clone(),
                                                        fact.side_exit_label.clone(),
                                                    )
                                                })
                                            }
                                            _ => None,
                                        };
                                    if let Some((obj_ptr, side_exit_label)) = loop_fact {
                                        let field_idx_str = field_index.to_string();
                                        let store_idx =
                                            ctx.new_block("class_field_loop_store.fast");
                                        let store_label = ctx.block_label(store_idx);
                                        {
                                            let blk = ctx.block();
                                            let val_bits = blk.bitcast_double_to_i64(&val_double);
                                            let finite = crate::expr::class_field_inline_guard::
                                        emit_plain_finite_number_check(blk, &val_bits);
                                            blk.cond_br(&finite, &store_label, &side_exit_label);
                                        }
                                        ctx.current_block = store_idx;
                                        {
                                            let header_skip =
                                                crate::target_layout::object_header_size_bytes(
                                                    ctx.target_triple,
                                                )
                                                .to_string();
                                            let blk = ctx.block();
                                            let fields_base =
                                                blk.gep(I8, &obj_ptr, &[(I64, &header_skip)]);
                                            let field_ptr = blk.gep(
                                                DOUBLE,
                                                &fields_base,
                                                &[(I64, &field_idx_str)],
                                            );
                                            // No raw-f64 canonicalization call is needed:
                                            // INT32-boxed and NaN values — the only
                                            // inputs `js_array_numeric_value_to_raw_f64`
                                            // rewrites — cannot pass the finite check.
                                            //
                                            // GC_STORE_AUDIT(POINTER_FREE): the inline
                                            // finite check proved `val_double` is a
                                            // genuine (unboxed, finite) double, never a
                                            // heap pointer — no edge, no write barrier.
                                            blk.store(DOUBLE, &val_double, &field_ptr);
                                        }
                                        let stored = LoweredValue {
                                            semantic: SemanticKind::JsNumber,
                                            rep: NativeRep::F64,
                                            llvm_ty: DOUBLE,
                                            value: val_double.clone(),
                                        };
                                        ctx.record_lowered_value_with_access_mode_and_facts(
                                            "ClassFieldSet",
                                            None,
                                            "class_field_set.loop_raw_f64_store",
                                            &stored,
                                            Some(BoundsState::Guarded {
                                                guard_id: "class_field_loop_preheader_check"
                                                    .to_string(),
                                            }),
                                            None,
                                            Some(BufferAccessMode::CheckedNative),
                                            None,
                                            None,
                                            None,
                                            vec![raw_f64_layout_fact(
                                                None,
                                                "consumed",
                                                "class_field_loop_preheader_check",
                                                None,
                                            )],
                                            Vec::new(),
                                            false,
                                            false,
                                            vec![
                                                format!("class={}", class_name),
                                                format!("field={}", property),
                                                format!("field_index={}", field_idx_str),
                                                "receiver_proof=loop_preheader_shape_check"
                                                    .to_string(),
                                                "field_layout=raw_f64_slot_array".to_string(),
                                                "loop_versioning=class_field_fast_clone"
                                                    .to_string(),
                                                "rhs_numeric_guard=inline_plain_finite_check"
                                                    .to_string(),
                                                "store_guard_failure=side_exit_slow_restart"
                                                    .to_string(),
                                            ],
                                        );
                                        return Ok(val_double);
                                    }
                                }
                                // Representation-selection Phase 3b: shape-proven
                                // Ptr<Shape> receiver (collectors/ptr_shape.rs) — no
                                // guard call, no shape diamond. Raw-f64 slots keep the
                                // inline plain-finite value check with a cold
                                // `js_class_field_set_fallback` arm (a NaN/Inf/boxed
                                // value must never be stored raw into a scalar-masked
                                // slot — the runtime setter performs the layout
                                // downgrade the GC scan relies on). Boxed slots store
                                // inline with the existing generational write barrier
                                // for possibly-pointer values.
                                // Phase 5a routes `this` here too. The freeze-family
                                // module-wide kill (collectors/proven_this.rs) is what
                                // makes a guard-free STORE through a proven `this`
                                // sound: unlike a Phase 3b local the receiver is
                                // caller-owned and therefore aliased, so a frozen or
                                // sealed target would otherwise silently accept a raw
                                // store where the spec requires a strict TypeError.
                                let ptr_shape_proven = ctx
                                    .ptr_shape_receiver_fact(object.as_ref())
                                    .map(|fact| fact.class_name == class_name)
                                    .unwrap_or(false);
                                if ptr_shape_proven {
                                    ctx.note_ptr_shape_consumed(object.as_ref(), "ptr_shape_set");
                                    let header_skip =
                                        crate::target_layout::object_header_size_bytes(
                                            ctx.target_triple,
                                        )
                                        .to_string();
                                    let field_set_barrier_needed =
                                        !expr_produces_non_pointer_bits_by_construction(ctx, value);
                                    let (obj_bits, obj_handle, field_ptr, val_bits) = {
                                        let blk = ctx.block();
                                        let obj_bits = blk.bitcast_double_to_i64(&recv_box);
                                        let obj_handle = blk.and(I64, &obj_bits, POINTER_MASK_I64);
                                        let obj_ptr = blk.inttoptr(I64, &obj_handle);
                                        let fields_base =
                                            blk.gep(I8, &obj_ptr, &[(I64, &header_skip)]);
                                        let field_ptr =
                                            blk.gep(DOUBLE, &fields_base, &[(I64, &field_idx_str)]);
                                        let val_bits = blk.bitcast_double_to_i64(&val_double);
                                        (obj_bits, obj_handle, field_ptr, val_bits)
                                    };
                                    if requires_raw_f64 {
                                        let store_idx = ctx.new_block("ptr_shape_set.raw_store");
                                        let cold_idx = ctx.new_block("ptr_shape_set.downgrade");
                                        let merge_idx = ctx.new_block("ptr_shape_set.merge");
                                        let store_label = ctx.block_label(store_idx);
                                        let cold_label = ctx.block_label(cold_idx);
                                        let merge_label = ctx.block_label(merge_idx);
                                        {
                                            let blk = ctx.block();
                                            let finite = crate::expr::class_field_inline_guard::
                                        emit_plain_finite_number_check(blk, &val_bits);
                                            blk.cond_br(&finite, &store_label, &cold_label);
                                        }
                                        ctx.current_block = store_idx;
                                        {
                                            // The finite check proved a genuine unboxed
                                            // double (INT32-boxed and every NaN-box tag
                                            // share the all-ones exponent), so no
                                            // canonicalization call is needed.
                                            let blk = ctx.block();
                                            // GC_STORE_AUDIT(POINTER_FREE): pointer-free
                                            // by that proof — no GC pointer reaches the
                                            // slot, so no write barrier.
                                            blk.store(DOUBLE, &val_double, &field_ptr);
                                            blk.br(&merge_label);
                                        }
                                        ctx.current_block = cold_idx;
                                        {
                                            let blk = ctx.block();
                                            let key_box = blk.load(DOUBLE, &key_handle_global);
                                            let key_bits = blk.bitcast_double_to_i64(&key_box);
                                            let key_raw = blk.and(I64, &key_bits, POINTER_MASK_I64);
                                            blk.call_void(
                                                "js_class_field_set_fallback",
                                                &[
                                                    (I64, &site_id),
                                                    (I64, &obj_bits),
                                                    (I64, &key_raw),
                                                    (DOUBLE, &val_double),
                                                ],
                                            );
                                            blk.br(&merge_label);
                                        }
                                        ctx.current_block = merge_idx;
                                    } else {
                                        // Repsel Phase 4b.1: retire the two bookkeeping
                                        // calls that are provably dead here.
                                        //
                                        // The receiver being `Ptr<Shape>`-proven is
                                        // what licenses the layout-note elision. Three
                                        // facts close it:
                                        //
                                        // Both are decided from the VALUE expression,
                                        // and gated independently because they are dead
                                        // under different conditions: the note needs
                                        // "not a pointer", the addref only "not a heap
                                        // string". Neither is keyed on the declared
                                        // field type — Perry does not enforce declared
                                        // types at runtime, so a `boolean` field can
                                        // legitimately receive a string through an
                                        // `any`, and a wrong addref elision there
                                        // silently corrupts it on the next in-place
                                        // append.
                                        //
                                        // `requires_raw_f64` is false on this arm, so
                                        // the raw-f64-mask arm of `layout_note_slot` —
                                        // the one that *must* downgrade — is
                                        // unreachable from here. The full per-layout-
                                        // state argument, including why a pointer store
                                        // into a pointer-masked slot is deliberately
                                        // NOT elided, is on
                                        // `class_field_store_needs_layout_note`.
                                        let layout_note_needed =
                                            class_field_store_needs_layout_note(ctx, value);
                                        let string_addref_needed =
                                            class_field_store_needs_string_addref(ctx, value);
                                        let field_addr = ctx.block().ptrtoint(&field_ptr, I64);
                                        // #7511: whatever these three flags could not
                                        // be proved away statically is decided by ONE
                                        // live test of the stored bits — see
                                        // `emit_jsvalue_slot_store_pointer_tested`.
                                        emit_jsvalue_slot_store_pointer_tested(
                                            ctx,
                                            &field_ptr,
                                            &val_double,
                                            &obj_handle,
                                            &field_idx_str,
                                            string_addref_needed,
                                            layout_note_needed,
                                            &obj_bits,
                                            &field_addr,
                                            field_set_barrier_needed,
                                            class_field_store_layout_note_is_conforming(
                                                ctx,
                                                &class_name,
                                                field_index,
                                            ),
                                        );
                                    }
                                    let (semantic, rep) = if requires_raw_f64 {
                                        (SemanticKind::JsNumber, NativeRep::F64)
                                    } else {
                                        (SemanticKind::JsValue, NativeRep::JsValue)
                                    };
                                    let stored = LoweredValue {
                                        semantic,
                                        rep,
                                        llvm_ty: DOUBLE,
                                        value: val_double.clone(),
                                    };
                                    ctx.record_lowered_value_with_access_mode_and_facts(
                                        "ClassFieldSet",
                                        None,
                                        "class_field_set.shape_proven_store",
                                        &stored,
                                        Some(BoundsState::Guarded {
                                            guard_id: "ptr_shape_static_proof".to_string(),
                                        }),
                                        None,
                                        Some(BufferAccessMode::CheckedNative),
                                        None,
                                        None,
                                        None,
                                        if requires_raw_f64 {
                                            vec![raw_f64_layout_fact(
                                                None,
                                                "consumed",
                                                "ptr_shape_static_proof",
                                                None,
                                            )]
                                        } else {
                                            Vec::new()
                                        },
                                        Vec::new(),
                                        false,
                                        false,
                                        vec![
                                            format!("class={}", class_name),
                                            format!("field={}", property),
                                            format!("field_index={}", field_idx_str),
                                            "receiver_proof=ptr_shape_local".to_string(),
                                            format!("field_layout_raw_f64={}", requires_raw_f64),
                                        ],
                                    );
                                    return Ok(val_double);
                                }
                                // #5334 lever B: oversized modules full-outline the entire
                                // class-field-SET IC diamond (guard + fast store +
                                // fallback) to a single `js_class_field_set_ic(...)` call.
                                // This trades a call frame on the (cold, startup-
                                // dominated) field-set path for a large per-site IR
                                // reduction, so clang -O0 — which oversized modules are
                                // forced to (#4880) — can actually compile the module.
                                // Only the call's own operands are materialized (the key
                                // handle + expected-keys), not the inline-store scaffolding.
                                if crate::codegen::full_outline_ic_enabled() {
                                    let key_raw = {
                                        let blk = ctx.block();
                                        let key_box = blk.load(DOUBLE, &key_handle_global);
                                        let key_bits = blk.bitcast_double_to_i64(&key_box);
                                        blk.and(I64, &key_bits, POINTER_MASK_I64)
                                    };
                                    ctx.block().call_void(
                                        "js_class_field_set_ic",
                                        &[
                                            (I64, &site_id),
                                            (DOUBLE, &recv_box),
                                            (I32, &expected_class_id_str),
                                            (I32, &expected_shape_id),
                                            (I64, &key_raw),
                                            (I32, &field_idx_str),
                                            (DOUBLE, &val_double),
                                            (I32, requires_raw_f64_str),
                                        ],
                                    );
                                    return Ok(val_double);
                                }
                                // #5093: build the guard operands once, up front, so both
                                // the inline shape pre-check and the guard-call fallback
                                // can reference them.
                                let (obj_bits, obj_handle, key_raw, val_bits) = {
                                    let blk = ctx.block();
                                    let obj_bits = blk.bitcast_double_to_i64(&recv_box);
                                    let obj_handle = blk.and(I64, &obj_bits, POINTER_MASK_I64);
                                    let key_box = blk.load(DOUBLE, &key_handle_global);
                                    let key_bits = blk.bitcast_double_to_i64(&key_box);
                                    let key_raw = blk.and(I64, &key_bits, POINTER_MASK_I64);
                                    let val_bits = blk.bitcast_double_to_i64(&val_double);
                                    (obj_bits, obj_handle, key_raw, val_bits)
                                };
                                let fast_idx = ctx.new_block("class_field_set.fast");
                                let fallback_idx = ctx.new_block("class_field_set.fallback");
                                let merge_idx = ctx.new_block("class_field_set.merge");
                                let fast_label = ctx.block_label(fast_idx);
                                let fallback_label = ctx.block_label(fallback_idx);
                                let merge_label = ctx.block_label(merge_idx);

                                // #5093: inline shape pre-check. On a hit this branches
                                // straight to the store, skipping the call; on a miss the
                                // guard-call path below runs unchanged.
                                //
                                // #7854: this used to be gated on `requires_raw_f64`,
                                // leaving every BOXED declared field (`string`, a class
                                // type, a union — i.e. most fields of most objects) paying
                                // an unconditional cross-crate
                                // `js_typed_feedback_class_field_set_guard` call per
                                // store, including the synthesized
                                // `__AnonShape_*_constructor` that every closed-shape
                                // object literal runs. The stated reason — "its setter-in-
                                // chain handling and write barrier aren't reproduced
                                // inline" — is answered by
                                // `try_lower_sloppy_class_field_boxed_store`, which has
                                // taken the boxed inline precheck since #7288: the write
                                // barrier, layout note and string demote come from
                                // `emit_jsvalue_slot_store_pointer_tested` (which the
                                // shared `fast_label` block below calls, with the very
                                // same value-side predicates), NOT from the guard; and a
                                // setter in the chain is already refused upstream by
                                // `class_field_global_index`'s `accessor_in_chain`.
                                //
                                // What the precheck proves is a strict subset of the
                                // runtime `class_field_fast_contract`: on a hit the guard
                                // call would have answered "fast" too, so this only
                                // removes a call, never changes which store happens. Every
                                // miss still lands on the guardcall block and the
                                // unchanged strict fallback, so `[[Set]]` rejection and
                                // descriptor dispatch are untouched. `require_raw_f64` is
                                // forwarded rather than hardcoded, so a boxed slot skips
                                // the plain-finite value test (a boxed slot accepts any
                                // `JSValue`) but still proves not-frozen / no per-object
                                // descriptors via `set_value_bits: Some`.
                                //
                                // #7861: and the shape test it emits is widened from the
                                // DECLARED class to that class's subclass closure. Without
                                // this the boxed arm #7854 just un-gated would still miss
                                // 100% of the time for a store in a base class's own
                                // constructor, where `this` is only ever a subclass. The
                                // arms are computed with `requires_raw_f64` rather than a
                                // literal, so a candidate whose declared type disagrees
                                // about the slot's representation is dropped.
                                let subclass_arms =
                            crate::expr::class_field_inline_guard::class_field_subclass_arms(
                                ctx,
                                &class_name,
                                property,
                                field_index,
                                requires_raw_f64,
                            );
                                let _guardcall_label =
                            crate::expr::class_field_inline_guard::emit_class_field_inline_precheck(
                                ctx,
                                &obj_bits,
                                &obj_handle,
                                &expected_class_id_str,
                                &expected_shape_id,
                                field_index,
                                requires_raw_f64,
                                Some(&val_bits),
                                &fast_label,
                                &subclass_arms,
                            );
                                let guard_ok = ctx.block().call(
                                    I32,
                                    "js_typed_feedback_class_field_set_guard",
                                    &[
                                        (I64, &site_id),
                                        (DOUBLE, &recv_box),
                                        (I32, &expected_class_id_str),
                                        (I32, &expected_shape_id),
                                        (I64, &key_raw),
                                        (I32, &field_idx_str),
                                        (DOUBLE, &val_double),
                                        (I32, requires_raw_f64_str),
                                    ],
                                );
                                let guard_pass = ctx.block().icmp_ne(I32, &guard_ok, "0");
                                ctx.block()
                                    .cond_br(&guard_pass, &fast_label, &fallback_label);

                                ctx.current_block = fast_idx;
                                // #5334 lever D: a value that is a non-pointer by
                                // construction (number / bool / undefined / null /
                                // comparison / arithmetic) creates no parent→child heap
                                // reference, so the generational write barrier is a
                                // semantic no-op and can be skipped. Computed before the
                                // block builder is borrowed below. The LAYOUT NOTE is
                                // kept regardless: it records the slot's pointer-ness for
                                // minor-scan skipping, and a non-pointer write into a
                                // slot that previously held a pointer is a real
                                // transition the GC must observe. Same soundness standard
                                // as the array-store barrier elision.
                                let field_set_barrier_needed =
                                    !expr_produces_non_pointer_bits_by_construction(ctx, value);
                                // #7469: value-side elision of the addref and layout
                                // note on the guarded arm — computed here because the
                                // predicates take `&FnCtx` and the block builder is
                                // borrowed below.
                                let guarded_note_needed =
                                    class_field_store_needs_layout_note(ctx, value);
                                let guarded_addref_needed =
                                    class_field_store_needs_string_addref(ctx, value);
                                let raw_stored_value = {
                                    // arm64_32 watchOS: the object fields region begins at
                                    // `size_of::<ObjectHeader>()` past the user pointer — 24 on
                                    // 64-bit, 20 on ILP32 (the trailing `keys_array` pointer is
                                    // 4 bytes there). A hardcoded 24 writes every class field 4
                                    // bytes off on a 32-bit watch; the paired inline read
                                    // (`property_get`) and the runtime setter must agree, so
                                    // derive it from the target triple (no-op on 64-bit; see
                                    // `target_layout`).
                                    let header_skip =
                                        crate::target_layout::object_header_size_bytes(
                                            ctx.target_triple,
                                        )
                                        .to_string();
                                    let field_ptr = {
                                        let blk = ctx.block();
                                        let obj_ptr = blk.inttoptr(I64, &obj_handle);
                                        let fields_base =
                                            blk.gep(I8, &obj_ptr, &[(I64, &header_skip)]);
                                        blk.gep(DOUBLE, &fields_base, &[(I64, &field_idx_str)])
                                    };
                                    let raw_stored_value = if requires_raw_f64 {
                                        // Guarded raw-f64 slots are pointer-free by typed
                                        // shape descriptor; non-number writes miss the
                                        // guard and use the boxed setter fallback.
                                        // GC_STORE_AUDIT(POINTER_FREE): typed raw-f64 class
                                        // slots contain numbers only.
                                        let blk = ctx.block();
                                        let numeric_value =
                                            canonicalize_raw_f64_numeric_store_value(
                                                blk,
                                                &val_double,
                                            );
                                        blk.store(DOUBLE, &numeric_value, &field_ptr);
                                        Some(numeric_value)
                                    } else {
                                        // #5334 lever D: skip the barrier when the value
                                        // is a non-pointer by construction. #7469 extends
                                        // the same value-expression gating to the addref
                                        // and layout note — the Phase 4b.1 predicates are
                                        // value-side-only proofs (see their docs: safe in
                                        // every layout state the receiver can be in), so
                                        // they apply on this guarded arm exactly as on
                                        // the ptr-shape-proven arm above. The guard
                                        // passing does not change what the VALUE can be;
                                        // `requires_raw_f64` is false here, which is the
                                        // precondition `class_field_store_needs_layout_note`
                                        // documents.
                                        //
                                        // #7511: this is the arm the shared
                                        // `<class>_constructor` symbol lands on, where the
                                        // value is an opaque function parameter and lever D
                                        // can never fire. Whatever survives it is decided by
                                        // ONE live test of the stored bits instead of three
                                        // cross-crate calls that each re-ask the same
                                        // question — see
                                        // `emit_jsvalue_slot_store_pointer_tested`.
                                        let field_addr = ctx.block().ptrtoint(&field_ptr, I64);
                                        emit_jsvalue_slot_store_pointer_tested(
                                            ctx,
                                            &field_ptr,
                                            &val_double,
                                            &obj_handle,
                                            &field_idx_str,
                                            guarded_addref_needed,
                                            guarded_note_needed,
                                            &obj_bits,
                                            &field_addr,
                                            field_set_barrier_needed,
                                            class_field_store_layout_note_is_conforming(
                                                ctx,
                                                &class_name,
                                                field_index,
                                            ),
                                        );
                                        None
                                    };
                                    ctx.block().br(&merge_label);
                                    raw_stored_value
                                };
                                if let Some(numeric_value) = raw_stored_value {
                                    let stored = LoweredValue {
                                        semantic: SemanticKind::JsNumber,
                                        rep: NativeRep::F64,
                                        llvm_ty: DOUBLE,
                                        value: numeric_value.clone(),
                                    };
                                    ctx.record_lowered_value_with_access_mode_and_facts(
                                        "ClassFieldSet",
                                        None,
                                        "class_field_set.raw_f64_store",
                                        &stored,
                                        Some(BoundsState::Guarded {
                                            guard_id: "class_field_set_guard".to_string(),
                                        }),
                                        None,
                                        Some(BufferAccessMode::CheckedNative),
                                        None,
                                        None,
                                        None,
                                        vec![raw_f64_layout_fact(
                                            None,
                                            "consumed",
                                            "class_field_set_guard",
                                            None,
                                        )],
                                        Vec::new(),
                                        false,
                                        false,
                                        vec![
                                    format!("class={}", class_name),
                                    format!("class_id={}", expected_class_id_str),
                                    format!("field={}", property),
                                    format!("field_index={}", field_idx_str),
                                    "receiver_proof=declared_named_receiver_guarded_exact_class"
                                        .to_string(),
                                    "field_layout=raw_f64_slot_array".to_string(),
                                    "pointer_bitmap=non_pointer".to_string(),
                                ],
                                    );
                                    ctx.record_lowered_value_with_access_mode(
                                        "WriteBarrierElided",
                                        None,
                                        "write_barrier.elided_raw_f64_class_field",
                                        &stored,
                                        None,
                                        None,
                                        None,
                                        None,
                                        false,
                                        false,
                                        vec![
                                    "reason=raw_f64_class_field_pointer_free".to_string(),
                                    format!("class={}", class_name),
                                    format!("class_id={}", expected_class_id_str),
                                    format!("field={}", property),
                                    format!("field_index={}", field_idx_str),
                                    "receiver_proof=declared_named_receiver_guarded_exact_class"
                                        .to_string(),
                                    "field_layout=raw_f64_slot_array".to_string(),
                                    "pointer_bitmap=non_pointer".to_string(),
                                ],
                                    );
                                }

                                ctx.current_block = fallback_idx;
                                let blk = ctx.block();
                                // #5334 lever A: the guard already ran and FAILED in the
                                // entry block, so this cold arm is a pure guard-miss
                                // fallback. Outline the two operations it used to emit
                                // inline (record_fallback + by-name set) into ONE
                                // `js_class_field_set_fallback` call. Semantics are
                                // byte-identical; only the emitted IR shrinks (cold path
                                // → zero hot-loop cost). `obj_bits` keeps the full
                                // NaN-box tag; `key_raw` is POINTER_MASK-stripped — the
                                // same operands the two calls received.
                                blk.call_void(
                                    "js_class_field_set_fallback",
                                    &[
                                        (I64, &site_id),
                                        (I64, &obj_bits),
                                        (I64, &key_raw),
                                        (DOUBLE, &val_double),
                                    ],
                                );
                                blk.br(&merge_label);
                                if requires_raw_f64 {
                                    let fallback = LoweredValue {
                                        semantic: SemanticKind::JsValue,
                                        rep: NativeRep::JsValue,
                                        llvm_ty: DOUBLE,
                                        value: val_double.clone(),
                                    };
                                    ctx.record_lowered_value_with_access_mode_and_facts(
                                        "ClassFieldSet",
                                        None,
                                        "js_object_set_field_by_name",
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
                                                None,
                                                "rejected",
                                                "class_field_set_guard",
                                                Some(MaterializationReason::RuntimeApi),
                                            ),
                                            raw_f64_layout_fact(
                                                None,
                                                "invalidated",
                                                "runtime_api",
                                                Some(MaterializationReason::RuntimeApi),
                                            ),
                                        ],
                                        false,
                                        false,
                                        vec![
                                            format!("class={}", class_name),
                                            format!("field={}", property),
                                            format!("field_index={}", field_idx_str),
                                        ],
                                    );
                                }

                                ctx.current_block = merge_idx;
                                Ok(val_double)
                            },
                        );
                    }
                }
            }
            // #7154: the value expression can collect, and an evacuating minor
            // inside it relocates the receiver out from under `obj_box` --
            // `obj.k = f()` then writes `k` into abandoned from-space memory
            // and the field never appears on the object the program keeps.
            //
            // `across` rather than the plain form because the value is lowered
            // by `lower_value_for_dynamic_property_set` (an
            // `ExpectedNativeRep::JsValueBits` lowering plus a recorded
            // materialisation), which the operand list cannot produce.
            rooting::with_operands_rooted_across(
                ctx,
                &[object.as_ref()],
                &[value.as_ref()],
                |ctx| {
                    lower_value_for_dynamic_property_set(
                        ctx,
                        value,
                        "property_set.dynamic_value_bits",
                        "dynamic_property_set_helper_edge",
                    )
                },
                |ctx, vals, (val_double, _val_bits)| {
                    let obj_box = &vals[0];
                    // Intern the field name in the StringPool (same one the
                    // matching getter uses, so they share the global string).
                    let key_idx = ctx.strings.intern(property);
                    let key_handle_global =
                        format!("@{}", ctx.strings.entry(key_idx).handle_global);
                    let obj_bits = ctx.block().bitcast_double_to_i64(obj_box);
                    emit_nullish_write_guard(ctx, &obj_bits, property, "pset");
                    // Issue #618-followup: pass the FULL bits (including NaN-box
                    // tag) so the runtime can detect INT32-tagged class refs
                    // (`SQL.Aliased = Aliased` IIFE-static-property pattern from
                    // drizzle-orm). Pre-fix the AND-with-POINTER_MASK_I64 stripped
                    // the 0x7FFE tag, leaving the runtime with a small integer
                    // (the class id) — which fell into the small-handle dispatch
                    // path and silently dropped the assignment. The runtime now
                    // checks for top16 == 0x7FFE and routes to CLASS_DYNAMIC_PROPS.
                    let key_box = ctx.block().load(DOUBLE, &key_handle_global);
                    let key_bits = ctx.block().bitcast_double_to_i64(&key_box);
                    let key_raw = ctx.block().and(I64, &key_bits, POINTER_MASK_I64);
                    if matches!(property.as_str(), "caller" | "arguments") {
                        ctx.block().call_void(
                            "js_object_set_field_by_name",
                            &[(I64, &obj_bits), (I64, &key_raw), (DOUBLE, &val_double)],
                        );
                        return Ok(val_double);
                    }
                    let site_id = emit_typed_feedback_register_site(
                        ctx,
                        TypedFeedbackKind::PropertySet,
                        property,
                        TypedFeedbackContract::object_set_by_name(),
                    );
                    ctx.block().call_void(
                        "js_typed_feedback_object_set_field_by_name_fast",
                        &[
                            (I64, &site_id),
                            (I64, &obj_bits),
                            (I64, &key_raw),
                            (DOUBLE, &val_double),
                        ],
                    );
                    Ok(val_double)
                },
            )
        }

        // `obj.field` — generic object field read. We get the key string
        // handle from the StringPool (interned, so the same key across
        // multiple sites shares one allocation), unbox both the object
        // pointer and the key handle, then call
        // `js_object_get_field_by_name_f64`. The result is a raw f64
        // (which IS the NaN-boxed value for non-number fields — same bit
        // pattern, runtime callers re-interpret based on context).
        _ => unreachable!("expr/mod.rs dispatched a variant not handled by this submodule"),
    }
}
