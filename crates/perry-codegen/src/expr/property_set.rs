//! PropertySet (obj.prop = v).
//!
//! Extracted from `expr/mod.rs` to keep that file under the 2000-line cap.
//! Pure mechanical move — match arm bodies are verbatim copies, called from
//! `lower_expr`'s outer dispatch.

use anyhow::Result;
use perry_hir::Expr;
use perry_types::Type as HirType;

use crate::nanbox::POINTER_MASK_I64;
use crate::native_value::{
    BoundsState, BufferAccessMode, ExpectedNativeRep, LoweredValue, MaterializationReason,
    NativeRep, SemanticKind,
};
use crate::type_analysis::{
    expr_may_return_boxed_value_from_raw_f64_fallback, is_numeric_expr, receiver_class_name,
};
use crate::types::{DOUBLE, I1, I32, I64, I8, PTR};

use super::{
    emit_jsvalue_slot_store_on_block, emit_typed_feedback_register_site,
    expr_produces_non_pointer_bits_by_construction, lower_expr, lower_expr_native,
    raw_f64_layout_fact, try_lower_pod_field_set, unbox_to_i64, FnCtx, TypedFeedbackContract,
    TypedFeedbackKind,
};

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

fn class_has_computed_runtime_members(ctx: &FnCtx<'_>, class_name: &str) -> bool {
    ctx.classes
        .get(class_name)
        .is_some_and(|class| !class.computed_members.is_empty())
}

fn lower_runtime_property_set_by_name(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    property: &str,
    value: &Expr,
) -> Result<String> {
    let recv_box = lower_expr(ctx, object)?;
    let val_double = lower_expr(ctx, value)?;
    let key_idx = ctx.strings.intern(property);
    let key_handle_global = format!("@{}", ctx.strings.entry(key_idx).handle_global);
    let blk = ctx.block();
    let obj_bits = blk.bitcast_double_to_i64(&recv_box);
    let key_box = blk.load(DOUBLE, &key_handle_global);
    let key_bits = blk.bitcast_double_to_i64(&key_box);
    let property_id = blk.and(I64, &key_bits, POINTER_MASK_I64);
    blk.call_void(
        "js_object_set_field_by_property_id",
        &[(I64, &obj_bits), (I64, &property_id), (DOUBLE, &val_double)],
    );
    Ok(val_double)
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
                let arr_box = lower_expr(ctx, object)?;
                let val_double = lower_expr(ctx, value)?;
                let blk = ctx.block();
                let arr_bits = blk.bitcast_double_to_i64(&arr_box);
                let arr_handle = blk.and(I64, &arr_bits, POINTER_MASK_I64);
                // `arr.length = v` is a strict `Set(O,"length",v,true)`: a frozen
                // array's `length` is non-writable, so route to the throwing
                // variant instead of the silent internal helper.
                blk.call_void(
                    "js_array_set_length_strict",
                    &[(I64, &arr_handle), (DOUBLE, &val_double)],
                );
                return Ok(val_double);
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
                    // String-alias fix (mirror of `let y = x` in stmt/let_stmt.rs):
                    // a string-typed local stored into a scalar-replaced field's
                    // alloca slot aliases the same heap buffer. The runtime
                    // write-barrier choke point (runtime_store_jsvalue_slot) can't
                    // see this store because scalar replacement elides the real
                    // heap object, so mark the buffer shared here. Otherwise a
                    // later `s = s + suffix` mutates it in-place via
                    // js_string_append's refcount==1 fast path and corrupts this
                    // field. Only a `LocalGet` of a string-typed local can carry a
                    // uniquely-owned buffer (concat/literal results are shared).
                    if let Expr::LocalGet(src_id) = &**value {
                        if matches!(ctx.local_types.get(src_id), Some(HirType::String)) {
                            ctx.block().call_void(
                                "js_string_addref_if_heap_string",
                                &[(DOUBLE, &val_double)],
                            );
                        }
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
                        // String-alias fix: see the ScalarObjectFieldSet path
                        // above. `this.field = s` into a scalar-replaced ctor slot
                        // aliases the string buffer; mark it shared so a later
                        // self-append doesn't mutate it in-place and corrupt the
                        // field.
                        if let Expr::LocalGet(src_id) = &**value {
                            if matches!(ctx.local_types.get(src_id), Some(HirType::String)) {
                                ctx.block().call_void(
                                    "js_string_addref_if_heap_string",
                                    &[(DOUBLE, &val_double)],
                                );
                            }
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
            if let Some(class_name) = receiver_class_name(ctx, object) {
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
                        let recv_box = lower_expr(ctx, object)?;
                        let val_double = lower_expr(ctx, value)?;
                        let _ = ctx.block().call(
                            DOUBLE,
                            &fn_name,
                            &[(DOUBLE, &recv_box), (DOUBLE, &val_double)],
                        );
                        return Ok(val_double);
                    }
                }
                // Fast path: known class instance + plain instance field.
                // The runtime guard checks the receiver's class/shape and
                // descriptor state before this block touches the raw slot.
                if let Some(field_index) =
                    crate::type_analysis::class_field_global_index(ctx, &class_name, property)
                {
                    if let (Some(&expected_class_id), Some(keys_global_name)) = (
                        ctx.class_ids.get(&class_name),
                        ctx.class_keys_globals.get(&class_name).cloned(),
                    ) {
                        let recv_box = lower_expr(ctx, object)?;
                        let val_double = lower_expr(ctx, value)?;
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
                        let requires_raw_f64 = crate::type_analysis::class_field_declared_type(
                            ctx,
                            &class_name,
                            property,
                        )
                        .as_ref()
                        .is_some_and(crate::typed_shape::type_is_raw_f64_candidate);
                        let requires_raw_f64_str = if requires_raw_f64 { "1" } else { "0" };
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
                            let loop_fact = match object.as_ref() {
                                Expr::LocalGet(recv_id) => {
                                    crate::expr::class_field_loop_fact_lookup(
                                        &ctx.class_field_loop_facts,
                                        *recv_id,
                                        &class_name,
                                        property,
                                    )
                                    .filter(|(_, loop_idx)| *loop_idx == field_index)
                                    .map(|(fact, _)| {
                                        (fact.obj_ptr.clone(), fact.side_exit_label.clone())
                                    })
                                }
                                _ => None,
                            };
                            if let Some((obj_ptr, side_exit_label)) = loop_fact {
                                let field_idx_str = field_index.to_string();
                                let store_idx = ctx.new_block("class_field_loop_store.fast");
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
                                    let fields_base = blk.gep(I8, &obj_ptr, &[(I64, &header_skip)]);
                                    let field_ptr =
                                        blk.gep(DOUBLE, &fields_base, &[(I64, &field_idx_str)]);
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
                                        guard_id: "class_field_loop_preheader_check".to_string(),
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
                                        "receiver_proof=loop_preheader_shape_check".to_string(),
                                        "field_layout=raw_f64_slot_array".to_string(),
                                        "loop_versioning=class_field_fast_clone".to_string(),
                                        "rhs_numeric_guard=inline_plain_finite_check".to_string(),
                                        "store_guard_failure=side_exit_slow_restart".to_string(),
                                    ],
                                );
                                return Ok(val_double);
                            }
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
                            let (key_raw, expected_keys) = {
                                let blk = ctx.block();
                                let key_box = blk.load(DOUBLE, &key_handle_global);
                                let key_bits = blk.bitcast_double_to_i64(&key_box);
                                let key_raw = blk.and(I64, &key_bits, POINTER_MASK_I64);
                                let expected_keys =
                                    blk.load(I64, &format!("@{}", keys_global_name));
                                (key_raw, expected_keys)
                            };
                            ctx.block().call_void(
                                "js_class_field_set_ic",
                                &[
                                    (I64, &site_id),
                                    (DOUBLE, &recv_box),
                                    (I32, &expected_class_id_str),
                                    (I64, &expected_keys),
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
                        let (obj_bits, obj_handle, key_raw, expected_keys, val_bits) = {
                            let blk = ctx.block();
                            let obj_bits = blk.bitcast_double_to_i64(&recv_box);
                            let obj_handle = blk.and(I64, &obj_bits, POINTER_MASK_I64);
                            let key_box = blk.load(DOUBLE, &key_handle_global);
                            let key_bits = blk.bitcast_double_to_i64(&key_box);
                            let key_raw = blk.and(I64, &key_bits, POINTER_MASK_I64);
                            let expected_keys = blk.load(I64, &format!("@{}", keys_global_name));
                            let val_bits = blk.bitcast_double_to_i64(&val_double);
                            (obj_bits, obj_handle, key_raw, expected_keys, val_bits)
                        };
                        let fast_idx = ctx.new_block("class_field_set.fast");
                        let fallback_idx = ctx.new_block("class_field_set.fallback");
                        let merge_idx = ctx.new_block("class_field_set.merge");
                        let fast_label = ctx.block_label(fast_idx);
                        let fallback_label = ctx.block_label(fallback_idx);
                        let merge_label = ctx.block_label(merge_idx);

                        // #5093: inline shape pre-check, raw-f64 fields only. The
                        // boxed-store path keeps the guard call (its setter-in-
                        // chain handling and write barrier aren't reproduced
                        // inline). On a hit this branches straight to the raw
                        // store, skipping the call; on a miss the guard-call path
                        // below runs unchanged.
                        if requires_raw_f64 {
                            let _guardcall_label =
                                crate::expr::class_field_inline_guard::emit_class_field_inline_precheck(
                                    ctx,
                                    &obj_bits,
                                    &obj_handle,
                                    &expected_class_id_str,
                                    &expected_keys,
                                    field_index,
                                    true,
                                    Some(&val_bits),
                                    &fast_label,
                                );
                        }
                        let guard_ok = ctx.block().call(
                            I32,
                            "js_typed_feedback_class_field_set_guard",
                            &[
                                (I64, &site_id),
                                (DOUBLE, &recv_box),
                                (I32, &expected_class_id_str),
                                (I64, &expected_keys),
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
                                crate::target_layout::object_header_size_bytes(ctx.target_triple)
                                    .to_string();
                            let blk = ctx.block();
                            let obj_ptr = blk.inttoptr(I64, &obj_handle);
                            let fields_base = blk.gep(I8, &obj_ptr, &[(I64, &header_skip)]);
                            let field_ptr = blk.gep(DOUBLE, &fields_base, &[(I64, &field_idx_str)]);
                            let raw_stored_value = if requires_raw_f64 {
                                // Guarded raw-f64 slots are pointer-free by typed
                                // shape descriptor; non-number writes miss the
                                // guard and use the boxed setter fallback.
                                // GC_STORE_AUDIT(POINTER_FREE): typed raw-f64 class
                                // slots contain numbers only.
                                let numeric_value =
                                    canonicalize_raw_f64_numeric_store_value(blk, &val_double);
                                blk.store(DOUBLE, &numeric_value, &field_ptr);
                                Some(numeric_value)
                            } else {
                                // #5334 lever D: skip the barrier when the value
                                // is a non-pointer by construction.
                                let field_addr = blk.ptrtoint(&field_ptr, I64);
                                emit_jsvalue_slot_store_on_block(
                                    blk,
                                    &field_ptr,
                                    &val_double,
                                    &obj_handle,
                                    &field_idx_str,
                                    true,
                                    &obj_bits,
                                    &field_addr,
                                    field_set_barrier_needed,
                                );
                                None
                            };
                            blk.br(&merge_label);
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
                        return Ok(val_double);
                    }
                }
            }
            let obj_box = lower_expr(ctx, object)?;
            let (val_double, _val_bits) = lower_value_for_dynamic_property_set(
                ctx,
                value,
                "property_set.dynamic_value_bits",
                "dynamic_property_set_helper_edge",
            )?;
            // Intern the field name in the StringPool (same one the
            // matching getter uses, so they share the global string).
            let key_idx = ctx.strings.intern(property);
            let key_handle_global = format!("@{}", ctx.strings.entry(key_idx).handle_global);
            let obj_bits = ctx.block().bitcast_double_to_i64(&obj_box);
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
