//! The sloppy-mode class-field store fast paths, split from
//! `property_set.rs` to keep it under the 2000-line file cap.

use super::*;

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
            crate::typed_shape::load_class_shape_id(ctx, &class_name, &keys_global_name);

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
            // past the user pointer (16 on LP64 and ILP32 since #8047) —
            // same derivation as the strict arm and the runtime setter.
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
pub(super) fn try_lower_sloppy_class_field_boxed_store(
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
            crate::typed_shape::load_class_shape_id(ctx, class_name, keys_global_name);

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
                "class_field_set",
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
